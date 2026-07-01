use super::*;
use std::borrow::Cow;

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
    pub base_url: Option<&'a std::path::Path>,
    pub root_url: Option<&'a std::path::Path>,
    pub important: bool,
    pub layer_order: Option<usize>,
    pub specificity: u32,
    pub scope_proximity: usize,
    pub stylesheet_index: usize,
    pub rule_order: usize,
    pub declaration_order: usize,
}

pub(super) fn cascaded_declarations_from(
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
/// scoped proximity, and source order. Reasyprint currently models UA, user,
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
fn layer_precedence_rank(layer_order: Option<usize>, important: bool) -> usize {
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
fn scope_proximity_rank(scope_proximity: usize) -> usize {
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
/// Reasyprint currently has no transition or animation origin, so the modeled
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
fn is_shadowed_by_later_var_declaration(
    declarations: &[CascadedDeclaration<'_>],
    index: usize,
    name: &str,
) -> bool {
    declarations[index + 1..].iter().any(|declaration| {
        declaration.name.as_ref() == name && trim_css_value(&declaration.value).contains("var(")
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
fn declarations_after_css_wide_rollbacks<'a>(
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
fn expand_modeled_shorthands<'a>(
    declarations: &'a [CascadedDeclaration<'a>],
    direction: Direction,
    writing_mode: WritingMode,
) -> Vec<CascadedDeclaration<'a>> {
    let mut expanded = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        if declaration.value.contains("var(")
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

fn expand_box_edge_shorthand(name: &str, value: &str) -> Option<Vec<(&'static str, String)>> {
    let names = match name {
        "margin" => ["margin-top", "margin-right", "margin-bottom", "margin-left"],
        "padding" => [
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
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

fn expand_simple_modeled_shorthand(
    name: &str,
    value: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    match name {
        "gap" => expand_gap_shorthand(value),
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
        "inset-block" | "inset-inline" | "inset-block-start" | "inset-block-end"
        | "inset-inline-start" | "inset-inline-end" => {
            expand_logical_box_edge_values(name, value, "inset", direction, writing_mode)
        }
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

fn expand_logical_size_value(
    name: &str,
    value: &str,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    Some(vec![(
        logical_size_physical_longhand(name, writing_mode)?,
        trim_css_value(value).to_string(),
    )])
}

/// Return the physical sizing longhand addressed by a logical size property.
///
/// CSS Logical Properties maps inline/block size longhands through the
/// element's writing mode:
/// <https://www.w3.org/TR/css-logical-1/#dimension-properties>.
fn logical_size_physical_longhand(name: &str, writing_mode: WritingMode) -> Option<&'static str> {
    let inline_axis = inline_start_side(writing_mode, Direction::Ltr).axis();
    let block_axis = block_start_side(writing_mode).axis();
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

fn size_longhand_for_axis(axis: PhysicalAxis) -> &'static str {
    match axis {
        PhysicalAxis::Horizontal => "width",
        PhysicalAxis::Vertical => "height",
    }
}

fn min_size_longhand_for_axis(axis: PhysicalAxis) -> &'static str {
    match axis {
        PhysicalAxis::Horizontal => "min-width",
        PhysicalAxis::Vertical => "min-height",
    }
}

fn max_size_longhand_for_axis(axis: PhysicalAxis) -> &'static str {
    match axis {
        PhysicalAxis::Horizontal => "max-width",
        PhysicalAxis::Vertical => "max-height",
    }
}

struct ListStyleShorthandComponents {
    style_type: String,
    position: String,
    image: String,
}

/// Expands the CSS Lists `list-style` shorthand into its three longhands.
///
/// CSS Lists Level 3 defines `list-style` as an unordered shorthand for
/// `list-style-type`, `list-style-position`, and `list-style-image`, with
/// ambiguous `none` tokens assigned to whichever of type/image are not
/// otherwise specified:
/// <https://www.w3.org/TR/css-lists-3/#propdef-list-style>.
fn expand_list_style_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let components = parse_list_style_shorthand(value)?;
    Some(vec![
        ("list-style-type", components.style_type),
        ("list-style-position", components.position),
        ("list-style-image", components.image),
    ])
}

fn parse_list_style_shorthand(value: &str) -> Option<ListStyleShorthandComponents> {
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

fn parse_list_style_image_component(value: &str) -> Option<Option<String>> {
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
fn expand_logical_box_edge_values(
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

fn physical_inset_side_longhand(side: BoxSide) -> &'static str {
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
fn expand_border_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
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
fn expand_border_side_shorthand(name: &str, value: &str) -> Option<Vec<(&'static str, String)>> {
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

struct BorderShorthandComponents {
    width: String,
    style: String,
    color: String,
}

fn border_shorthand_components(value: &str) -> Option<BorderShorthandComponents> {
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
fn expand_outline_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
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
fn expand_logical_border_shorthand(
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
fn expand_logical_border_side_shorthand(
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
fn expand_logical_border_axis_values(
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

fn physical_border_side_shorthand(side: BorderSide) -> &'static str {
    match side {
        BorderSide::Top => "border-top",
        BorderSide::Right => "border-right",
        BorderSide::Bottom => "border-bottom",
        BorderSide::Left => "border-left",
    }
}

fn physical_border_side_component(side: BorderSide, component: &str) -> &'static str {
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
fn expand_border_radius_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
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
fn split_border_radius_groups(value: &str) -> Option<(Vec<String>, Vec<String>)> {
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

fn split_css_top_level_slashes(value: &str) -> Vec<&str> {
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
fn split_top_level_once(value: &str, delimiter: char) -> Option<(&str, &str)> {
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
fn expand_four_radius_components(values: &[String]) -> Option<Vec<String>> {
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
fn radius_pair(horizontal: &str, vertical: &str) -> String {
    if horizontal == vertical {
        horizontal.to_string()
    } else {
        format!("{horizontal} {vertical}")
    }
}

/// Expand `corner-shape` into physical per-corner shape longhands.
///
/// CSS Borders and Box Decorations Level 4 uses the same four-corner expansion
/// order as `border-radius`:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape-shorthand>.
fn expand_corner_shape_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let parts = split_css_component_values(value)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let expanded = expand_four_radius_components(&parts)?;
    Some(vec![
        ("corner-top-left-shape", expanded[0].clone()),
        ("corner-top-right-shape", expanded[1].clone()),
        ("corner-bottom-right-shape", expanded[2].clone()),
        ("corner-bottom-left-shape", expanded[3].clone()),
    ])
}

/// Expand `corner` into radius and shape longhands.
///
/// CSS Borders and Box Decorations Level 4 defines `corner` as a shorthand for
/// all `border-*-radius` and `corner-*-shape` longhands:
/// <https://drafts.csswg.org/css-borders-4/#corner-shorthand>.
fn expand_corner_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let groups = split_css_top_level_slashes(value);
    if groups.is_empty() || groups.len() > 4 {
        return None;
    }
    let parsed = groups
        .iter()
        .map(|group| split_corner_radius_shape_component(group))
        .collect::<Option<Vec<_>>>()?;
    let expanded = match parsed.as_slice() {
        [all] => [all, all, all, all],
        [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
        [top_left, horizontal, bottom_right] => [top_left, horizontal, bottom_right, horizontal],
        [top_left, top_right, bottom_right, bottom_left] => {
            [top_left, top_right, bottom_right, bottom_left]
        }
        _ => return None,
    };
    let corner_names = [
        ("border-top-left-radius", "corner-top-left-shape"),
        ("border-top-right-radius", "corner-top-right-shape"),
        ("border-bottom-right-radius", "corner-bottom-right-shape"),
        ("border-bottom-left-radius", "corner-bottom-left-shape"),
    ];
    let mut declarations = Vec::with_capacity(8);
    for ((radius_name, shape_name), (radius, shape)) in corner_names.into_iter().zip(expanded) {
        declarations.push((radius_name, radius.clone()));
        declarations.push((shape_name, shape.clone()));
    }
    Some(declarations)
}

fn split_corner_radius_shape_component(value: &str) -> Option<(String, String)> {
    let mut shape = None;
    let mut radius_parts = Vec::new();
    for part in split_css_component_values(value) {
        match part.to_ascii_lowercase().as_str() {
            "round" | "bevel" | "scoop" | "notch" if shape.is_none() => {
                shape = Some(part.to_string());
            }
            _ => radius_parts.push(part.to_string()),
        }
    }
    (!radius_parts.is_empty()).then_some((radius_parts.join(" "), shape.unwrap_or("round".into())))
}

fn expand_gap_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let parts = split_css_component_values(value);
    match parts.as_slice() {
        [row] if parse_gap(row, ROOT_FONT_SIZE_PT).is_some() => Some(vec![
            ("row-gap", (*row).to_string()),
            ("column-gap", (*row).to_string()),
        ]),
        [row, column]
            if parse_gap(row, ROOT_FONT_SIZE_PT).is_some()
                && parse_gap(column, ROOT_FONT_SIZE_PT).is_some() =>
        {
            Some(vec![
                ("row-gap", (*row).to_string()),
                ("column-gap", (*column).to_string()),
            ])
        }
        _ => None,
    }
}

/// Parses the `gap` shorthand into row and column gap computed values.
///
/// CSS Box Alignment defines `gap` as `<'row-gap'> <'column-gap'>?`; the
/// shorthand is invalid as a whole if either component is invalid:
/// <https://www.w3.org/TR/css-align-3/#gap-shorthand>.
fn parse_gap_shorthand_components(
    parts: &[&str],
    font_size: f32,
) -> Option<(ComputedGap, ComputedGap)> {
    match parts {
        [row] => {
            let row = parse_gap(row, font_size)?;
            Some((row, row))
        }
        [row, column] => Some((parse_gap(row, font_size)?, parse_gap(column, font_size)?)),
        _ => None,
    }
}

fn parse_grid_track_list(value: &str, font_size: f32) -> Option<GridTrackList> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(GridTrackList::None);
    }
    let (components, trailing_names) = parse_grid_track_list_components(value, font_size)?;
    (!components.is_empty()).then_some(GridTrackList::Tracks {
        components,
        trailing_names,
    })
}

fn expand_grid_template_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(vec![
            ("grid-template-rows", "none".to_string()),
            ("grid-template-columns", "none".to_string()),
            ("grid-template-areas", "none".to_string()),
        ]);
    }

    let (rows, columns, has_slash) = split_top_level_once(value, '/')
        .map(|(rows, columns)| (trim_css_value(rows), trim_css_value(columns), true))
        .unwrap_or((value, "none", false));
    if rows.is_empty() || columns.is_empty() {
        return None;
    }
    let rows_have_areas = split_css_component_values(rows)
        .iter()
        .any(|token| css_string_token_contents(token).is_some());
    if !rows_have_areas {
        if !has_slash {
            return None;
        }
        parse_grid_track_list(rows, ROOT_FONT_SIZE_PT)?;
        parse_grid_track_list(columns, ROOT_FONT_SIZE_PT)?;
        return Some(vec![
            ("grid-template-rows", rows.to_string()),
            ("grid-template-columns", columns.to_string()),
            ("grid-template-areas", "none".to_string()),
        ]);
    }

    let (row_tracks, areas) = parse_grid_template_ascii_rows(rows)?;
    parse_grid_track_list(&row_tracks, ROOT_FONT_SIZE_PT)?;
    parse_grid_template_areas(&areas)?;
    parse_grid_track_list(columns, ROOT_FONT_SIZE_PT)?;
    Some(vec![
        ("grid-template-rows", row_tracks),
        ("grid-template-columns", columns.to_string()),
        ("grid-template-areas", areas),
    ])
}

fn expand_grid_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let value = trim_css_value(value);
    let (left, right) = split_top_level_once(value, '/')
        .map(|(left, right)| (trim_css_value(left), trim_css_value(right)))
        .unwrap_or((value, ""));
    if left.is_empty() || right.is_empty() {
        let mut expanded = expand_grid_template_shorthand(value)?;
        expanded.extend(grid_implicit_initial_longhands());
        return Some(expanded);
    }
    if let Some((dense, auto_tracks)) = parse_grid_auto_flow_shorthand_side(left) {
        parse_grid_track_list(right, ROOT_FONT_SIZE_PT)?;
        return Some(vec![
            ("grid-template-rows", "none".to_string()),
            ("grid-template-columns", right.to_string()),
            ("grid-template-areas", "none".to_string()),
            (
                "grid-auto-flow",
                if dense { "row dense" } else { "row" }.to_string(),
            ),
            ("grid-auto-rows", auto_tracks),
            ("grid-auto-columns", "auto".to_string()),
        ]);
    }
    if let Some((dense, auto_tracks)) = parse_grid_auto_flow_shorthand_side(right) {
        parse_grid_track_list(left, ROOT_FONT_SIZE_PT)?;
        return Some(vec![
            ("grid-template-rows", left.to_string()),
            ("grid-template-columns", "none".to_string()),
            ("grid-template-areas", "none".to_string()),
            (
                "grid-auto-flow",
                if dense { "column dense" } else { "column" }.to_string(),
            ),
            ("grid-auto-rows", "auto".to_string()),
            ("grid-auto-columns", auto_tracks),
        ]);
    }
    let mut expanded = expand_grid_template_shorthand(value)?;
    expanded.extend(grid_implicit_initial_longhands());
    Some(expanded)
}

fn grid_implicit_initial_longhands() -> Vec<(&'static str, String)> {
    vec![
        ("grid-auto-flow", "row".to_string()),
        ("grid-auto-rows", "auto".to_string()),
        ("grid-auto-columns", "auto".to_string()),
    ]
}

fn parse_grid_auto_flow_shorthand_side(value: &str) -> Option<(bool, String)> {
    let tokens = split_css_component_values(value);
    let auto_flow_index = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("auto-flow"))?;
    let mut dense = false;
    for token in &tokens[..auto_flow_index] {
        if token.eq_ignore_ascii_case("dense") && !dense {
            dense = true;
        } else {
            return None;
        }
    }
    let mut track_start = auto_flow_index + 1;
    if tokens
        .get(track_start)
        .is_some_and(|token| token.eq_ignore_ascii_case("dense"))
    {
        dense = true;
        track_start += 1;
    }
    if tokens[track_start..]
        .iter()
        .any(|token| token.eq_ignore_ascii_case("dense") || token.eq_ignore_ascii_case("auto-flow"))
    {
        return None;
    }
    let auto_tracks = if track_start == tokens.len() {
        "auto".to_string()
    } else {
        let auto_tracks = tokens[track_start..].join(" ");
        parse_grid_auto_track_list(&auto_tracks, ROOT_FONT_SIZE_PT)?;
        auto_tracks
    };
    Some((dense, auto_tracks))
}

fn parse_grid_template_ascii_rows(value: &str) -> Option<(String, String)> {
    let tokens = split_css_component_values(value);
    let mut index = 0usize;
    let mut row_track_tokens = Vec::new();
    let mut area_tokens = Vec::new();

    while index < tokens.len() {
        while index < tokens.len() && parse_grid_line_names(tokens[index]).is_some() {
            row_track_tokens.push(tokens[index].to_string());
            index += 1;
        }

        let area = tokens
            .get(index)
            .and_then(|token| css_string_token_contents(token))?;
        area_tokens.push(css_quote_string(area));
        index += 1;

        if index < tokens.len()
            && css_string_token_contents(tokens[index]).is_none()
            && parse_grid_line_names(tokens[index]).is_none()
        {
            parse_grid_track_size(tokens[index], ROOT_FONT_SIZE_PT)?;
            row_track_tokens.push(tokens[index].to_string());
            index += 1;
        } else {
            row_track_tokens.push("auto".to_string());
        }

        while index < tokens.len() && parse_grid_line_names(tokens[index]).is_some() {
            row_track_tokens.push(tokens[index].to_string());
            index += 1;
        }
    }

    (!area_tokens.is_empty()).then(|| (row_track_tokens.join(" "), area_tokens.join(" ")))
}

fn css_string_token_contents(token: &str) -> Option<&str> {
    let token = trim_css_value(token);
    token
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            token
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
}

fn css_quote_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn parse_grid_track_list_components(
    value: &str,
    font_size: f32,
) -> Option<(Vec<GridTrackListComponent>, GridLineNames)> {
    parse_grid_track_list_components_with_options(value, font_size, true)
}

fn parse_grid_track_list_components_with_options(
    value: &str,
    font_size: f32,
    allow_repeat: bool,
) -> Option<(Vec<GridTrackListComponent>, GridLineNames)> {
    let mut components = Vec::new();
    let mut pending_names = Vec::new();
    let mut saw_auto_repeat = false;
    for token in split_css_component_values(value) {
        if let Some(names) = parse_grid_line_names(token) {
            pending_names.extend(names);
            continue;
        }
        if allow_repeat && let Some(repeat) = parse_grid_repeat(token, font_size) {
            if matches!(
                repeat.count,
                GridRepeatCount::AutoFill | GridRepeatCount::AutoFit
            ) {
                if saw_auto_repeat {
                    return None;
                }
                saw_auto_repeat = true;
            }
            components.push(GridTrackListComponent::Repeat(
                std::mem::take(&mut pending_names),
                repeat,
            ));
            continue;
        }
        if let Some(size) = parse_grid_track_size(token, font_size) {
            components.push(GridTrackListComponent::Track(
                std::mem::take(&mut pending_names),
                size,
            ));
            continue;
        }
        return None;
    }
    if !pending_names.is_empty() && components.is_empty() {
        return None;
    }
    Some((components, pending_names))
}

fn parse_grid_line_names(token: &str) -> Option<Vec<String>> {
    let token = trim_css_value(token);
    let inner = token.strip_prefix('[')?.strip_suffix(']')?;
    let mut names = Vec::new();
    for name in inner.split_whitespace() {
        if !grid_placement_name_is_custom_ident(name) {
            return None;
        }
        names.push(name.to_string());
    }
    Some(names)
}

fn parse_grid_repeat(token: &str, font_size: f32) -> Option<GridRepeat> {
    let inner = grid_function_body(trim_css_value(token), "repeat")?;
    let (count, tracks) = split_top_level_once(inner, ',')?;
    let count = match trim_css_value(count).to_ascii_lowercase().as_str() {
        "auto-fill" => GridRepeatCount::AutoFill,
        "auto-fit" => GridRepeatCount::AutoFit,
        value => GridRepeatCount::Number(value.parse::<u16>().ok().filter(|count| *count > 0)?),
    };
    let (tracks, trailing_names) =
        parse_grid_track_list_components_with_options(tracks, font_size, false)?;
    if tracks.is_empty() || !grid_repeat_tracks_are_valid(count, &tracks) {
        return None;
    }
    Some(GridRepeat {
        count,
        tracks,
        trailing_names,
    })
}

/// Validate the CSS Grid `repeat()` grammar after parsing its track fragment.
///
/// CSS Grid forbids nested `repeat()` fragments, and `auto-fill`/`auto-fit`
/// use the stricter `<fixed-size>` grammar because auto-repeat counts are
/// derived from definite track breadths:
/// <https://www.w3.org/TR/css-grid-1/#repeat-notation>.
fn grid_repeat_tracks_are_valid(count: GridRepeatCount, tracks: &[GridTrackListComponent]) -> bool {
    tracks.iter().all(|component| match component {
        GridTrackListComponent::Track(_, size) => {
            !matches!(count, GridRepeatCount::AutoFill | GridRepeatCount::AutoFit)
                || grid_track_size_is_fixed_for_auto_repeat(*size)
        }
        GridTrackListComponent::Repeat(_, _) => false,
    })
}

fn grid_track_size_is_fixed_for_auto_repeat(size: GridTrackSize) -> bool {
    grid_min_track_breadth_is_fixed(size.min)
        || (grid_min_track_breadth_is_inflexible(size.min)
            && grid_max_track_breadth_is_fixed(size.max))
}

fn grid_min_track_breadth_is_fixed(value: GridMinTrackBreadth) -> bool {
    matches!(value, GridMinTrackBreadth::LengthPercentage(_))
}

fn grid_min_track_breadth_is_inflexible(value: GridMinTrackBreadth) -> bool {
    matches!(
        value,
        GridMinTrackBreadth::Auto
            | GridMinTrackBreadth::MinContent
            | GridMinTrackBreadth::MaxContent
            | GridMinTrackBreadth::LengthPercentage(_)
    )
}

fn grid_max_track_breadth_is_fixed(value: GridMaxTrackBreadth) -> bool {
    matches!(value, GridMaxTrackBreadth::LengthPercentage(_))
}

fn parse_grid_auto_track_list(value: &str, font_size: f32) -> Option<GridAutoTrackList> {
    let tracks = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_grid_track_size(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    (!tracks.is_empty()).then_some(GridAutoTrackList { tracks })
}

fn parse_grid_track_size(value: &str, font_size: f32) -> Option<GridTrackSize> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    if let Some(inner) = grid_function_body(value, "minmax") {
        let (min, max) = split_top_level_once(inner, ',')?;
        return Some(GridTrackSize {
            min: parse_grid_min_track_breadth(min, font_size)?,
            max: parse_grid_max_track_breadth(max, font_size)?,
        });
    }
    if let Some(inner) = grid_function_body(value, "fit-content") {
        return Some(GridTrackSize {
            min: GridMinTrackBreadth::Auto,
            max: GridMaxTrackBreadth::FitContent(parse_computed_length_percentage(
                inner, font_size,
            )?),
        });
    }
    if let Some(flex) = parse_grid_flex(&lower) {
        return Some(GridTrackSize {
            min: GridMinTrackBreadth::Auto,
            max: GridMaxTrackBreadth::Flex(flex),
        });
    }
    match lower.as_str() {
        "auto" => Some(GridTrackSize::AUTO),
        "min-content" => Some(GridTrackSize {
            min: GridMinTrackBreadth::MinContent,
            max: GridMaxTrackBreadth::MinContent,
        }),
        "max-content" => Some(GridTrackSize {
            min: GridMinTrackBreadth::MaxContent,
            max: GridMaxTrackBreadth::MaxContent,
        }),
        _ => {
            let length = parse_computed_length_percentage(value, font_size)?;
            Some(GridTrackSize {
                min: GridMinTrackBreadth::LengthPercentage(length),
                max: GridMaxTrackBreadth::LengthPercentage(length),
            })
        }
    }
}

fn parse_grid_min_track_breadth(value: &str, font_size: f32) -> Option<GridMinTrackBreadth> {
    let value = trim_css_value(value);
    match value.to_ascii_lowercase().as_str() {
        "auto" => Some(GridMinTrackBreadth::Auto),
        "min-content" => Some(GridMinTrackBreadth::MinContent),
        "max-content" => Some(GridMinTrackBreadth::MaxContent),
        _ => parse_computed_length_percentage(value, font_size)
            .map(GridMinTrackBreadth::LengthPercentage),
    }
}

fn parse_grid_max_track_breadth(value: &str, font_size: f32) -> Option<GridMaxTrackBreadth> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    if let Some(flex) = parse_grid_flex(&lower) {
        return Some(GridMaxTrackBreadth::Flex(flex));
    }
    if let Some(inner) = grid_function_body(value, "fit-content") {
        return parse_computed_length_percentage(inner, font_size)
            .map(GridMaxTrackBreadth::FitContent);
    }
    match lower.as_str() {
        "auto" => Some(GridMaxTrackBreadth::Auto),
        "min-content" => Some(GridMaxTrackBreadth::MinContent),
        "max-content" => Some(GridMaxTrackBreadth::MaxContent),
        _ => parse_computed_length_percentage(value, font_size)
            .map(GridMaxTrackBreadth::LengthPercentage),
    }
}

fn parse_grid_flex(lower: &str) -> Option<f32> {
    let value = lower.strip_suffix("fr")?;
    value.parse::<f32>().ok().filter(|value| *value >= 0.0)
}

fn grid_function_body<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let value = trim_css_value(value);
    let prefix_len = name.len();
    let prefix = value.get(..prefix_len)?;
    if !prefix.eq_ignore_ascii_case(name) {
        return None;
    }
    value[prefix_len..]
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')
}

fn parse_grid_template_areas(value: &str) -> Option<GridTemplateAreas> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(GridTemplateAreas::None);
    }
    let mut rows = Vec::new();
    for token in split_css_component_values(value) {
        let token = trim_css_value(token);
        let row = token
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                token
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })?;
        let cells = parse_grid_template_area_row(row)?;
        if cells.is_empty() {
            return None;
        }
        rows.push(GridTemplateAreaRow { cells });
    }
    let width = rows.first()?.cells.len();
    (rows.iter().all(|row| row.cells.len() == width) && grid_template_areas_are_rectangular(&rows))
        .then_some(GridTemplateAreas::Areas(rows))
}

/// Parses one `grid-template-areas` string token into named and null cells.
///
/// CSS Grid parses each string as a whitespace-separated row of named cell
/// tokens or null cell tokens. Any unrecognized sequence is invalid:
/// <https://www.w3.org/TR/css-grid-1/#typedef-grid-template-areas-string>.
fn parse_grid_template_area_row(row: &str) -> Option<Vec<Option<String>>> {
    let mut cells = Vec::new();
    let mut chars = row.chars().peekable();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        if ch == '.' {
            while matches!(chars.peek(), Some('.')) {
                chars.next();
            }
            cells.push(None);
            continue;
        }
        if grid_template_area_name_code_point(ch) {
            let mut name = String::new();
            while let Some(ch) = chars.peek().copied() {
                if !grid_template_area_name_code_point(ch) {
                    break;
                }
                name.push(ch);
                chars.next();
            }
            cells.push(Some(name));
            continue;
        }
        return None;
    }
    Some(cells)
}

fn grid_template_area_name_code_point(ch: char) -> bool {
    ch == '-' || ch == '_' || ch.is_ascii_alphanumeric() || !ch.is_ascii()
}

/// Validates the CSS Grid requirement that named area cells form rectangles.
///
/// If any named grid area spans multiple cells, those cells must define a
/// single filled-in rectangle and no disconnected fragments:
/// <https://www.w3.org/TR/css-grid-1/#grid-template-areas-property>.
fn grid_template_areas_are_rectangular(rows: &[GridTemplateAreaRow]) -> bool {
    let mut areas: Vec<GridTemplateAreaParseBounds> = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            let Some(name) = cell else {
                continue;
            };
            if let Some(area) = areas.iter_mut().find(|area| area.name == *name) {
                area.row_start = area.row_start.min(row_index);
                area.row_end = area.row_end.max(row_index);
                area.column_start = area.column_start.min(column_index);
                area.column_end = area.column_end.max(column_index);
            } else {
                areas.push(GridTemplateAreaParseBounds {
                    name: name.clone(),
                    row_start: row_index,
                    row_end: row_index,
                    column_start: column_index,
                    column_end: column_index,
                });
            }
        }
    }
    areas.into_iter().all(|area| {
        (area.row_start..=area.row_end).all(|row_index| {
            (area.column_start..=area.column_end).all(|column_index| {
                rows.get(row_index)
                    .and_then(|row| row.cells.get(column_index))
                    .is_some_and(|cell| cell.as_ref() == Some(&area.name))
            })
        })
    })
}

#[derive(Debug, Clone)]
struct GridTemplateAreaParseBounds {
    name: String,
    row_start: usize,
    row_end: usize,
    column_start: usize,
    column_end: usize,
}

fn parse_grid_auto_flow(value: &str) -> Option<GridAutoFlow> {
    let mut axis = None;
    let mut dense = false;
    for token in split_css_component_values(value) {
        match token.to_ascii_lowercase().as_str() {
            "row" if axis.replace("row").is_none() => {}
            "column" if axis.replace("column").is_none() => {}
            "dense" if !dense => dense = true,
            _ => return None,
        }
    }
    match (axis, dense) {
        (None, false) => None,
        (Some("row") | None, true) => Some(GridAutoFlow::RowDense),
        (Some("row"), false) => Some(GridAutoFlow::Row),
        (Some("column"), false) => Some(GridAutoFlow::Column),
        (Some("column"), true) => Some(GridAutoFlow::ColumnDense),
        _ => None,
    }
}

fn parse_grid_placement(value: &str) -> Option<GridPlacement> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("auto") {
        return Some(GridPlacement::Auto);
    }
    if parts.iter().any(|part| part.eq_ignore_ascii_case("span")) {
        parse_grid_span_placement(&parts).map(GridPlacement::Span)
    } else {
        parse_grid_line_placement(&parts).map(GridPlacement::Line)
    }
}

fn expand_grid_placement_shorthand(
    value: &str,
    start_name: &'static str,
    end_name: &'static str,
) -> Option<Vec<(&'static str, String)>> {
    let (start, end) = split_top_level_once(value, '/')
        .map(|(start, end)| (trim_css_value(start), trim_css_value(end).to_string()))
        .unwrap_or_else(|| {
            let start = trim_css_value(value);
            let end = if grid_placement_is_custom_ident(start) {
                start.to_string()
            } else {
                "auto".to_string()
            };
            (start, end)
        });
    if start.is_empty()
        || end.is_empty()
        || parse_grid_placement(start).is_none()
        || parse_grid_placement(&end).is_none()
    {
        return None;
    }
    Some(vec![(start_name, start.to_string()), (end_name, end)])
}

fn expand_grid_area_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let parts = split_css_top_level_slashes(value);
    if parts.is_empty() || parts.len() > 4 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    if parts
        .iter()
        .any(|part| parse_grid_placement(part).is_none())
    {
        return None;
    }
    let row_start = parts[0];
    let column_start = parts.get(1).copied().unwrap_or_else(|| {
        if grid_placement_is_custom_ident(row_start) {
            row_start
        } else {
            "auto"
        }
    });
    let row_end = parts.get(2).copied().unwrap_or_else(|| {
        if grid_placement_is_custom_ident(row_start) {
            row_start
        } else {
            "auto"
        }
    });
    let column_end = parts.get(3).copied().unwrap_or_else(|| {
        if grid_placement_is_custom_ident(column_start) {
            column_start
        } else {
            "auto"
        }
    });
    Some(vec![
        ("grid-row-start", row_start.to_string()),
        ("grid-column-start", column_start.to_string()),
        ("grid-row-end", row_end.to_string()),
        ("grid-column-end", column_end.to_string()),
    ])
}

fn grid_placement_is_custom_ident(value: &str) -> bool {
    matches!(
        parse_grid_placement(value),
        Some(GridPlacement::Line(GridLinePlacement {
            name: Some(_),
            index: None
        }))
    )
}

fn grid_placement_name_is_custom_ident(value: &str) -> bool {
    is_css_identifier(value)
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "auto" | "span" | "initial" | "inherit" | "unset" | "revert" | "revert-layer"
        )
}

fn parse_grid_line_placement(parts: &[&str]) -> Option<GridLinePlacement> {
    let mut name = None;
    let mut index = None;
    for part in parts {
        if part.eq_ignore_ascii_case("auto") || part.eq_ignore_ascii_case("span") {
            return None;
        }
        if let Ok(value) = part.parse::<i32>() {
            if value == 0 || index.replace(value).is_some() {
                return None;
            }
            continue;
        }
        if !grid_placement_name_is_custom_ident(part) {
            return None;
        }
        if name.replace((*part).to_string()).is_some() {
            return None;
        }
    }
    (name.is_some() || index.is_some()).then_some(GridLinePlacement { name, index })
}

fn parse_grid_span_placement(parts: &[&str]) -> Option<GridSpanPlacement> {
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut saw_span = false;
    let mut name = None;
    let mut span = None;
    for part in parts {
        if part.eq_ignore_ascii_case("span") {
            if saw_span {
                return None;
            }
            saw_span = true;
            continue;
        }
        if part.eq_ignore_ascii_case("auto") {
            return None;
        }
        if let Ok(value) = part.parse::<u16>() {
            if value == 0 || span.replace(value).is_some() {
                return None;
            }
            continue;
        }
        if !grid_placement_name_is_custom_ident(part) {
            return None;
        }
        if name.replace((*part).to_string()).is_some() {
            return None;
        }
    }
    (saw_span && (name.is_some() || span.is_some())).then_some(GridSpanPlacement { name, span })
}

fn expand_flex_flow_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let mut direction = "row";
    let mut wrap = "nowrap";
    let mut saw_direction = false;
    let mut saw_wrap = false;
    for token in trim_css_value(value).split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "row" | "row-reverse" | "column" | "column-reverse" if !saw_direction => {
                direction = token;
                saw_direction = true;
            }
            "nowrap" | "wrap" | "wrap-reverse" if !saw_wrap => {
                wrap = token;
                saw_wrap = true;
            }
            _ => return None,
        }
    }
    (saw_direction || saw_wrap).then(|| {
        vec![
            ("flex-direction", direction.to_string()),
            ("flex-wrap", wrap.to_string()),
        ]
    })
}

fn expand_flex_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let (grow, shrink, basis) = parse_flex_shorthand_components(value)?;
    Some(vec![
        ("flex-grow", grow),
        ("flex-shrink", shrink),
        ("flex-basis", basis),
    ])
}

/// Parses the CSS `flex` shorthand into its longhand component strings.
///
/// CSS Flexbox defines `flex` as `none | [ <flex-grow> <flex-shrink>? ||
/// <flex-basis> ]`, with omitted shrink defaulting to `1` and omitted basis
/// defaulting to `0%`:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-property>.
fn parse_flex_shorthand_components(value: &str) -> Option<(String, String, String)> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(("0".to_string(), "0".to_string(), "auto".to_string()));
    }
    if value.eq_ignore_ascii_case("auto") {
        return Some(("1".to_string(), "1".to_string(), "auto".to_string()));
    }

    let parts = split_css_component_values(value);
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }

    let mut grow = None;
    let mut shrink = None;
    let mut basis = None;
    for part in parts {
        if grow.is_some() && shrink.is_some() && basis.is_none() && is_unitless_zero(part) {
            basis = Some("0px".to_string());
        } else if let Some(number) = parse_nonnegative_flex_number(part) {
            if grow.is_none() {
                grow = Some(number);
            } else if shrink.is_none() {
                shrink = Some(number);
            } else {
                return None;
            }
        } else if basis.is_none() && parse_computed_flex_basis(part, ROOT_FONT_SIZE_PT).is_some() {
            basis = Some(part.to_string());
        } else {
            return None;
        }
    }

    let grow = grow.unwrap_or_else(|| "1".to_string());
    let shrink = shrink.unwrap_or_else(|| "1".to_string());
    let basis = basis.unwrap_or_else(|| "0%".to_string());
    Some((grow, shrink, basis))
}

/// Returns whether a token is the unitless zero allowed for `flex-basis` in `flex`.
///
/// CSS Flexbox keeps the `flex` shorthand compatible with common authoring by
/// accepting a unitless zero in the flex-basis slot, while nonzero unitless
/// values remain flex factors rather than lengths:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-property>.
fn is_unitless_zero(value: &str) -> bool {
    trim_css_value(value)
        .parse::<f32>()
        .is_ok_and(|number| number == 0.0)
}

fn parse_nonnegative_flex_number(value: &str) -> Option<String> {
    let value = trim_css_value(value);
    let number = value.parse::<f32>().ok()?;
    (number >= 0.0).then(|| value.to_string())
}

/// Expands CSS Box Alignment `place-*` shorthands into modeled longhands.
///
/// CSS Box Alignment defines `place-content`, `place-items`, and `place-self`
/// as paired block/inline-axis alignment shorthands:
/// <https://www.w3.org/TR/css-align-3/#place-content-property>,
/// <https://www.w3.org/TR/css-align-3/#place-items-property>, and
/// <https://www.w3.org/TR/css-align-3/#place-self-property>.
fn expand_alignment_place_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    match name {
        "place-content" => {
            let (align, justify) = split_place_content_shorthand(value)?;
            Some(vec![("align-content", align), ("justify-content", justify)])
        }
        "place-items" => {
            let (align, justify) = split_place_shorthand(
                value,
                parse_align_items_keyword,
                parse_justify_items_keyword,
            )?;
            Some(vec![("align-items", align), ("justify-items", justify)])
        }
        "place-self" => {
            let (align, justify) =
                split_place_shorthand(value, parse_align_self_keyword, parse_justify_self_keyword)?;
            Some(vec![("align-self", align), ("justify-self", justify)])
        }
        _ => None,
    }
}

fn split_place_content_shorthand(value: &str) -> Option<(String, String)> {
    let value = trim_css_value(value);
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if let Some(align) = parse_content_alignment_keyword(value, false, true) {
        if parse_justify_content_keyword(value).is_some() {
            return Some((value.to_string(), value.to_string()));
        }
        if matches!(
            align.keyword,
            ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline
        ) {
            return Some((value.to_string(), "start".to_string()));
        }
    }
    for split in 1..tokens.len() {
        let align = tokens[..split].join(" ");
        let justify = tokens[split..].join(" ");
        if parse_align_content_keyword(&align).is_some()
            && parse_justify_content_keyword(&justify).is_some()
        {
            return Some((align, justify));
        }
    }
    None
}

fn split_place_shorthand<A, J>(
    value: &str,
    parse_align: A,
    parse_justify: J,
) -> Option<(String, String)>
where
    A: Fn(&str) -> Option<()>,
    J: Fn(&str) -> Option<()>,
{
    let value = trim_css_value(value);
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if parse_align(value).is_some() && parse_justify(value).is_some() {
        return Some((value.to_string(), value.to_string()));
    }
    for split in 1..tokens.len() {
        let align = tokens[..split].join(" ");
        let justify = tokens[split..].join(" ");
        if parse_align(&align).is_some() && parse_justify(&justify).is_some() {
            return Some((align, justify));
        }
    }
    None
}

fn parse_alignment_safety_and_keyword(value: &str) -> (AlignmentSafety, String) {
    let mut parts = split_css_component_values(value);
    let safety = match parts.first().map(|part| part.to_ascii_lowercase()) {
        Some(keyword) if keyword == "safe" => {
            parts.remove(0);
            AlignmentSafety::Safe
        }
        Some(keyword) if keyword == "unsafe" => {
            parts.remove(0);
            AlignmentSafety::Unsafe
        }
        _ => AlignmentSafety::Default,
    };
    let keyword = parts
        .into_iter()
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    (safety, keyword)
}

fn content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> ContentAlignment {
    match safety {
        AlignmentSafety::Default => ContentAlignment::new(keyword),
        AlignmentSafety::Unsafe => ContentAlignment::unsafe_position(keyword),
        AlignmentSafety::Safe => ContentAlignment::safe(keyword),
    }
}

fn self_alignment(keyword: SelfAlignmentKeyword, safety: AlignmentSafety) -> SelfAlignment {
    match safety {
        AlignmentSafety::Default => SelfAlignment::new(keyword),
        AlignmentSafety::Unsafe => SelfAlignment::unsafe_position(keyword),
        AlignmentSafety::Safe => SelfAlignment::safe(keyword),
    }
}

fn alignment_safety_allowed_for_content(keyword: ContentAlignmentKeyword) -> bool {
    matches!(
        keyword,
        ContentAlignmentKeyword::Normal
            | ContentAlignmentKeyword::Start
            | ContentAlignmentKeyword::End
            | ContentAlignmentKeyword::FlexStart
            | ContentAlignmentKeyword::FlexEnd
            | ContentAlignmentKeyword::Left
            | ContentAlignmentKeyword::Right
            | ContentAlignmentKeyword::Center
    )
}

fn alignment_safety_allowed_for_self(keyword: SelfAlignmentKeyword) -> bool {
    matches!(
        keyword,
        SelfAlignmentKeyword::Normal
            | SelfAlignmentKeyword::Start
            | SelfAlignmentKeyword::End
            | SelfAlignmentKeyword::SelfStart
            | SelfAlignmentKeyword::SelfEnd
            | SelfAlignmentKeyword::FlexStart
            | SelfAlignmentKeyword::FlexEnd
            | SelfAlignmentKeyword::Left
            | SelfAlignmentKeyword::Right
            | SelfAlignmentKeyword::Center
    )
}

fn parse_content_alignment_keyword(
    value: &str,
    allow_left_right: bool,
    allow_baseline: bool,
) -> Option<ContentAlignment> {
    let (safety, keyword) = parse_alignment_safety_and_keyword(value);
    let keyword = match keyword.as_str() {
        "normal" => ContentAlignmentKeyword::Normal,
        "center" => ContentAlignmentKeyword::Center,
        "space-between" => ContentAlignmentKeyword::SpaceBetween,
        "space-around" => ContentAlignmentKeyword::SpaceAround,
        "space-evenly" => ContentAlignmentKeyword::SpaceEvenly,
        "stretch" => ContentAlignmentKeyword::Stretch,
        "flex-start" => ContentAlignmentKeyword::FlexStart,
        "flex-end" => ContentAlignmentKeyword::FlexEnd,
        "start" => ContentAlignmentKeyword::Start,
        "end" => ContentAlignmentKeyword::End,
        "left" if allow_left_right => ContentAlignmentKeyword::Left,
        "right" if allow_left_right => ContentAlignmentKeyword::Right,
        "baseline" | "first baseline" if allow_baseline => ContentAlignmentKeyword::Baseline,
        "last baseline" if allow_baseline => ContentAlignmentKeyword::LastBaseline,
        _ => return None,
    };
    if safety != AlignmentSafety::Default && !alignment_safety_allowed_for_content(keyword) {
        return None;
    }
    Some(content_alignment(keyword, safety))
}

fn parse_self_alignment_keyword(
    value: &str,
    allow_auto: bool,
    allow_left_right: bool,
) -> Option<SelfAlignment> {
    let (safety, keyword) = parse_alignment_safety_and_keyword(value);
    let keyword = match keyword.as_str() {
        "auto" if allow_auto => SelfAlignmentKeyword::Auto,
        "normal" => SelfAlignmentKeyword::Normal,
        "stretch" => SelfAlignmentKeyword::Stretch,
        "center" => SelfAlignmentKeyword::Center,
        "flex-start" => SelfAlignmentKeyword::FlexStart,
        "flex-end" => SelfAlignmentKeyword::FlexEnd,
        "start" => SelfAlignmentKeyword::Start,
        "end" => SelfAlignmentKeyword::End,
        "self-start" => SelfAlignmentKeyword::SelfStart,
        "self-end" => SelfAlignmentKeyword::SelfEnd,
        "left" if allow_left_right => SelfAlignmentKeyword::Left,
        "right" if allow_left_right => SelfAlignmentKeyword::Right,
        "baseline" | "first baseline" => SelfAlignmentKeyword::Baseline,
        "last baseline" => SelfAlignmentKeyword::LastBaseline,
        _ => return None,
    };
    if safety != AlignmentSafety::Default && !alignment_safety_allowed_for_self(keyword) {
        return None;
    }
    Some(self_alignment(keyword, safety))
}

fn parse_justify_content_keyword(value: &str) -> Option<()> {
    parse_content_alignment_keyword(value, true, false).map(|_| ())
}

fn parse_align_content_keyword(value: &str) -> Option<()> {
    parse_content_alignment_keyword(value, false, true).map(|_| ())
}

fn parse_align_items_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, false, false).map(|_| ())
}

fn parse_align_self_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, true, false).map(|_| ())
}

fn parse_justify_items_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, false, true).map(|_| ())
}

fn parse_justify_self_keyword(value: &str) -> Option<()> {
    parse_self_alignment_keyword(value, true, true).map(|_| ())
}

fn parse_justify_content(value: &str, current: JustifyContent) -> JustifyContent {
    parse_content_alignment_keyword(value, true, false).unwrap_or(current)
}

fn parse_align_content(value: &str, current: AlignContent) -> AlignContent {
    parse_content_alignment_keyword(value, false, true).unwrap_or(current)
}

fn parse_align_items(value: &str, current: AlignItems) -> AlignItems {
    parse_self_alignment_keyword(value, false, false).unwrap_or(current)
}

fn parse_align_self(value: &str, current: AlignSelf) -> AlignSelf {
    parse_self_alignment_keyword(value, true, false).unwrap_or(current)
}

fn parse_justify_items(value: &str, current: JustifyItems) -> JustifyItems {
    parse_self_alignment_keyword(value, false, true).unwrap_or(current)
}

fn parse_justify_self(value: &str, current: JustifySelf) -> JustifySelf {
    parse_self_alignment_keyword(value, true, true).unwrap_or(current)
}

fn expand_columns_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let mut count = "auto".to_string();
    let mut width = "auto".to_string();
    let mut saw_component = false;
    for part in trim_css_value(value).split_whitespace() {
        if part.eq_ignore_ascii_case("auto") {
            saw_component = true;
        } else if part
            .parse::<u16>()
            .ok()
            .filter(|count| *count > 0)
            .is_some()
        {
            count = part.to_string();
            saw_component = true;
        } else if parse_computed_length_percentage(part, ROOT_FONT_SIZE_PT)
            .is_some_and(|length| length.percent == 0.0)
        {
            width = part.to_string();
            saw_component = true;
        } else {
            return None;
        }
    }
    saw_component.then(|| vec![("column-count", count), ("column-width", width)])
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

fn declarations_affect_same_property_in_context(
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

/// Returns the physical longhands affected by a property in a writing context.
///
/// CSS Cascade Level 5 applies defaulting and rollback to the longhands of a
/// shorthand, while CSS Logical Properties resolves flow-relative border
/// properties through `writing-mode` and `direction`:
/// <https://www.w3.org/TR/css-cascade-5/#shorthand> and
/// <https://www.w3.org/TR/css-logical-1/#border-properties>.
fn affected_longhands(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<&'static str>> {
    if let Some(longhand) = logical_size_physical_longhand(name, writing_mode) {
        return Some(vec![longhand]);
    }
    if matches!(name, "margin-block" | "margin-inline") {
        let [start, end] = logical_box_axis_side_names(name)?;
        return Some(vec![
            physical_margin_side_longhand(logical_box_side(start, direction, writing_mode)?),
            physical_margin_side_longhand(logical_box_side(end, direction, writing_mode)?),
        ]);
    }
    if matches!(name, "padding-block" | "padding-inline") {
        let [start, end] = logical_box_axis_side_names(name)?;
        return Some(vec![
            physical_padding_side_longhand(logical_box_side(start, direction, writing_mode)?),
            physical_padding_side_longhand(logical_box_side(end, direction, writing_mode)?),
        ]);
    }
    if matches!(name, "inset-block" | "inset-inline") {
        let [start, end] = logical_box_axis_side_names(name)?;
        return Some(vec![
            physical_inset_side_longhand(logical_box_side(start, direction, writing_mode)?),
            physical_inset_side_longhand(logical_box_side(end, direction, writing_mode)?),
        ]);
    }
    if matches!(
        name,
        "margin-block-start" | "margin-block-end" | "margin-inline-start" | "margin-inline-end"
    ) {
        return Some(vec![physical_margin_side_longhand(logical_box_side(
            name,
            direction,
            writing_mode,
        )?)]);
    }
    if matches!(
        name,
        "padding-block-start" | "padding-block-end" | "padding-inline-start" | "padding-inline-end"
    ) {
        return Some(vec![physical_padding_side_longhand(logical_box_side(
            name,
            direction,
            writing_mode,
        )?)]);
    }
    if matches!(
        name,
        "inset-block-start" | "inset-block-end" | "inset-inline-start" | "inset-inline-end"
    ) {
        return Some(vec![physical_inset_side_longhand(logical_box_side(
            name,
            direction,
            writing_mode,
        )?)]);
    }
    if name == "inset" {
        return Some(vec!["top", "right", "bottom", "left"]);
    }
    if matches!(name, "border-block" | "border-inline") {
        let logical_sides = logical_axis_side_names(name)?;
        let mut longhands = Vec::with_capacity(6);
        for logical_side in logical_sides {
            longhands.extend(border_side_component_longhands(logical_border_side(
                logical_side,
                direction,
                writing_mode,
            )?));
        }
        return Some(longhands);
    }
    if matches!(
        name,
        "border-block-start" | "border-block-end" | "border-inline-start" | "border-inline-end"
    ) {
        return Some(
            border_side_component_longhands(logical_border_side(name, direction, writing_mode)?)
                .to_vec(),
        );
    }
    if matches!(name, "border-block-width" | "border-inline-width") {
        return Some(
            logical_axis_sides(name, direction, writing_mode)?
                .into_iter()
                .map(|side| physical_border_side_component_longhand(side, "width"))
                .collect(),
        );
    }
    if matches!(name, "border-block-style" | "border-inline-style") {
        return Some(
            logical_axis_sides(name, direction, writing_mode)?
                .into_iter()
                .map(|side| physical_border_side_component_longhand(side, "style"))
                .collect(),
        );
    }
    if matches!(name, "border-block-color" | "border-inline-color") {
        return Some(
            logical_axis_sides(name, direction, writing_mode)?
                .into_iter()
                .map(|side| physical_border_side_component_longhand(side, "color"))
                .collect(),
        );
    }
    if matches!(
        name,
        "border-block-start-width"
            | "border-block-end-width"
            | "border-inline-start-width"
            | "border-inline-end-width"
    ) {
        return Some(vec![physical_border_side_component_longhand(
            logical_border_side(name, direction, writing_mode)?,
            "width",
        )]);
    }
    if matches!(
        name,
        "border-block-start-style"
            | "border-block-end-style"
            | "border-inline-start-style"
            | "border-inline-end-style"
    ) {
        return Some(vec![physical_border_side_component_longhand(
            logical_border_side(name, direction, writing_mode)?,
            "style",
        )]);
    }
    if matches!(
        name,
        "border-block-start-color"
            | "border-block-end-color"
            | "border-inline-start-color"
            | "border-inline-end-color"
    ) {
        return Some(vec![physical_border_side_component_longhand(
            logical_border_side(name, direction, writing_mode)?,
            "color",
        )]);
    }
    if matches!(
        name,
        "border-start-start-radius"
            | "border-start-end-radius"
            | "border-end-start-radius"
            | "border-end-end-radius"
    ) {
        return Some(vec![logical_corner_radius_longhand(
            name,
            direction,
            writing_mode,
        )?]);
    }

    let longhands: &[&str] = match name {
        "margin" => &["margin-top", "margin-right", "margin-bottom", "margin-left"],
        "margin-top" => &["margin-top"],
        "margin-right" => &["margin-right"],
        "margin-bottom" => &["margin-bottom"],
        "margin-left" => &["margin-left"],
        "padding" => &[
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ],
        "padding-top" => &["padding-top"],
        "padding-right" => &["padding-right"],
        "padding-bottom" => &["padding-bottom"],
        "padding-left" => &["padding-left"],
        "border" => &[
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
        ],
        "border-top" => &["border-top-width", "border-top-style", "border-top-color"],
        "border-right" => &[
            "border-right-width",
            "border-right-style",
            "border-right-color",
        ],
        "border-bottom" => &[
            "border-bottom-width",
            "border-bottom-style",
            "border-bottom-color",
        ],
        "border-left" => &[
            "border-left-width",
            "border-left-style",
            "border-left-color",
        ],
        "border-width" => &[
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
        ],
        "border-top-width" => &["border-top-width"],
        "border-right-width" => &["border-right-width"],
        "border-bottom-width" => &["border-bottom-width"],
        "border-left-width" => &["border-left-width"],
        "border-style" => &[
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
        ],
        "border-top-style" => &["border-top-style"],
        "border-right-style" => &["border-right-style"],
        "border-bottom-style" => &["border-bottom-style"],
        "border-left-style" => &["border-left-style"],
        "border-color" => &[
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
        ],
        "border-top-color" => &["border-top-color"],
        "border-right-color" => &["border-right-color"],
        "border-bottom-color" => &["border-bottom-color"],
        "border-left-color" => &["border-left-color"],
        "border-radius" => &[
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ],
        "corner" => &[
            "border-top-left-radius",
            "corner-top-left-shape",
            "border-top-right-radius",
            "corner-top-right-shape",
            "border-bottom-right-radius",
            "corner-bottom-right-shape",
            "border-bottom-left-radius",
            "corner-bottom-left-shape",
        ],
        "corner-shape" => &[
            "corner-top-left-shape",
            "corner-top-right-shape",
            "corner-bottom-right-shape",
            "corner-bottom-left-shape",
        ],
        "border-top-left-radius" => &["border-top-left-radius"],
        "border-top-right-radius" => &["border-top-right-radius"],
        "border-bottom-right-radius" => &["border-bottom-right-radius"],
        "border-bottom-left-radius" => &["border-bottom-left-radius"],
        "corner-top-left-shape" => &["corner-top-left-shape"],
        "corner-top-right-shape" => &["corner-top-right-shape"],
        "corner-bottom-right-shape" => &["corner-bottom-right-shape"],
        "corner-bottom-left-shape" => &["corner-bottom-left-shape"],
        "border-image" => &[
            "border-image-source",
            "border-image-slice",
            "border-image-width",
            "border-image-outset",
            "border-image-repeat",
        ],
        "border-image-source" => &["border-image-source"],
        "border-image-slice" => &["border-image-slice"],
        "border-image-width" => &["border-image-width"],
        "border-image-outset" => &["border-image-outset"],
        "border-image-repeat" => &["border-image-repeat"],
        "background" => &[
            "background-color",
            "background-image",
            "background-size",
            "background-position",
            "background-repeat",
            "background-origin",
            "background-clip",
        ],
        "background-color" => &["background-color"],
        "background-image" => &["background-image"],
        "background-size" => &["background-size"],
        "background-position" => &["background-position"],
        "background-repeat" => &["background-repeat"],
        "background-origin" => &["background-origin"],
        "background-clip" => &["background-clip"],
        "text-align" => &["text-align-all", "text-align-last"],
        "text-align-all" => &["text-align-all"],
        "text-align-last" => &["text-align-last"],
        "flex" => &["flex-grow", "flex-shrink", "flex-basis"],
        "flex-grow" => &["flex-grow"],
        "flex-shrink" => &["flex-shrink"],
        "flex-basis" => &["flex-basis"],
        "order" => &["order"],
        "flex-flow" => &["flex-direction", "flex-wrap"],
        "flex-direction" => &["flex-direction"],
        "flex-wrap" => &["flex-wrap"],
        "place-content" => &["align-content", "justify-content"],
        "place-items" => &["align-items", "justify-items"],
        "place-self" => &["align-self", "justify-self"],
        "justify-content" => &["justify-content"],
        "justify-items" => &["justify-items"],
        "justify-self" => &["justify-self"],
        "align-content" => &["align-content"],
        "align-items" => &["align-items"],
        "align-self" => &["align-self"],
        "gap" => &["row-gap", "column-gap"],
        "row-gap" => &["row-gap"],
        "column-gap" => &["column-gap"],
        "columns" => &["column-count", "column-width"],
        "column-count" => &["column-count"],
        "column-width" => &["column-width"],
        "list-style" => &["list-style-type", "list-style-position", "list-style-image"],
        "list-style-type" => &["list-style-type"],
        "list-style-position" => &["list-style-position"],
        "list-style-image" => &["list-style-image"],
        "text-decoration" => &[
            "text-decoration-line",
            "text-decoration-style",
            "text-decoration-color",
            "text-decoration-thickness",
        ],
        "text-decoration-line" => &["text-decoration-line"],
        "text-decoration-style" => &["text-decoration-style"],
        "text-decoration-color" => &["text-decoration-color"],
        "text-decoration-thickness" => &["text-decoration-thickness"],
        "text-decoration-inset" => &["text-decoration-inset"],
        "text-decoration-skip" => &[
            "text-decoration-skip-ink",
            "text-decoration-skip-self",
            "text-decoration-skip-box",
            "text-decoration-skip-spaces",
        ],
        "text-decoration-skip-ink" => &["text-decoration-skip-ink"],
        "text-decoration-skip-self" => &["text-decoration-skip-self"],
        "text-decoration-skip-box" => &["text-decoration-skip-box"],
        "text-decoration-skip-spaces" => &["text-decoration-skip-spaces"],
        "text-underline-offset" => &["text-underline-offset"],
        "text-underline-position" => &["text-underline-position"],
        "text-emphasis" => &["text-emphasis-style", "text-emphasis-color"],
        "text-emphasis-style" => &["text-emphasis-style"],
        "text-emphasis-color" => &["text-emphasis-color"],
        "text-emphasis-position" => &["text-emphasis-position"],
        "text-emphasis-skip" => &["text-emphasis-skip"],
        "text-shadow" => &["text-shadow"],
        "box-shadow" => &["box-shadow"],
        "overflow" => &["overflow-x", "overflow-y"],
        "overflow-x" => &["overflow-x"],
        "overflow-y" => &["overflow-y"],
        "word-wrap" | "overflow-wrap" => &["overflow-wrap"],
        "font-variant" => &[
            "font-variant-ligatures",
            "font-variant-position",
            "font-variant-caps",
            "font-variant-numeric",
            "font-variant-alternates",
            "font-variant-east-asian",
            "font-variant-emoji",
        ],
        "font-variant-ligatures" => &["font-variant-ligatures"],
        "font-variant-position" => &["font-variant-position"],
        "font-variant-caps" => &["font-variant-caps"],
        "font-variant-numeric" => &["font-variant-numeric"],
        "font-variant-alternates" => &["font-variant-alternates"],
        "font-variant-east-asian" => &["font-variant-east-asian"],
        "font-variant-emoji" => &["font-variant-emoji"],
        "page-break-before" | "break-before" => &["break-before"],
        "page-break-after" | "break-after" => &["break-after"],
        "page-break-inside" | "break-inside" => &["break-inside"],
        _ => return None,
    };
    Some(longhands.to_vec())
}

fn logical_axis_side_names(name: &str) -> Option<[&'static str; 2]> {
    match name {
        "border-block" | "border-block-width" | "border-block-style" | "border-block-color" => {
            Some(["border-block-start", "border-block-end"])
        }
        "border-inline" | "border-inline-width" | "border-inline-style" | "border-inline-color" => {
            Some(["border-inline-start", "border-inline-end"])
        }
        _ => None,
    }
}

/// Returns the logical start/end side names for a box edge axis shorthand.
///
/// CSS Logical Properties defines `*-block` and `*-inline` margin/padding
/// shorthands as setting the start and end sides of that logical axis:
/// <https://www.w3.org/TR/css-logical-1/#box>.
fn logical_box_axis_side_names(name: &str) -> Option<[&'static str; 2]> {
    match name {
        "margin-block" | "padding-block" | "inset-block" => Some(["block-start", "block-end"]),
        "margin-inline" | "padding-inline" | "inset-inline" => Some(["inline-start", "inline-end"]),
        _ => None,
    }
}

fn logical_axis_sides(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<[BorderSide; 2]> {
    let [start, end] = logical_axis_side_names(name)?;
    Some([
        logical_border_side(start, direction, writing_mode)?,
        logical_border_side(end, direction, writing_mode)?,
    ])
}

fn border_side_component_longhands(side: BorderSide) -> [&'static str; 3] {
    [
        physical_border_side_component_longhand(side, "width"),
        physical_border_side_component_longhand(side, "style"),
        physical_border_side_component_longhand(side, "color"),
    ]
}

fn physical_border_side_component_longhand(side: BorderSide, component: &str) -> &'static str {
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

fn declaration_is_revert_layer(value: &str) -> bool {
    trim_css_value(value).eq_ignore_ascii_case("revert-layer")
}

fn declaration_is_revert(value: &str) -> bool {
    trim_css_value(value).eq_ignore_ascii_case("revert")
}

/// Returns whether a prior declaration is erased by a later `revert`.
///
/// CSS Cascade Level 5 rolls author-origin `revert` back to user level,
/// user-origin `revert` back to UA level, and treats UA-origin `revert` like
/// `unset`:
/// <https://www.w3.org/TR/css-cascade-5/#revert>.
fn same_or_stronger_reverted_origin(
    prior: &CascadedDeclaration<'_>,
    rollback: &CascadedDeclaration<'_>,
) -> bool {
    match rollback.origin {
        StylesheetOrigin::Author => prior.origin == StylesheetOrigin::Author,
        StylesheetOrigin::User => {
            matches!(
                prior.origin,
                StylesheetOrigin::User | StylesheetOrigin::Author
            )
        }
        StylesheetOrigin::UserAgent => prior.origin == StylesheetOrigin::UserAgent,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CssWideDefaultKeyword {
    Initial,
    Inherit,
    Unset,
}

impl CssWideDefaultKeyword {
    fn parse(value: &str) -> Option<Self> {
        match trim_css_value(value).to_ascii_lowercase().as_str() {
            "initial" => Some(Self::Initial),
            "inherit" => Some(Self::Inherit),
            "unset" => Some(Self::Unset),
            _ => None,
        }
    }
}

/// Applies CSS-wide defaulting keywords to modeled properties.
///
/// CSS Cascade Level 5 defines `initial`, `inherit`, and `unset` as defaulting
/// behaviors accepted by every property. Shorthands, including `all`, apply the
/// keyword to their longhands:
/// <https://www.w3.org/TR/css-cascade-5/#defaulting-keywords> and
/// <https://www.w3.org/TR/css-cascade-5/#all-shorthand>.
fn apply_css_wide_default_keyword(
    style: &mut ComputedStyle,
    name: &str,
    keyword: CssWideDefaultKeyword,
    defaulted_style: &ComputedStyle,
) {
    if name.eq_ignore_ascii_case("all") {
        for longhand in ALL_MODELED_LONGHANDS {
            apply_css_wide_default_longhand(style, longhand, keyword, defaulted_style);
        }
        return;
    }
    if let Some(longhands) = affected_longhands(name, style.direction, style.writing_mode) {
        for longhand in longhands {
            apply_css_wide_default_longhand(style, longhand, keyword, defaulted_style);
        }
    } else {
        apply_css_wide_default_longhand(style, name, keyword, defaulted_style);
    }
}

fn apply_css_wide_default_longhand(
    style: &mut ComputedStyle,
    name: &str,
    keyword: CssWideDefaultKeyword,
    defaulted_style: &ComputedStyle,
) {
    let initial;
    let source = match keyword {
        CssWideDefaultKeyword::Initial => {
            initial = ComputedStyle::initial();
            &initial
        }
        CssWideDefaultKeyword::Inherit => defaulted_style,
        CssWideDefaultKeyword::Unset if property_is_inherited(name) => defaulted_style,
        CssWideDefaultKeyword::Unset => {
            initial = ComputedStyle::initial();
            &initial
        }
    };
    copy_modeled_property(style, source, name);
    if name.eq_ignore_ascii_case("page") {
        style.page_name_specified = true;
    }
}

fn same_cascade_layer(left: &CascadedDeclaration<'_>, right: &CascadedDeclaration<'_>) -> bool {
    left.origin == right.origin
        && left.important == right.important
        && left.layer_order == right.layer_order
}

/// Parses the CSS Box Model Level 4 `margin-trim` property.
///
/// The value is a set of trim-side keywords; `block` and `inline` expand to
/// both sides in that axis:
/// <https://drafts.csswg.org/css-box-4/#margin-trim>.
fn parse_margin_trim(value: &str) -> Option<MarginTrim> {
    let mut trim = MarginTrim::NONE;
    let mut saw_token = false;
    let mut saw_none = false;
    for token in trim_css_value(value).split_whitespace() {
        saw_token = true;
        match token.to_ascii_lowercase().as_str() {
            "none" if !saw_none && trim == MarginTrim::NONE => saw_none = true,
            "none" => return None,
            "block" => {
                if saw_none {
                    return None;
                }
                trim.block_start = true;
                trim.block_end = true;
            }
            "block-start" => {
                if saw_none {
                    return None;
                }
                trim.block_start = true;
            }
            "block-end" => {
                if saw_none {
                    return None;
                }
                trim.block_end = true;
            }
            "inline" => {
                if saw_none {
                    return None;
                }
                trim.inline_start = true;
                trim.inline_end = true;
            }
            "inline-start" => {
                if saw_none {
                    return None;
                }
                trim.inline_start = true;
            }
            "inline-end" => {
                if saw_none {
                    return None;
                }
                trim.inline_end = true;
            }
            _ => return None,
        }
    }
    saw_token.then_some(trim)
}

pub(crate) fn apply_declarations(style: &mut ComputedStyle, declarations: &Declarations) {
    apply_declarations_with_origin(style, declarations, StylesheetOrigin::Author);
}

pub(crate) fn apply_declarations_with_origin(
    style: &mut ComputedStyle,
    declarations: &Declarations,
    origin: StylesheetOrigin,
) {
    let mut declarations = cascaded_declarations_from(declarations, origin);
    sort_cascaded_declarations(&mut declarations);
    apply_cascaded_declarations(style, &declarations);
}

pub(crate) fn apply_cascaded_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
) {
    let defaulted_style = style.clone();
    apply_cascaded_declarations_with_inheritance_source(style, declarations, &defaulted_style);
}

pub(crate) fn apply_cascaded_declarations_with_inheritance_source(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
) {
    let parent_ch_advance = fallback_ch_advance_for_style(inheritance_source);
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
        style,
        declarations,
        inheritance_source,
        parent_ch_advance,
    );
}

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
        match name {
            "direction" => {
                if let Some(direction) = parse_direction(value) {
                    style.direction = direction;
                }
            }
            "unicode-bidi" => {
                style.unicode_bidi = match value.trim().to_ascii_lowercase().as_str() {
                    "normal" => UnicodeBidi::Normal,
                    "embed" => UnicodeBidi::Embed,
                    "isolate" => UnicodeBidi::Isolate,
                    "bidi-override" => UnicodeBidi::BidiOverride,
                    "isolate-override" => UnicodeBidi::IsolateOverride,
                    "plaintext" => UnicodeBidi::Plaintext,
                    _ => style.unicode_bidi,
                };
            }
            "writing-mode" => {
                if let Some(writing_mode) = parse_writing_mode(value) {
                    style.writing_mode = writing_mode;
                }
            }
            "text-orientation" => {
                if let Some(text_orientation) = parse_text_orientation(value) {
                    style.text_orientation = text_orientation;
                }
            }
            "display" => {
                style.display = parse_display(value, style.display);
            }
            "flex-direction" => {
                style.flex_direction = match value.to_ascii_lowercase().as_str() {
                    "column" => FlexDirection::Column,
                    "column-reverse" => FlexDirection::ColumnReverse,
                    "row" => FlexDirection::Row,
                    "row-reverse" => FlexDirection::RowReverse,
                    _ => style.flex_direction,
                };
            }
            "flex-flow" => {
                if let Some((direction, wrap)) = parse_flex_flow(value) {
                    style.flex_direction = direction;
                    style.flex_wrap = wrap;
                }
            }
            "justify-content" => {
                style.justify_content = parse_justify_content(value, style.justify_content);
            }
            "justify-items" => {
                style.justify_items = parse_justify_items(value, style.justify_items);
            }
            "justify-self" => {
                style.justify_self = parse_justify_self(value, style.justify_self);
            }
            "align-content" => {
                style.align_content = parse_align_content(value, style.align_content);
            }
            "align-items" => {
                style.align_items = parse_align_items(value, style.align_items);
            }
            "align-self" => {
                style.align_self = parse_align_self(value, style.align_self);
            }
            "flex-wrap" => {
                style.flex_wrap = match value.to_ascii_lowercase().as_str() {
                    "wrap" => FlexWrap::Wrap,
                    "wrap-reverse" => FlexWrap::WrapReverse,
                    "nowrap" => FlexWrap::NoWrap,
                    _ => style.flex_wrap,
                };
            }
            "flex-grow" => {
                if let Ok(value) = value.parse::<f32>()
                    && value >= 0.0
                {
                    style.flex_grow = value;
                }
            }
            "flex-shrink" => {
                if let Ok(value) = value.parse::<f32>()
                    && value >= 0.0
                {
                    style.flex_shrink = value;
                }
            }
            "flex-basis" => {
                if let Some(basis) = parse_computed_flex_basis(value, style.font_size) {
                    style.flex_basis = basis;
                }
            }
            "order" => {
                if let Ok(value) = value.parse::<i32>() {
                    style.order = value;
                }
            }
            "flex" => {
                if let Some((grow, shrink, basis)) = parse_flex_shorthand_components(value) {
                    if let Ok(grow) = grow.parse::<f32>() {
                        style.flex_grow = grow;
                    }
                    if let Ok(shrink) = shrink.parse::<f32>() {
                        style.flex_shrink = shrink;
                    }
                    if let Some(basis) = parse_computed_flex_basis(&basis, style.font_size) {
                        style.flex_basis = basis;
                    }
                }
            }
            "gap" => {
                let parts = split_css_component_values(value);
                if let Some((row_gap, column_gap)) =
                    parse_gap_shorthand_components(&parts, style.font_size)
                {
                    style.row_gap = row_gap;
                    style.column_gap = column_gap;
                }
            }
            "row-gap" => {
                if let Some(gap) = parse_gap(value, style.font_size) {
                    style.row_gap = gap;
                }
            }
            "column-count" => {
                style.column_count = parse_column_count(value);
            }
            "column-width" => {
                if let Some(width) = parse_column_width(value, style.font_size) {
                    style.column_width = width;
                }
            }
            "column-gap" => {
                if let Some(gap) = parse_column_gap(value, style.font_size) {
                    style.column_gap = gap;
                }
            }
            "grid-template-rows" => {
                if let Some(tracks) = parse_grid_track_list(value, style.font_size) {
                    style.grid_template_rows = tracks;
                }
            }
            "grid-template-columns" => {
                if let Some(tracks) = parse_grid_track_list(value, style.font_size) {
                    style.grid_template_columns = tracks;
                }
            }
            "grid-template-areas" => {
                if let Some(areas) = parse_grid_template_areas(value) {
                    style.grid_template_areas = areas;
                }
            }
            "grid-auto-rows" => {
                if let Some(tracks) = parse_grid_auto_track_list(value, style.font_size) {
                    style.grid_auto_rows = tracks;
                }
            }
            "grid-auto-columns" => {
                if let Some(tracks) = parse_grid_auto_track_list(value, style.font_size) {
                    style.grid_auto_columns = tracks;
                }
            }
            "grid-auto-flow" => {
                if let Some(flow) = parse_grid_auto_flow(value) {
                    style.grid_auto_flow = flow;
                }
            }
            "grid-row-start" => {
                if let Some(placement) = parse_grid_placement(value) {
                    style.grid_row_start = placement;
                }
            }
            "grid-row-end" => {
                if let Some(placement) = parse_grid_placement(value) {
                    style.grid_row_end = placement;
                }
            }
            "grid-column-start" => {
                if let Some(placement) = parse_grid_placement(value) {
                    style.grid_column_start = placement;
                }
            }
            "grid-column-end" => {
                if let Some(placement) = parse_grid_placement(value) {
                    style.grid_column_end = placement;
                }
            }
            "columns" => apply_columns(value, style),
            "margin-trim" => {
                if let Some(margin_trim) = parse_margin_trim(value) {
                    style.margin_trim = margin_trim;
                }
            }
            "margin-block" | "margin-inline" => {
                apply_logical_margin_axis(value, style, name, declaration.origin)
            }
            "margin-block-start"
            | "margin-block-end"
            | "margin-inline-start"
            | "margin-inline-end" => {
                apply_logical_margin_side(value, style, name, declaration.origin)
            }
            "margin" => {
                if let Some(typed) = parse_margin_edge_values(value, style.font_size) {
                    style.box_values.margin = typed;
                    style.margin = legacy_margin_edges(typed);
                    style.ua_margin_em = if declaration.origin == StylesheetOrigin::UserAgent {
                        parse_margin_em_edges(value)
                    } else {
                        OptionalEdges::NONE
                    };
                }
            }
            "margin-top" => set_margin_side(value, style.font_size, |typed| {
                style.box_values.margin.top = typed;
                style.margin.top = typed.length_if_no_percent().unwrap_or(0.0);
                style.ua_margin_em.top = if declaration.origin == StylesheetOrigin::UserAgent {
                    parse_em_length_factor(value)
                } else {
                    None
                };
            }),
            "margin-right" => set_margin_side(value, style.font_size, |typed| {
                style.box_values.margin.right = typed;
                style.margin.right = typed.length_if_no_percent().unwrap_or(0.0);
                style.ua_margin_em.right = if declaration.origin == StylesheetOrigin::UserAgent {
                    parse_em_length_factor(value)
                } else {
                    None
                };
            }),
            "margin-bottom" => set_margin_side(value, style.font_size, |typed| {
                style.box_values.margin.bottom = typed;
                style.margin.bottom = typed.length_if_no_percent().unwrap_or(0.0);
                style.ua_margin_em.bottom = if declaration.origin == StylesheetOrigin::UserAgent {
                    parse_em_length_factor(value)
                } else {
                    None
                };
            }),
            "margin-left" => set_margin_side(value, style.font_size, |typed| {
                style.box_values.margin.left = typed;
                style.margin.left = typed.length_if_no_percent().unwrap_or(0.0);
                style.ua_margin_em.left = if declaration.origin == StylesheetOrigin::UserAgent {
                    parse_em_length_factor(value)
                } else {
                    None
                };
            }),
            "padding-block" | "padding-inline" => apply_logical_padding_axis(value, style, name),
            "padding-block-start"
            | "padding-block-end"
            | "padding-inline-start"
            | "padding-inline-end" => apply_logical_padding_side(value, style, name),
            "padding" => {
                if let Some(typed) = parse_edge_values(value, style.font_size) {
                    style.box_values.padding = typed;
                    if let Some(edges) = legacy_edge_lengths(typed) {
                        style.padding = edges;
                    }
                }
            }
            "padding-top" => set_computed_length_percentage(value, style.font_size, |typed| {
                style.box_values.padding.top = typed;
                if let Some(length) = typed.length_if_no_percent() {
                    style.padding.top = length;
                }
            }),
            "padding-right" => set_computed_length_percentage(value, style.font_size, |typed| {
                style.box_values.padding.right = typed;
                if let Some(length) = typed.length_if_no_percent() {
                    style.padding.right = length;
                }
            }),
            "padding-bottom" => set_computed_length_percentage(value, style.font_size, |typed| {
                style.box_values.padding.bottom = typed;
                if let Some(length) = typed.length_if_no_percent() {
                    style.padding.bottom = length;
                }
            }),
            "padding-left" => set_computed_length_percentage(value, style.font_size, |typed| {
                style.box_values.padding.left = typed;
                if let Some(length) = typed.length_if_no_percent() {
                    style.padding.left = length;
                }
            }),
            "border" => apply_border(value, style, None),
            "border-top" => apply_border(value, style, Some(BorderSide::Top)),
            "border-right" => apply_border(value, style, Some(BorderSide::Right)),
            "border-bottom" => apply_border(value, style, Some(BorderSide::Bottom)),
            "border-left" => apply_border(value, style, Some(BorderSide::Left)),
            "border-block" | "border-inline" => apply_logical_border_axis(value, style, name),
            "border-block-start"
            | "border-block-end"
            | "border-inline-start"
            | "border-inline-end" => apply_logical_border(value, style, name),
            "border-width" => {
                if let Some(edges) = parse_border_width_edges(value, style.font_size) {
                    style.border_width_values = edges;
                    style.border_widths = Edges {
                        top: edges.top.length_if_no_percent().unwrap_or(edges.top.length),
                        right: edges
                            .right
                            .length_if_no_percent()
                            .unwrap_or(edges.right.length),
                        bottom: edges
                            .bottom
                            .length_if_no_percent()
                            .unwrap_or(edges.bottom.length),
                        left: edges
                            .left
                            .length_if_no_percent()
                            .unwrap_or(edges.left.length),
                    };
                    style.border_width = max_edge(style.border_widths);
                }
            }
            "border-block-width" => {
                if let Some([start, end]) = parse_logical_border_widths(value, style.font_size)
                    && let Some([start_side, end_side]) =
                        logical_axis_sides(name, style.direction, style.writing_mode)
                {
                    set_border_side_width(style, start_side, start);
                    set_border_side_width(style, end_side, end);
                }
            }
            "border-inline-width" => {
                if let Some([start, end]) = parse_logical_border_widths(value, style.font_size)
                    && let Some([start_side, end_side]) =
                        logical_axis_sides(name, style.direction, style.writing_mode)
                {
                    set_border_side_width(style, start_side, start);
                    set_border_side_width(style, end_side, end);
                }
            }
            "border-top-width" => {
                if let Some(length) = parse_computed_border_width(value, style.font_size) {
                    set_border_side_width(style, BorderSide::Top, length);
                }
            }
            "border-right-width" => {
                if let Some(length) = parse_computed_border_width(value, style.font_size) {
                    set_border_side_width(style, BorderSide::Right, length);
                }
            }
            "border-bottom-width" => {
                if let Some(length) = parse_computed_border_width(value, style.font_size) {
                    set_border_side_width(style, BorderSide::Bottom, length);
                }
            }
            "border-left-width" => {
                if let Some(length) = parse_computed_border_width(value, style.font_size) {
                    set_border_side_width(style, BorderSide::Left, length);
                }
            }
            "border-block-start-width"
            | "border-block-end-width"
            | "border-inline-start-width"
            | "border-inline-end-width" => {
                if let Some(side) = logical_border_side(name, style.direction, style.writing_mode)
                    && let Some(length) = parse_computed_border_width(value, style.font_size)
                {
                    set_border_side_width(style, side, length);
                }
            }
            "border-color" => {
                if let Some(colors) = parse_border_colors(value, style.color) {
                    style.border_colors = colors;
                    style.border_color = colors.top;
                } else if let Some(color) = parse_border_color(value, style.color) {
                    style.border_color = color;
                    style.border_colors = border_colors_all(color);
                }
            }
            "border-top-color" => {
                if let Some(color) = parse_border_color(value, style.color) {
                    style.border_colors.top = color;
                    style.border_color = color;
                }
            }
            "border-right-color" => {
                if let Some(color) = parse_border_color(value, style.color) {
                    style.border_colors.right = color;
                }
            }
            "border-bottom-color" => {
                if let Some(color) = parse_border_color(value, style.color) {
                    style.border_colors.bottom = color;
                }
            }
            "border-left-color" => {
                if let Some(color) = parse_border_color(value, style.color) {
                    style.border_colors.left = color;
                }
            }
            "border-block-color" => {
                if let Some([start, end]) = parse_logical_border_colors(value, style.color)
                    && let Some([start_side, end_side]) =
                        logical_axis_sides(name, style.direction, style.writing_mode)
                {
                    set_border_side_color(style, start_side, start);
                    set_border_side_color(style, end_side, end);
                }
            }
            "border-inline-color" => {
                if let Some([start, end]) = parse_logical_border_colors(value, style.color)
                    && let Some([start_side, end_side]) =
                        logical_axis_sides(name, style.direction, style.writing_mode)
                {
                    set_border_side_color(style, start_side, start);
                    set_border_side_color(style, end_side, end);
                }
            }
            "border-block-start-color"
            | "border-block-end-color"
            | "border-inline-start-color"
            | "border-inline-end-color" => {
                if let Some(side) = logical_border_side(name, style.direction, style.writing_mode)
                    && let Some(color) = parse_border_color(value, style.color)
                {
                    set_border_side_color(style, side, color);
                }
            }
            "border-style" => {
                if let Some(styles) = parse_border_styles(value) {
                    style.border_styles = styles;
                }
            }
            "outline-width" => {
                if let Some(length) = parse_computed_border_width(value, style.font_size) {
                    style.outline_width_value = length;
                    style.outline_width = length.length_if_no_percent().unwrap_or(length.length);
                }
            }
            "outline-style" => {
                if let Some(outline_style) = parse_border_style(value) {
                    style.outline_style = outline_style;
                }
            }
            "outline-color" => {
                if let Some(color) = parse_border_color(value, style.color) {
                    style.outline_color = color;
                }
            }
            "outline-offset" => {
                if let Some(length) = parse_computed_length_percentage(value, style.font_size)
                    && length.percent == 0.0
                {
                    style.outline_offset = length;
                }
            }
            "border-block-style" => {
                if let Some([start, end]) = parse_logical_border_styles(value)
                    && let Some([start_side, end_side]) =
                        logical_axis_sides(name, style.direction, style.writing_mode)
                {
                    set_border_side_style_value(style, start_side, start);
                    set_border_side_style_value(style, end_side, end);
                }
            }
            "border-inline-style" => {
                if let Some([start, end]) = parse_logical_border_styles(value)
                    && let Some([start_side, end_side]) =
                        logical_axis_sides(name, style.direction, style.writing_mode)
                {
                    set_border_side_style_value(style, start_side, start);
                    set_border_side_style_value(style, end_side, end);
                }
            }
            "border-radius" => {
                if let Some(radius) = parse_border_radius(value, style.font_size) {
                    style.border_radius = radius;
                }
            }
            "corner" => {
                if let Some((radius, shapes)) = parse_corner_shorthand(value, style.font_size) {
                    style.border_radius = radius;
                    style.corner_shapes = shapes;
                }
            }
            "corner-shape" => {
                if let Some(shapes) = parse_corner_shapes(value) {
                    style.corner_shapes = shapes;
                }
            }
            "border-top-left-radius" => {
                if let Some(radius) = parse_corner_radius(value, style.font_size) {
                    style.border_radius.top_left = radius;
                }
            }
            "border-top-right-radius" => {
                if let Some(radius) = parse_corner_radius(value, style.font_size) {
                    style.border_radius.top_right = radius;
                }
            }
            "border-bottom-right-radius" => {
                if let Some(radius) = parse_corner_radius(value, style.font_size) {
                    style.border_radius.bottom_right = radius;
                }
            }
            "border-bottom-left-radius" => {
                if let Some(radius) = parse_corner_radius(value, style.font_size) {
                    style.border_radius.bottom_left = radius;
                }
            }
            "corner-top-left-shape" => {
                if let Some(shape) = parse_corner_shape(value) {
                    style.corner_shapes.top_left = shape;
                }
            }
            "corner-top-right-shape" => {
                if let Some(shape) = parse_corner_shape(value) {
                    style.corner_shapes.top_right = shape;
                }
            }
            "corner-bottom-right-shape" => {
                if let Some(shape) = parse_corner_shape(value) {
                    style.corner_shapes.bottom_right = shape;
                }
            }
            "corner-bottom-left-shape" => {
                if let Some(shape) = parse_corner_shape(value) {
                    style.corner_shapes.bottom_left = shape;
                }
            }
            "border-start-start-radius"
            | "border-start-end-radius"
            | "border-end-start-radius"
            | "border-end-end-radius" => {
                if let Some(physical) =
                    logical_corner_radius_longhand(name, style.direction, style.writing_mode)
                    && let Some(radius) = parse_corner_radius(value, style.font_size)
                {
                    match physical {
                        "border-top-left-radius" => style.border_radius.top_left = radius,
                        "border-top-right-radius" => style.border_radius.top_right = radius,
                        "border-bottom-right-radius" => style.border_radius.bottom_right = radius,
                        "border-bottom-left-radius" => style.border_radius.bottom_left = radius,
                        _ => {}
                    }
                }
            }
            "border-top-style" => set_border_side_style(style, BorderSide::Top, value),
            "border-right-style" => set_border_side_style(style, BorderSide::Right, value),
            "border-bottom-style" => set_border_side_style(style, BorderSide::Bottom, value),
            "border-left-style" => set_border_side_style(style, BorderSide::Left, value),
            "border-block-start-style"
            | "border-block-end-style"
            | "border-inline-start-style"
            | "border-inline-end-style" => {
                if let Some(side) = logical_border_side(name, style.direction, style.writing_mode) {
                    set_border_side_style(style, side, value);
                }
            }
            "border-collapse" => {
                style.border_collapse = match value.to_ascii_lowercase().as_str() {
                    "collapse" => BorderCollapse::Collapse,
                    "separate" => BorderCollapse::Separate,
                    _ => style.border_collapse,
                };
            }
            "caption-side" => {
                style.caption_side = match value.to_ascii_lowercase().as_str() {
                    "top" => CaptionSide::Top,
                    "bottom" => CaptionSide::Bottom,
                    _ => style.caption_side,
                };
            }
            "table-layout" => {
                style.table_layout = match value.to_ascii_lowercase().as_str() {
                    "auto" => TableLayout::Auto,
                    "fixed" => TableLayout::Fixed,
                    _ => style.table_layout,
                };
            }
            "empty-cells" => {
                style.empty_cells = match value.to_ascii_lowercase().as_str() {
                    "show" => EmptyCells::Show,
                    "hide" => EmptyCells::Hide,
                    _ => style.empty_cells,
                };
            }
            "border-spacing" => {
                if let Some(spacing) = parse_border_spacing(value, style.font_size) {
                    style.border_spacing = spacing;
                    style.border_spacing_explicit = declaration.origin == StylesheetOrigin::Author;
                }
            }
            "background" => {
                apply_background_shorthand(style, value, declaration.base_url, declaration.root_url)
            }
            "background-color" => {
                style.background_color = parse_color(value);
            }
            "background-image" => {
                apply_background_image_list(
                    style,
                    value,
                    declaration.base_url,
                    declaration.root_url,
                );
            }
            "background-size" => {
                apply_background_size_list(style, value);
            }
            "background-position" => {
                apply_background_position_list(style, value);
            }
            "background-repeat" => {
                apply_background_repeat_list(style, value);
            }
            "background-origin" => {
                apply_background_origin_list(style, value);
            }
            "background-clip" => {
                apply_background_clip_list(style, value);
            }
            "border-image" => {
                if let Some(mut border_image) = parse_border_image(value, style.font_size) {
                    border_image.source_base_url = border_image
                        .source
                        .as_ref()
                        .and_then(|_| declaration.base_url.map(std::path::Path::to_path_buf));
                    border_image.source_root_url = border_image
                        .source
                        .as_ref()
                        .and_then(|_| declaration.root_url.map(std::path::Path::to_path_buf));
                    style.border_image = border_image;
                }
            }
            "border-image-source" => {
                if let Some(source) = parse_border_image_source(value) {
                    style.border_image.source = source;
                    style.border_image.source_base_url = style
                        .border_image
                        .source
                        .as_ref()
                        .and_then(|_| declaration.base_url.map(std::path::Path::to_path_buf));
                    style.border_image.source_root_url = style
                        .border_image
                        .source
                        .as_ref()
                        .and_then(|_| declaration.root_url.map(std::path::Path::to_path_buf));
                }
            }
            "border-image-slice" => {
                if let Some(slice) = parse_border_image_slice(value) {
                    style.border_image.slice = slice;
                }
            }
            "border-image-width" => {
                if let Some(width) = parse_border_image_width(value, style.font_size) {
                    style.border_image.width = width;
                }
            }
            "border-image-outset" => {
                if let Some(outset) = parse_border_image_outset(value, style.font_size) {
                    style.border_image.outset = outset;
                }
            }
            "border-image-repeat" => {
                if let Some(repeat) = parse_border_image_repeat(value) {
                    style.border_image.repeat = repeat;
                }
            }
            "color" => {
                if let Some(color) = parse_color(value) {
                    style.color = color;
                }
            }
            "font-size" => {
                // Applied in a pre-pass so same-rule `em` lengths use the
                // element's computed font size instead of declaration order.
            }
            "font" => {
                if let Some(font) = parse_font_shorthand_with_line_height_font_size(
                    value,
                    inheritance_source.font_size,
                    parent_ch_advance,
                    style.font_weight,
                    Some(style.font_size),
                ) {
                    style.font_style = font.style;
                    style.font_weight = font.weight;
                    style.font_width = font.width;
                    style.font_family = font.family;
                    style.font_size_adjust = FontSizeAdjust::None;
                    style.font_variant_ligatures = FontVariantLigatures::Normal;
                    style.font_variant_position = FontVariantPosition::Normal;
                    style.font_variant_caps = font.variant_caps;
                    style.font_variant_numeric = FontVariantNumeric::Normal;
                    style.font_variant_alternates = FontVariantAlternates::Normal;
                    style.font_variant_east_asian = FontVariantEastAsian::Normal;
                    style.font_variant_emoji = FontVariantEmoji::Normal;
                    style.line_height_value =
                        font.line_height.unwrap_or(ComputedLineHeight::Normal);
                    project_line_height(style);
                }
            }
            "line-height" => {
                if let Some(line_height) = parse_computed_line_height(value, style.font_size) {
                    style.line_height_value = line_height;
                    project_line_height(style);
                }
            }
            "letter-spacing" => {
                if let Some(letter_spacing) = parse_letter_spacing(value, style.font_size) {
                    style.letter_spacing = letter_spacing;
                }
            }
            "word-spacing" => {
                if let Some(word_spacing) = parse_word_spacing(value, style.font_size) {
                    style.word_spacing = word_spacing;
                }
            }
            "width" => {
                style.box_values.width = parse_computed_box_size(value, style.font_size)
                    .unwrap_or(style.box_values.width);
            }
            "height" => {
                style.box_values.height = parse_computed_box_size(value, style.font_size)
                    .unwrap_or(style.box_values.height);
            }
            "min-width" => {
                style.box_values.min_width = parse_computed_box_size(value, style.font_size)
                    .unwrap_or(style.box_values.min_width);
            }
            "max-width" => {
                style.box_values.max_width = parse_computed_box_size(value, style.font_size)
                    .unwrap_or(style.box_values.max_width);
            }
            "min-height" => {
                style.box_values.min_height = parse_computed_box_size(value, style.font_size)
                    .unwrap_or(style.box_values.min_height);
            }
            "max-height" => {
                style.box_values.max_height = parse_computed_box_size(value, style.font_size)
                    .unwrap_or(style.box_values.max_height);
            }
            "box-sizing" => {
                style.box_sizing = match value.to_ascii_lowercase().as_str() {
                    "border-box" => BoxSizing::BorderBox,
                    "content-box" => BoxSizing::ContentBox,
                    _ => style.box_sizing,
                };
            }
            "left" => {
                style.box_values.inset_left =
                    parse_computed_length_percentage_auto(value, style.font_size)
                        .unwrap_or(style.box_values.inset_left);
            }
            "top" => {
                style.box_values.inset_top =
                    parse_computed_length_percentage_auto(value, style.font_size)
                        .unwrap_or(style.box_values.inset_top);
            }
            "right" => {
                style.box_values.inset_right =
                    parse_computed_length_percentage_auto(value, style.font_size)
                        .unwrap_or(style.box_values.inset_right);
            }
            "bottom" => {
                style.box_values.inset_bottom =
                    parse_computed_length_percentage_auto(value, style.font_size)
                        .unwrap_or(style.box_values.inset_bottom);
            }
            "position" => {
                if let Some(name) = parse_running_position(value) {
                    // CSS GCPM running elements are removed from normal flow
                    // and become available to page-margin `element()`.
                    // https://www.w3.org/TR/css-gcpm-3/#running-elements
                    style.position = Position::Static;
                    style.running_element_name = Some(name);
                } else {
                    style.position = match value.to_ascii_lowercase().as_str() {
                        "absolute" => Position::Absolute,
                        "fixed" => Position::Fixed,
                        "sticky" => Position::Sticky,
                        "relative" => Position::Relative,
                        "static" => Position::Static,
                        _ => style.position,
                    };
                    style.running_element_name = None;
                }
            }
            "float" => {
                style.float = match value.to_ascii_lowercase().as_str() {
                    "left" => Float::Left,
                    "right" => Float::Right,
                    "inline-start" => Float::InlineStart,
                    "inline-end" => Float::InlineEnd,
                    "none" => Float::None,
                    _ => style.float,
                };
            }
            "clear" => {
                style.clear = match value.to_ascii_lowercase().as_str() {
                    "left" => Clear::Left,
                    "right" => Clear::Right,
                    "both" => Clear::Both,
                    "inline-start" => Clear::InlineStart,
                    "inline-end" => Clear::InlineEnd,
                    "none" => Clear::None,
                    _ => style.clear,
                };
            }
            "z-index" => {
                let value = value.trim();
                style.z_index = if value.eq_ignore_ascii_case("auto") {
                    None
                } else {
                    value.parse::<i32>().ok().or(style.z_index)
                };
            }
            "opacity" => {
                if let Some(opacity) = parse_opacity(value) {
                    style.opacity = opacity;
                }
            }
            "transform" => {
                if let Some(transform) = parse_transform(value, style.font_size) {
                    style.transform = transform;
                }
            }
            "transform-origin" => {
                if let Some(origin) = parse_transform_origin(value, style.font_size) {
                    style.transform_origin = origin;
                }
            }
            "isolation" => {
                style.isolation = match value.to_ascii_lowercase().as_str() {
                    "isolate" => Isolation::Isolate,
                    "auto" => Isolation::Auto,
                    _ => style.isolation,
                };
            }
            "mix-blend-mode" => {
                if let Some(mode) = parse_mix_blend_mode(value) {
                    style.mix_blend_mode = mode;
                }
            }
            "filter" => {
                let value = trim_css_value(value);
                style.filter = if value.eq_ignore_ascii_case("none") {
                    FilterValue::None
                } else {
                    FilterValue::Functions(value.to_string())
                };
            }
            "clip-path" => {
                if let Some(clip_path) = parse_clip_path(value) {
                    style.clip_path = clip_path;
                }
            }
            "mask" | "mask-image" => {
                let value = trim_css_value(value);
                style.mask = if value.eq_ignore_ascii_case("none") {
                    MaskValue::None
                } else {
                    MaskValue::Image(value.to_string())
                };
            }
            "contain" => {
                if let Some(contain) = parse_contain(value) {
                    style.contain = contain;
                }
            }
            "content-visibility" => {
                style.content_visibility = match value.to_ascii_lowercase().as_str() {
                    "visible" => ContentVisibility::Visible,
                    "auto" => ContentVisibility::Auto,
                    "hidden" => ContentVisibility::Hidden,
                    _ => style.content_visibility,
                };
            }
            "will-change" => {
                if let Some(will_change) = parse_will_change(value) {
                    style.will_change = will_change;
                }
            }
            "text-align" => {
                if value.eq_ignore_ascii_case("justify-all") {
                    style.text_align = TextAlign::JustifyAll;
                    style.text_align_last = TextAlignLast::Auto;
                } else if let Some(align) = parse_text_align_all(value, inheritance_source, true) {
                    style.text_align = align;
                    style.text_align_last = TextAlignLast::Auto;
                }
            }
            "text-align-all" => {
                if let Some(align) = parse_text_align_all(value, inheritance_source, false) {
                    style.text_align = align;
                }
            }
            "text-align-last" => {
                if let Some(align) = parse_text_align_last(value, inheritance_source) {
                    style.text_align_last = align;
                }
            }
            "text-justify" => {
                style.text_justify = match value.trim().to_ascii_lowercase().as_str() {
                    "auto" => TextJustify::Auto,
                    "inter-word" => TextJustify::InterWord,
                    "inter-character" | "distribute" => TextJustify::InterCharacter,
                    "none" => TextJustify::None,
                    _ => style.text_justify,
                };
            }
            "text-autospace" => {
                if let Some(text_autospace) = parse_text_autospace(value) {
                    style.text_autospace = text_autospace;
                }
            }
            "text-indent" => {
                if let Some(text_indent) = parse_text_indent(value, style.font_size) {
                    style.text_indent = text_indent;
                }
            }
            "hanging-punctuation" => {
                if let Some(hanging_punctuation) = parse_hanging_punctuation(value) {
                    style.hanging_punctuation = hanging_punctuation;
                }
            }
            "vertical-align" => {
                if let Some(vertical_align) = parse_vertical_align(value, style.font_size) {
                    style.vertical_align = vertical_align;
                }
            }
            "dominant-baseline" => {
                if let Some(dominant_baseline) = parse_dominant_baseline(value) {
                    style.vertical_align.dominant_baseline = dominant_baseline;
                }
            }
            "alignment-baseline" => {
                if let Some(alignment_baseline) = parse_alignment_baseline(value) {
                    style.vertical_align.alignment_baseline = alignment_baseline;
                }
            }
            "baseline-source" => {
                if let Some(baseline_source) = parse_baseline_source(value) {
                    style.vertical_align.baseline_source = baseline_source;
                }
            }
            "baseline-shift" => {
                if let Some(baseline_shift) = parse_baseline_shift(value, style.font_size) {
                    style.vertical_align.baseline_shift = baseline_shift;
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
            "bookmark-level" => {
                if let Some(level) = parse_bookmark_level(value) {
                    style.bookmark_level = level;
                }
            }
            "bookmark-label" => {
                if let Some(label) = parse_bookmark_label(value) {
                    style.bookmark_label = label;
                }
            }
            "bookmark-state" => {
                if let Some(state) = parse_bookmark_state(value) {
                    style.bookmark_state = state;
                }
            }
            "text-transform" => {
                if let Some(transform) = parse_text_transform(value) {
                    style.text_transform = transform;
                }
            }
            "tab-size" => {
                if let Some(tab_size) = parse_tab_size(value, style.font_size) {
                    style.tab_size = tab_size;
                }
            }
            "visibility" => {
                style.visibility = match value.trim().to_ascii_lowercase().as_str() {
                    "hidden" => Visibility::Hidden,
                    "collapse" => Visibility::Collapse,
                    "visible" => Visibility::Visible,
                    _ => style.visibility,
                };
            }
            "list-style" => {
                if let Some(components) = parse_list_style_shorthand(value) {
                    if let Some(style_type) = parse_list_style_type(&components.style_type) {
                        style.list_style_type = style_type;
                    }
                    if let Some(position) = parse_list_style_position(&components.position) {
                        style.list_style_position = position;
                    }
                    if components.image.eq_ignore_ascii_case("none") {
                        style.list_style_image = None;
                        style.list_style_image_base_url = None;
                        style.list_style_image_root_url = None;
                    } else if let Some(Some(image)) =
                        parse_list_style_image_component(&components.image)
                    {
                        style.list_style_image = Some(image);
                        style.list_style_image_base_url =
                            declaration.base_url.map(std::path::Path::to_path_buf);
                        style.list_style_image_root_url =
                            declaration.root_url.map(std::path::Path::to_path_buf);
                    }
                }
            }
            "list-style-type" => {
                style.list_style_type =
                    parse_list_style_type(value).unwrap_or_else(|| style.list_style_type.clone());
            }
            "list-style-position" => {
                style.list_style_position =
                    parse_list_style_position(value).unwrap_or(style.list_style_position);
            }
            "marker-side" => {
                if let Some(marker_side) = parse_marker_side(value) {
                    style.marker_side = marker_side;
                }
            }
            "list-style-image" => {
                if value.trim().eq_ignore_ascii_case("none") {
                    style.list_style_image = None;
                    style.list_style_image_base_url = None;
                    style.list_style_image_root_url = None;
                } else if let Some(image) = extract_css_url(value) {
                    style.list_style_image = Some(image);
                    style.list_style_image_base_url =
                        declaration.base_url.map(std::path::Path::to_path_buf);
                    style.list_style_image_root_url =
                        declaration.root_url.map(std::path::Path::to_path_buf);
                }
            }
            "counter-reset" => {
                if let Some(values) =
                    parse_counter_pairs(value, 0, CounterDuplicatePolicy::KeepLast)
                {
                    style.counter_resets = values;
                }
            }
            "counter-increment" => {
                if let Some(values) = parse_counter_pairs(value, 1, CounterDuplicatePolicy::KeepAll)
                {
                    style.counter_increments = values;
                }
            }
            "counter-set" => {
                if let Some(values) =
                    parse_counter_pairs(value, 0, CounterDuplicatePolicy::KeepLast)
                {
                    style.counter_sets = values;
                }
            }
            "string-set" => {
                // CSS Generated Content for Paged Media defines named strings
                // as generated content captured from document elements and
                // later referenced in page-margin boxes with `string()`.
                // https://www.w3.org/TR/css-gcpm-3/#named-strings
                if let Some(values) = parse_named_string_sets(value) {
                    style.string_sets = values;
                }
            }
            "page" => {
                let value = value.trim();
                if value.eq_ignore_ascii_case("auto") {
                    style.page_name = None;
                    style.page_name_specified = true;
                } else if is_css_identifier(value) {
                    style.page_name = Some(value.to_string());
                    style.page_name_specified = true;
                }
            }
            "break-before" | "page-break-before" => {
                style.break_before = parse_page_break(value);
            }
            "break-after" | "page-break-after" => {
                style.break_after = parse_page_break(value);
            }
            "break-inside" | "page-break-inside" => {
                style.break_inside_avoid = matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "avoid" | "avoid-page"
                );
            }
            "orphans" => {
                if let Some(value) = parse_positive_integer(value) {
                    style.orphans = value;
                }
            }
            "widows" => {
                if let Some(value) = parse_positive_integer(value) {
                    style.widows = value;
                }
            }
            "text-decoration" => {
                if let Some(decoration) = parse_text_decoration_shorthand(value, style) {
                    style.text_decoration = decoration;
                }
            }
            "text-decoration-line" => {
                if let Some(line) = parse_text_decoration_line(value) {
                    apply_text_decoration_line(&mut style.text_decoration, line);
                }
            }
            "text-decoration-style" => {
                if let Some(decoration_style) = parse_text_decoration_style(value) {
                    style.text_decoration.style = decoration_style;
                }
            }
            "text-decoration-color" => {
                if let Some(color) = parse_color(value) {
                    style.text_decoration.color = Some(color);
                }
            }
            "text-decoration-thickness" => {
                if let Some(thickness) = parse_text_decoration_thickness(value, style.font_size) {
                    style.text_decoration.thickness = thickness;
                }
            }
            "text-decoration-inset" => {
                if let Some(inset) = parse_text_decoration_inset(value, style.font_size) {
                    style.text_decoration.inset = inset;
                }
            }
            "text-decoration-skip" => {
                if let Some((skip_ink, skip_self, skip_box, skip_spaces)) =
                    parse_text_decoration_skip(value)
                {
                    style.text_decoration.skip_ink = skip_ink;
                    style.text_decoration.skip_self = skip_self;
                    style.text_decoration.skip_box = skip_box;
                    style.text_decoration.skip_spaces = skip_spaces;
                }
            }
            "text-decoration-skip-ink" => {
                if let Some(skip_ink) = parse_text_decoration_skip_ink(value) {
                    style.text_decoration.skip_ink = skip_ink;
                }
            }
            "text-decoration-skip-self" => {
                if let Some(skip_self) = parse_text_decoration_skip_self(value) {
                    style.text_decoration.skip_self = skip_self;
                }
            }
            "text-decoration-skip-box" => {
                if let Some(skip_box) = parse_text_decoration_skip_box(value) {
                    style.text_decoration.skip_box = skip_box;
                }
            }
            "text-decoration-skip-spaces" => {
                if let Some(skip_spaces) = parse_text_decoration_skip_spaces(value) {
                    style.text_decoration.skip_spaces = skip_spaces;
                }
            }
            "text-underline-offset" => {
                if let Some(offset) = parse_text_underline_offset(value, style.font_size) {
                    style.text_decoration.underline_offset = offset;
                }
            }
            "text-underline-position" => {
                if let Some(position) = parse_text_underline_position(value) {
                    style.text_decoration.underline_position = position;
                }
            }
            "text-emphasis-style" => {
                if let Some(emphasis_style) = parse_text_emphasis_style(value) {
                    style.text_emphasis_style = emphasis_style;
                }
            }
            "text-emphasis" => {
                if let Some((emphasis_style, emphasis_color)) = parse_text_emphasis(value) {
                    style.text_emphasis_style = emphasis_style;
                    style.text_emphasis_color = emphasis_color;
                }
            }
            "text-emphasis-color" => {
                if let Some(color) = parse_color(value) {
                    style.text_emphasis_color = Some(color);
                }
            }
            "text-emphasis-position" => {
                if let Some(position) = parse_text_emphasis_position(value) {
                    style.text_emphasis_position = position;
                }
            }
            "text-emphasis-skip" => {
                if let Some(skip) = parse_text_emphasis_skip(value) {
                    style.text_emphasis_skip = skip;
                }
            }
            "text-shadow" => {
                if let Some(shadows) = parse_text_shadow(value, style.font_size) {
                    style.text_shadow = shadows;
                }
            }
            "box-shadow" => {
                if let Some(shadows) = parse_box_shadow(value, style.font_size) {
                    style.box_shadow = shadows;
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
            "word-break" => {
                match value.trim().to_ascii_lowercase().as_str() {
                    "normal" => style.word_break = WordBreak::Normal,
                    "break-all" => style.word_break = WordBreak::BreakAll,
                    "keep-all" => style.word_break = WordBreak::KeepAll,
                    "break-word" => {
                        // CSS Text defines legacy `word-break: break-word` as
                        // normal word breaking plus emergency wrapping when no
                        // earlier soft wrap opportunity can fit the line:
                        // https://www.w3.org/TR/css-text-3/#word-break-property
                        style.word_break = WordBreak::Normal;
                        style.overflow_wrap = OverflowWrap::BreakWord;
                    }
                    _ => {}
                }
            }
            "overflow" => {
                if let Some(overflow) = parse_overflow_value(value) {
                    style.overflow = overflow;
                    style.overflow_x = overflow;
                    style.overflow_y = overflow;
                }
            }
            "overflow-x" => {
                if let Some(overflow) = parse_overflow_value(value) {
                    style.overflow_x = overflow;
                }
            }
            "overflow-y" => {
                if let Some(overflow) = parse_overflow_value(value) {
                    style.overflow_y = overflow;
                }
            }
            "overflow-wrap" | "word-wrap" => {
                style.overflow_wrap = match value.trim().to_ascii_lowercase().as_str() {
                    "normal" => OverflowWrap::Normal,
                    "anywhere" => OverflowWrap::Anywhere,
                    "break-word" => OverflowWrap::BreakWord,
                    _ => style.overflow_wrap,
                };
            }
            "line-break" => {
                style.line_break = match value.trim().to_ascii_lowercase().as_str() {
                    "auto" => LineBreak::Auto,
                    "loose" => LineBreak::Loose,
                    "normal" => LineBreak::Normal,
                    "strict" => LineBreak::Strict,
                    "anywhere" => LineBreak::Anywhere,
                    _ => style.line_break,
                };
            }
            "hyphens" => {
                style.hyphens = match value.trim().to_ascii_lowercase().as_str() {
                    "none" => Hyphens::None,
                    "manual" => Hyphens::Manual,
                    "auto" => Hyphens::Auto,
                    _ => style.hyphens,
                };
            }
            "hyphenate-limit-chars" => {
                if let Some(limit) = parse_hyphenate_limit_chars(value) {
                    style.hyphenate_limit_chars = limit;
                }
            }
            "content" => {
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

/// Parses one CSS margin side into its typed computed value.
///
/// CSS Box Model defines margin side properties, including `auto`, and CSS
/// Values defines length-percentage values:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
pub(super) fn set_margin_side(
    value: &str,
    font_size: f32,
    set: impl FnOnce(ComputedLengthPercentageOrAuto),
) {
    if let Some(value) = parse_computed_length_percentage_auto(value, font_size) {
        set(value);
    }
}

/// Applies a logical margin axis shorthand to computed physical margin edges.
///
/// CSS Logical Properties maps `margin-block` and `margin-inline` through the
/// computed writing mode and direction, and CSS Box Model permits `auto`
/// margins:
/// <https://www.w3.org/TR/css-logical-1/#margin-properties> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
fn apply_logical_margin_axis(
    value: &str,
    style: &mut ComputedStyle,
    name: &str,
    origin: StylesheetOrigin,
) {
    let Some([start, end]) = logical_box_axis_side_names(name) else {
        return;
    };
    let parts = split_css_component_values(trim_css_value(value));
    let [start_value, end_value] = match parts.as_slice() {
        [all] => [*all, *all],
        [start, end] => [*start, *end],
        _ => return,
    };
    apply_logical_margin_side(start_value, style, start, origin);
    apply_logical_margin_side(end_value, style, end, origin);
}

/// Applies one logical margin longhand to its resolved physical side.
///
/// CSS Logical Properties defines flow-relative margin longhands as aliases
/// for physical margin sides:
/// <https://www.w3.org/TR/css-logical-1/#margin-properties>.
fn apply_logical_margin_side(
    value: &str,
    style: &mut ComputedStyle,
    name: &str,
    origin: StylesheetOrigin,
) {
    let Some(side) = logical_box_side(name, style.direction, style.writing_mode) else {
        return;
    };
    set_margin_side(value, style.font_size, |typed| {
        set_margin_box_side(style, side, typed);
        set_ua_margin_em_side(
            style,
            side,
            (origin == StylesheetOrigin::UserAgent)
                .then(|| parse_em_length_factor(value))
                .flatten(),
        );
    });
}

fn set_margin_box_side(
    style: &mut ComputedStyle,
    side: BoxSide,
    typed: ComputedLengthPercentageOrAuto,
) {
    let length = typed.length_if_no_percent().unwrap_or(0.0);
    match side {
        BoxSide::Top => {
            style.box_values.margin.top = typed;
            style.margin.top = length;
        }
        BoxSide::Right => {
            style.box_values.margin.right = typed;
            style.margin.right = length;
        }
        BoxSide::Bottom => {
            style.box_values.margin.bottom = typed;
            style.margin.bottom = length;
        }
        BoxSide::Left => {
            style.box_values.margin.left = typed;
            style.margin.left = length;
        }
    }
}

fn set_ua_margin_em_side(style: &mut ComputedStyle, side: BoxSide, factor: Option<f32>) {
    match side {
        BoxSide::Top => style.ua_margin_em.top = factor,
        BoxSide::Right => style.ua_margin_em.right = factor,
        BoxSide::Bottom => style.ua_margin_em.bottom = factor,
        BoxSide::Left => style.ua_margin_em.left = factor,
    }
}

/// Parses one CSS length-percentage declaration into its typed computed value.
///
/// CSS Values and Units defines `<length-percentage>` and CSS Cascade defines
/// the computed-value stage:
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages> and
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
pub(super) fn set_computed_length_percentage(
    value: &str,
    font_size: f32,
    set: impl FnOnce(ComputedLengthPercentage),
) {
    if let Some(value) = parse_computed_length_percentage(value, font_size) {
        set(value);
    }
}

/// Applies a logical padding axis shorthand to computed physical padding edges.
///
/// CSS Logical Properties maps `padding-block` and `padding-inline` through
/// the computed writing mode and direction:
/// <https://www.w3.org/TR/css-logical-1/#padding-properties>.
fn apply_logical_padding_axis(value: &str, style: &mut ComputedStyle, name: &str) {
    let Some([start, end]) = logical_box_axis_side_names(name) else {
        return;
    };
    let parts = split_css_component_values(trim_css_value(value));
    let [start_value, end_value] = match parts.as_slice() {
        [all] => [*all, *all],
        [start, end] => [*start, *end],
        _ => return,
    };
    apply_logical_padding_side(start_value, style, start);
    apply_logical_padding_side(end_value, style, end);
}

/// Applies one logical padding longhand to its resolved physical side.
///
/// CSS Logical Properties defines flow-relative padding longhands as aliases
/// for physical padding sides:
/// <https://www.w3.org/TR/css-logical-1/#padding-properties>.
fn apply_logical_padding_side(value: &str, style: &mut ComputedStyle, name: &str) {
    let Some(side) = logical_box_side(name, style.direction, style.writing_mode) else {
        return;
    };
    set_computed_length_percentage(value, style.font_size, |typed| {
        set_padding_box_side(style, side, typed);
    });
}

fn set_padding_box_side(style: &mut ComputedStyle, side: BoxSide, typed: ComputedLengthPercentage) {
    let length = typed.length_if_no_percent();
    match side {
        BoxSide::Top => {
            style.box_values.padding.top = typed;
            if let Some(length) = length {
                style.padding.top = length;
            }
        }
        BoxSide::Right => {
            style.box_values.padding.right = typed;
            if let Some(length) = length {
                style.padding.right = length;
            }
        }
        BoxSide::Bottom => {
            style.box_values.padding.bottom = typed;
            if let Some(length) = length {
                style.padding.bottom = length;
            }
        }
        BoxSide::Left => {
            style.box_values.padding.left = typed;
            if let Some(length) = length {
                style.padding.left = length;
            }
        }
    }
}

/// Projects typed computed padding edges into the current length-only renderer cache.
///
/// CSS Cascade defines computed values, while CSS Box Model defines padding
/// edge properties:
/// <https://www.w3.org/TR/css-cascade-5/#computed> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(super) fn legacy_edge_lengths(values: CssEdges<ComputedLengthPercentage>) -> Option<Edges> {
    Some(Edges {
        top: values.top.length_if_no_percent()?,
        right: values.right.length_if_no_percent()?,
        bottom: values.bottom.length_if_no_percent()?,
        left: values.left.length_if_no_percent()?,
    })
}

/// Projects typed computed margin edges into the current length-only renderer cache.
///
/// CSS Cascade defines computed values, while CSS Box Model defines margin
/// edge properties and `auto` margins:
/// <https://www.w3.org/TR/css-cascade-5/#computed> and
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties>.
pub(super) fn legacy_margin_edges(values: CssEdges<ComputedLengthPercentageOrAuto>) -> Edges {
    Edges {
        top: values.top.length_if_no_percent().unwrap_or(0.0),
        right: values.right.length_if_no_percent().unwrap_or(0.0),
        bottom: values.bottom.length_if_no_percent().unwrap_or(0.0),
        left: values.left.length_if_no_percent().unwrap_or(0.0),
    }
}

/// Parses UA stylesheet `em` margins for delayed font-size-relative resolution.
///
/// CSS Values defines `em` units as font-relative lengths, and CSS Cascade
/// defines the computed-value stage where font-relative values are resolved:
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
pub(super) fn parse_margin_em_edges(value: &str) -> OptionalEdges<f32> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    let [top, right, bottom, left] = match parts.as_slice() {
        [] => return OptionalEdges::NONE,
        [all] => [all, all, all, all],
        [vertical, horizontal] => [vertical, horizontal, vertical, horizontal],
        [top, horizontal, bottom] => [top, horizontal, bottom, horizontal],
        [top, right, bottom, left, ..] => [top, right, bottom, left],
    };
    OptionalEdges {
        top: parse_em_length_factor(top),
        right: parse_em_length_factor(right),
        bottom: parse_em_length_factor(bottom),
        left: parse_em_length_factor(left),
    }
}

pub(super) fn parse_em_length_factor(value: &str) -> Option<f32> {
    trim_css_value(value)
        .to_ascii_lowercase()
        .strip_suffix("em")
        .and_then(|factor| factor.trim().parse::<f32>().ok())
}

pub(super) fn parse_positive_integer(value: &str) -> Option<usize> {
    let value = value.trim();
    if value.starts_with('+') || value.starts_with('-') || value.contains('.') {
        return None;
    }
    value.parse::<usize>().ok().filter(|value| *value > 0)
}

/// Parse CSS `hyphenate-limit-chars`.
///
/// CSS Text defines the grammar as one to three values, each `auto` or a
/// positive integer: total word length, minimum characters before the break,
/// and minimum characters after the break:
/// <https://www.w3.org/TR/css-text-4/#hyphenate-limit-chars>.
fn parse_hyphenate_limit_chars(value: &str) -> Option<HyphenateLimitChars> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let total = parse_hyphenate_limit_component(parts[0], HyphenateLimitChars::AUTO_TOTAL)?;
    let before = parts
        .get(1)
        .map(|value| parse_hyphenate_limit_component(value, HyphenateLimitChars::AUTO_BEFORE))
        .unwrap_or(Some(HyphenateLimitChars::AUTO_BEFORE))?;
    let after = parts
        .get(2)
        .map(|value| parse_hyphenate_limit_component(value, HyphenateLimitChars::AUTO_AFTER))
        .unwrap_or(Some(HyphenateLimitChars::AUTO_AFTER))?;
    Some(HyphenateLimitChars {
        total,
        before,
        after,
    })
}

fn parse_hyphenate_limit_component(value: &str, auto_value: u16) -> Option<u16> {
    if value.eq_ignore_ascii_case("auto") {
        return Some(auto_value);
    }
    u16::try_from(parse_positive_integer(value)?).ok()
}

pub(super) fn is_css_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '-' || first.is_ascii_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '-' || character.is_ascii_alphanumeric()
        })
}

pub(super) fn parse_running_position(value: &str) -> Option<String> {
    let value = value.trim();
    let prefix = value.get(.."running".len())?;
    if !prefix.eq_ignore_ascii_case("running") {
        return None;
    }
    let argument = value["running".len()..]
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    is_css_identifier(argument).then(|| argument.to_string())
}

pub(super) fn parse_page_break(value: &str) -> PageBreak {
    match value.trim().to_ascii_lowercase().as_str() {
        "avoid" | "avoid-page" => PageBreak::Avoid,
        "page" | "always" => PageBreak::Page,
        "left" => PageBreak::Left,
        "right" => PageBreak::Right,
        "recto" => PageBreak::Recto,
        "verso" => PageBreak::Verso,
        _ => PageBreak::Auto,
    }
}

/// Computes the writing context used to resolve logical properties.
///
/// CSS Logical Properties maps flow-relative properties through the computed
/// `direction` and `writing-mode` values. This prepass runs before shorthand
/// expansion and Cascade 5 rollback so logical and physical border longhands
/// compare in the right physical space:
/// <https://www.w3.org/TR/css-logical-1/#flow-relative> and
/// <https://www.w3.org/TR/css-cascade-5/#cascade>.
fn logical_mapping_context(
    base_style: &ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
) -> (Direction, WritingMode) {
    let mut direction = base_style.direction;
    let mut writing_mode = base_style.writing_mode;
    for declaration in declarations {
        let name = declaration.name.as_ref();
        let value = trim_css_value(&declaration.value);
        if value.contains("var(")
            || declaration_is_revert(value)
            || declaration_is_revert_layer(value)
        {
            continue;
        }
        if name.eq_ignore_ascii_case("all") {
            if let Some(keyword) = CssWideDefaultKeyword::parse(value) {
                writing_mode = defaulted_writing_mode(keyword, inheritance_source);
            }
            continue;
        }
        if let Some(keyword) = CssWideDefaultKeyword::parse(value) {
            match name {
                "direction" => direction = defaulted_direction(keyword, inheritance_source),
                "writing-mode" => {
                    writing_mode = defaulted_writing_mode(keyword, inheritance_source);
                }
                _ => {}
            }
            continue;
        }
        match name {
            "direction" => {
                if let Some(parsed) = parse_direction(value) {
                    direction = parsed;
                }
            }
            "writing-mode" => {
                if let Some(parsed) = parse_writing_mode(value) {
                    writing_mode = parsed;
                }
            }
            _ => {}
        }
    }
    (direction, writing_mode)
}

fn defaulted_direction(
    keyword: CssWideDefaultKeyword,
    inheritance_source: &ComputedStyle,
) -> Direction {
    match keyword {
        CssWideDefaultKeyword::Initial => ComputedStyle::initial().direction,
        CssWideDefaultKeyword::Inherit | CssWideDefaultKeyword::Unset => {
            inheritance_source.direction
        }
    }
}

fn defaulted_writing_mode(
    keyword: CssWideDefaultKeyword,
    inheritance_source: &ComputedStyle,
) -> WritingMode {
    match keyword {
        CssWideDefaultKeyword::Initial => ComputedStyle::initial().writing_mode,
        CssWideDefaultKeyword::Inherit | CssWideDefaultKeyword::Unset => {
            inheritance_source.writing_mode
        }
    }
}

/// Parses CSS `direction`.
///
/// CSS Writing Modes defines `direction` keywords as `ltr` and `rtl`:
/// <https://www.w3.org/TR/css-writing-modes-4/#direction>.
fn parse_direction(value: &str) -> Option<Direction> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "ltr" => Some(Direction::Ltr),
        "rtl" => Some(Direction::Rtl),
        _ => None,
    }
}

/// Parses CSS `writing-mode` values currently supported by layout.
///
/// CSS Writing Modes defines horizontal and vertical flow modes; sideways
/// modes are intentionally left unsupported until text layout supports them:
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
fn parse_writing_mode(value: &str) -> Option<WritingMode> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "horizontal-tb" => Some(WritingMode::HorizontalTb),
        "vertical-rl" => Some(WritingMode::VerticalRl),
        "vertical-lr" => Some(WritingMode::VerticalLr),
        _ => None,
    }
}

/// Parses CSS `text-orientation` values supported by vertical text placement.
///
/// CSS Writing Modes defines `mixed`, `upright`, and `sideways` as the modern
/// orientation keywords. Deprecated SVG aliases are intentionally left
/// unsupported until compatibility tests require them:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
fn parse_text_orientation(value: &str) -> Option<TextOrientation> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "mixed" => Some(TextOrientation::Mixed),
        "upright" => Some(TextOrientation::Upright),
        "sideways" => Some(TextOrientation::Sideways),
        _ => None,
    }
}

/// Parses CSS `opacity`.
///
/// CSS Color defines `opacity` as a number or percentage clamped to the
/// `[0, 1]` range:
/// <https://www.w3.org/TR/css-color-4/#transparency>.
fn parse_opacity(value: &str) -> Option<f32> {
    let value = trim_css_value(value);
    if let Some(percent) = parse_percentage(value) {
        return Some(percent.clamp(0.0, 1.0));
    }
    value.parse::<f32>().ok().map(|value| value.clamp(0.0, 1.0))
}

/// Parses the supported CSS 2D `transform` function list.
///
/// CSS Transforms Level 1 defines the 2D functions accepted here. The
/// implementation intentionally rejects 3D functions until the layout and PDF
/// backends carry 3D matrices:
/// <https://www.w3.org/TR/css-transforms-1/#two-d-transform-functions>.
fn parse_transform(value: &str, font_size: f32) -> Option<TransformList> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let functions = parse_transform_function_calls(value)?;
    let mut transform = Vec::new();
    for (name, args) in functions {
        let lower_name = name.to_ascii_lowercase();
        let comma_args = split_css_args(args, ',');
        let args = if comma_args.len() > 1 {
            comma_args
        } else {
            split_css_whitespace_args(args)
        };
        match lower_name.as_str() {
            "matrix" if args.len() == 6 => {
                transform.push(TransformFunction::Matrix(
                    parse_css_number(args[0])?,
                    parse_css_number(args[1])?,
                    parse_css_number(args[2])?,
                    parse_css_number(args[3])?,
                    parse_css_number(args[4])?,
                    parse_css_number(args[5])?,
                ));
            }
            "translate" if args.len() == 1 || args.len() == 2 => {
                let x = parse_computed_length_percentage(args[0], font_size)?;
                let y = args
                    .get(1)
                    .and_then(|arg| parse_computed_length_percentage(arg, font_size))
                    .unwrap_or(ComputedLengthPercentage::ZERO);
                transform.push(TransformFunction::Translate(x, y));
            }
            "translatex" if args.len() == 1 => {
                transform.push(TransformFunction::Translate(
                    parse_computed_length_percentage(args[0], font_size)?,
                    ComputedLengthPercentage::ZERO,
                ));
            }
            "translatey" if args.len() == 1 => {
                transform.push(TransformFunction::Translate(
                    ComputedLengthPercentage::ZERO,
                    parse_computed_length_percentage(args[0], font_size)?,
                ));
            }
            "scale" if args.len() == 1 || args.len() == 2 => {
                let x = parse_css_number(args[0])?;
                let y = args
                    .get(1)
                    .and_then(|arg| parse_css_number(arg))
                    .unwrap_or(x);
                transform.push(TransformFunction::Scale(x, y));
            }
            "scalex" if args.len() == 1 => {
                transform.push(TransformFunction::Scale(parse_css_number(args[0])?, 1.0));
            }
            "scaley" if args.len() == 1 => {
                transform.push(TransformFunction::Scale(1.0, parse_css_number(args[0])?));
            }
            "rotate" if args.len() == 1 => {
                transform.push(TransformFunction::Rotate(parse_css_angle_radians(args[0])?));
            }
            "skew" if args.len() == 1 || args.len() == 2 => {
                let x = parse_css_angle_radians(args[0])?;
                let y = args
                    .get(1)
                    .and_then(|arg| parse_css_angle_radians(arg))
                    .unwrap_or(0.0);
                transform.push(TransformFunction::Skew(x, y));
            }
            "skewx" if args.len() == 1 => {
                transform.push(TransformFunction::Skew(
                    parse_css_angle_radians(args[0])?,
                    0.0,
                ));
            }
            "skewy" if args.len() == 1 => {
                transform.push(TransformFunction::Skew(
                    0.0,
                    parse_css_angle_radians(args[0])?,
                ));
            }
            _ => return None,
        }
    }
    Some(transform)
}

/// Parses 2D `transform-origin`, ignoring a third z-origin component.
///
/// CSS Transforms resolves keyword origins to percentages over the border box:
/// <https://www.w3.org/TR/css-transforms-1/#transform-origin-property>.
fn parse_transform_origin(value: &str, font_size: f32) -> Option<TransformOrigin> {
    let parts = split_css_whitespace_args(trim_css_value(value));
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let parts = &parts[..parts.len().min(2)];
    match parts {
        [single] if is_vertical_origin_keyword(single) => Some(TransformOrigin {
            x: ComputedLengthPercentage::from_percent(0.5),
            y: parse_origin_component(single, true, font_size)?,
        }),
        [single] => Some(TransformOrigin {
            x: parse_origin_component(single, false, font_size)?,
            y: ComputedLengthPercentage::from_percent(0.5),
        }),
        [first, second] if is_vertical_origin_keyword(first) => Some(TransformOrigin {
            x: parse_origin_component(second, false, font_size)?,
            y: parse_origin_component(first, true, font_size)?,
        }),
        [first, second] => Some(TransformOrigin {
            x: parse_origin_component(first, false, font_size)?,
            y: parse_origin_component(second, true, font_size)?,
        }),
        _ => None,
    }
}

fn parse_origin_component(
    value: &str,
    vertical: bool,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    match value.to_ascii_lowercase().as_str() {
        "left" if !vertical => Some(ComputedLengthPercentage::ZERO),
        "right" if !vertical => Some(ComputedLengthPercentage::from_percent(1.0)),
        "top" if vertical => Some(ComputedLengthPercentage::ZERO),
        "bottom" if vertical => Some(ComputedLengthPercentage::from_percent(1.0)),
        "center" => Some(ComputedLengthPercentage::from_percent(0.5)),
        _ => parse_computed_length_percentage(value, font_size),
    }
}

fn is_vertical_origin_keyword(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "top" | "bottom")
}

/// Parse CSS Compositing's `mix-blend-mode` keywords.
///
/// Non-`normal` values establish a stacking context:
/// <https://www.w3.org/TR/compositing-1/#mix-blend-mode>.
fn parse_mix_blend_mode(value: &str) -> Option<MixBlendMode> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "normal" => Some(MixBlendMode::Normal),
        "multiply" => Some(MixBlendMode::Multiply),
        "screen" => Some(MixBlendMode::Screen),
        "overlay" => Some(MixBlendMode::Overlay),
        "darken" => Some(MixBlendMode::Darken),
        "lighten" => Some(MixBlendMode::Lighten),
        "color-dodge" => Some(MixBlendMode::ColorDodge),
        "color-burn" => Some(MixBlendMode::ColorBurn),
        "hard-light" => Some(MixBlendMode::HardLight),
        "soft-light" => Some(MixBlendMode::SoftLight),
        "difference" => Some(MixBlendMode::Difference),
        "exclusion" => Some(MixBlendMode::Exclusion),
        "hue" => Some(MixBlendMode::Hue),
        "saturation" => Some(MixBlendMode::Saturation),
        "color" => Some(MixBlendMode::Color),
        "luminosity" => Some(MixBlendMode::Luminosity),
        _ => None,
    }
}

/// Parse the `contain` keywords that affect paint isolation.
///
/// CSS Containment maps `strict` to size/layout/style/paint and `content` to
/// layout/style/paint containment:
/// <https://www.w3.org/TR/css-contain-2/#contain-property>.
fn parse_contain(value: &str) -> Option<Contain> {
    let value = trim_css_value(value).to_ascii_lowercase();
    if value == "none" {
        return Some(Contain::NONE);
    }
    if value == "strict" {
        return Some(Contain {
            size: true,
            layout: true,
            style: true,
            paint: true,
        });
    }
    if value == "content" {
        return Some(Contain {
            size: false,
            layout: true,
            style: true,
            paint: true,
        });
    }

    let mut contain = Contain::NONE;
    for token in value.split_whitespace() {
        match token {
            "size" | "inline-size" => contain.size = true,
            "layout" => contain.layout = true,
            "style" => contain.style = true,
            "paint" => contain.paint = true,
            _ => return None,
        }
    }
    (contain.size || contain.layout || contain.style || contain.paint).then_some(contain)
}

/// Parse `clip-path` coarsely enough to identify stacking-context triggers.
///
/// The current paint layer records non-`none` values as isolation triggers; path
/// geometry is implemented later in paint effects:
/// <https://www.w3.org/TR/css-masking-1/#the-clip-path>.
fn parse_clip_path(value: &str) -> Option<ClipPath> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(ClipPath::None);
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("url(") {
        Some(ClipPath::Url)
    } else if lower.starts_with("inset(") {
        Some(ClipPath::Inset)
    } else if lower.ends_with(')') {
        Some(ClipPath::Shape)
    } else {
        None
    }
}

/// Parse `will-change` features that may pre-create stacking contexts.
///
/// CSS Will Change lets authors request the same stacking behavior that the
/// named property would have at a non-initial value:
/// <https://www.w3.org/TR/css-will-change-1/#will-change>.
fn parse_will_change(value: &str) -> Option<WillChange> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(WillChange::default());
    }
    let mut will_change = WillChange::default();
    for token in value
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token.to_ascii_lowercase().as_str() {
            "contents" => will_change.contents = true,
            "scroll-position" => will_change.scroll_position = true,
            "opacity" => will_change.opacity = true,
            "transform" => will_change.transform = true,
            "filter" => will_change.filter = true,
            "clip-path" => will_change.clip_path = true,
            "mask" | "mask-image" => will_change.mask = true,
            "mix-blend-mode" => will_change.mix_blend_mode = true,
            "isolation" => will_change.isolation = true,
            "contain" => will_change.contain = true,
            _ => return None,
        }
    }
    Some(will_change)
}

fn parse_transform_function_calls(value: &str) -> Option<Vec<(&str, &str)>> {
    let mut calls = Vec::new();
    let mut rest = trim_css_value(value);
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = trim_css_value(&rest[..open]);
        if name.is_empty() {
            return None;
        }
        let close = find_matching_close_paren(rest, open)?;
        calls.push((name, &rest[open + 1..close]));
        rest = trim_css_value(&rest[close + 1..]);
    }
    Some(calls)
}

fn find_matching_close_paren(value: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in value.bytes().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_css_args(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            candidate if candidate == delimiter && depth == 0 => {
                parts.push(trim_css_value(&value[start..index]));
                start = index + candidate.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(trim_css_value(&value[start..]));
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

fn split_css_whitespace_args(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => {
                if start.is_none() {
                    start = Some(index);
                }
                depth += 1;
            }
            ')' => depth = depth.saturating_sub(1),
            character if character.is_whitespace() && depth == 0 => {
                if let Some(part_start) = start.take() {
                    let part = trim_css_value(&value[part_start..index]);
                    if !part.is_empty() {
                        parts.push(part);
                    }
                }
            }
            _ if start.is_none() => start = Some(index),
            _ => {}
        }
    }
    if let Some(part_start) = start {
        let part = trim_css_value(&value[part_start..]);
        if !part.is_empty() {
            parts.push(part);
        }
    }
    parts
}

fn parse_css_number(value: &str) -> Option<f32> {
    trim_css_value(value).parse::<f32>().ok()
}

fn parse_css_angle_radians(value: &str) -> Option<f32> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    if let Some(number) = lower.strip_suffix("deg") {
        return parse_css_number(number).map(f32::to_radians);
    }
    if let Some(number) = lower.strip_suffix("grad") {
        return parse_css_number(number).map(|value| value * std::f32::consts::PI / 200.0);
    }
    if let Some(number) = lower.strip_suffix("turn") {
        return parse_css_number(number).map(|value| value * std::f32::consts::TAU);
    }
    lower
        .strip_suffix("rad")
        .and_then(parse_css_number)
        .or_else(|| parse_css_number(value).filter(|value| *value == 0.0))
}

/// Parses the `flex-flow` shorthand into `flex-direction` and `flex-wrap`.
///
/// CSS Flexible Box Layout defines `flex-flow` as
/// `<'flex-direction'> || <'flex-wrap'>`; omitted components reset to their
/// initial values (`row` and `nowrap`):
/// <https://www.w3.org/TR/css-flexbox-1/#flex-flow-property>.
fn parse_flex_flow(value: &str) -> Option<(FlexDirection, FlexWrap)> {
    let mut direction = FlexDirection::Row;
    let mut wrap = FlexWrap::NoWrap;
    let mut saw_direction = false;
    let mut saw_wrap = false;
    for token in trim_css_value(value).split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "row" if !saw_direction => {
                direction = FlexDirection::Row;
                saw_direction = true;
            }
            "row-reverse" if !saw_direction => {
                direction = FlexDirection::RowReverse;
                saw_direction = true;
            }
            "column" if !saw_direction => {
                direction = FlexDirection::Column;
                saw_direction = true;
            }
            "column-reverse" if !saw_direction => {
                direction = FlexDirection::ColumnReverse;
                saw_direction = true;
            }
            "nowrap" if !saw_wrap => {
                wrap = FlexWrap::NoWrap;
                saw_wrap = true;
            }
            "wrap" if !saw_wrap => {
                wrap = FlexWrap::Wrap;
                saw_wrap = true;
            }
            "wrap-reverse" if !saw_wrap => {
                wrap = FlexWrap::WrapReverse;
                saw_wrap = true;
            }
            _ => return None,
        }
    }
    (saw_direction || saw_wrap).then_some((direction, wrap))
}

/// Parses a single CSS Overflow keyword.
///
/// CSS Overflow defines the `overflow`, `overflow-x`, and `overflow-y`
/// properties as keyword values controlling visible, clipped, and scrollable
/// overflow:
/// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>.
fn parse_overflow_value(value: &str) -> Option<Overflow> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "visible" => Some(Overflow::Visible),
        "hidden" => Some(Overflow::Hidden),
        "clip" => Some(Overflow::Clip),
        "scroll" => Some(Overflow::Scroll),
        "auto" => Some(Overflow::Auto),
        _ => None,
    }
}

/// Parse CSS Text Level 4's `text-autospace` keyword set.
///
/// The grammar accepts `normal`, `auto`, `no-autospace`, or an unordered set
/// of autospace features with an optional insertion/replacement mode. The
/// current layout engine uses insertion semantics for both modes because PDF
/// output has no editable text-replacement phase:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>.
fn parse_text_autospace(value: &str) -> Option<TextAutospace> {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 {
        return match tokens[0].to_ascii_lowercase().as_str() {
            "normal" | "auto" => Some(TextAutospace::NORMAL),
            "no-autospace" => Some(TextAutospace::NONE),
            "ideograph-alpha" => Some(TextAutospace {
                ideograph_alpha: true,
                ..TextAutospace::NONE
            }),
            "ideograph-numeric" => Some(TextAutospace {
                ideograph_numeric: true,
                ..TextAutospace::NONE
            }),
            "punctuation" => Some(TextAutospace {
                punctuation: true,
                ..TextAutospace::NONE
            }),
            _ => None,
        };
    }

    let mut autospace = TextAutospace::NONE;
    let mut saw_mode = false;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "normal" | "auto" | "no-autospace" => return None,
            "ideograph-alpha" if !autospace.ideograph_alpha => {
                autospace.ideograph_alpha = true;
            }
            "ideograph-numeric" if !autospace.ideograph_numeric => {
                autospace.ideograph_numeric = true;
            }
            "punctuation" if !autospace.punctuation => {
                autospace.punctuation = true;
            }
            "insert" | "replace" if !saw_mode => {
                saw_mode = true;
            }
            _ => return None,
        }
    }

    (!autospace.is_none()).then_some(autospace)
}

fn parse_text_align_all(
    value: &str,
    inheritance_source: &ComputedStyle,
    allow_justify_all: bool,
) -> Option<TextAlign> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "start" => Some(TextAlign::Start),
        "end" => Some(TextAlign::End),
        "center" => Some(TextAlign::Center),
        "right" => Some(TextAlign::Right),
        "justify" => Some(TextAlign::Justify),
        "justify-all" if allow_justify_all => Some(TextAlign::JustifyAll),
        "left" => Some(TextAlign::Left),
        "match-parent" => Some(resolve_match_parent_text_align(
            inheritance_source.text_align,
            inheritance_source.direction,
        )),
        _ => None,
    }
}

fn parse_text_align_last(value: &str, inheritance_source: &ComputedStyle) -> Option<TextAlignLast> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(TextAlignLast::Auto),
        "start" => Some(TextAlignLast::Align(TextAlign::Start)),
        "end" => Some(TextAlignLast::Align(TextAlign::End)),
        "center" => Some(TextAlignLast::Align(TextAlign::Center)),
        "right" => Some(TextAlignLast::Align(TextAlign::Right)),
        "justify" => Some(TextAlignLast::Align(TextAlign::Justify)),
        "left" => Some(TextAlignLast::Align(TextAlign::Left)),
        "match-parent" => Some(match inheritance_source.text_align_last {
            TextAlignLast::Auto => TextAlignLast::Auto,
            TextAlignLast::Align(align) => TextAlignLast::Align(resolve_match_parent_text_align(
                align,
                inheritance_source.direction,
            )),
        }),
        _ => None,
    }
}

fn resolve_match_parent_text_align(align: TextAlign, parent_direction: Direction) -> TextAlign {
    match align {
        TextAlign::Start | TextAlign::End => align.physical(parent_direction),
        TextAlign::JustifyAll => TextAlign::JustifyAll,
        align => align,
    }
}

/// Parse CSS Text's `text-transform` keyword set.
///
/// CSS Text defines `text-transform` as either `none` or a combination of at
/// most one case transform with optional `full-width` and `full-size-kana`:
/// <https://www.w3.org/TR/css-text-3/#text-transform-property>.
fn parse_text_transform(value: &str) -> Option<TextTransform> {
    let tokens = split_css_component_values(value);
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("none") {
        return Some(TextTransform::NONE);
    }

    let mut transform = TextTransform::NONE;
    let mut saw_case = false;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "none" => return None,
            "uppercase" if !saw_case => {
                transform.case = TextTransformCase::Uppercase;
                saw_case = true;
            }
            "lowercase" if !saw_case => {
                transform.case = TextTransformCase::Lowercase;
                saw_case = true;
            }
            "capitalize" if !saw_case => {
                transform.case = TextTransformCase::Capitalize;
                saw_case = true;
            }
            "full-width" if !transform.full_width => transform.full_width = true,
            "full-size-kana" if !transform.full_size_kana => transform.full_size_kana = true,
            _ => return None,
        }
    }

    (transform != TextTransform::NONE).then_some(transform)
}

#[derive(Debug, Clone, Copy)]
struct TextDecorationLineParts {
    underline: bool,
    overline: bool,
    line_through: bool,
    blink: bool,
    spelling_error: bool,
    grammar_error: bool,
}

/// Parses CSS `text-decoration-line`.
///
/// CSS Text Decoration defines `none` and a space-separated set of line
/// keywords. Repeated keywords or unknown keywords invalidate the declaration:
/// <https://www.w3.org/TR/css-text-decor-3/#text-decoration-line-property>.
fn parse_text_decoration_line(value: &str) -> Option<TextDecorationLineParts> {
    let parts = split_css_component_values(value);
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("none") {
        return Some(TextDecorationLineParts {
            underline: false,
            overline: false,
            line_through: false,
            blink: false,
            spelling_error: false,
            grammar_error: false,
        });
    }
    let mut line = TextDecorationLineParts {
        underline: false,
        overline: false,
        line_through: false,
        blink: false,
        spelling_error: false,
        grammar_error: false,
    };
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "none" => return None,
            "underline" if !line.underline => line.underline = true,
            "overline" if !line.overline => line.overline = true,
            "line-through" if !line.line_through => line.line_through = true,
            "blink" if !line.blink => line.blink = true,
            "spelling-error" if !line.spelling_error => line.spelling_error = true,
            "grammar-error" if !line.grammar_error => line.grammar_error = true,
            _ => return None,
        }
    }
    Some(line)
}

fn apply_text_decoration_line(decoration: &mut TextDecoration, line: TextDecorationLineParts) {
    decoration.underline = line.underline;
    decoration.overline = line.overline;
    decoration.line_through = line.line_through;
    decoration.blink = line.blink;
    decoration.spelling_error = line.spelling_error;
    decoration.grammar_error = line.grammar_error;
}

/// Parses CSS `text-decoration-style`.
///
/// CSS Text Decoration defines solid, double, dotted, dashed, and wavy
/// decoration styles:
/// <https://www.w3.org/TR/css-text-decor-3/#text-decoration-style-property>.
fn parse_text_decoration_style(value: &str) -> Option<TextDecorationStyle> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "solid" => Some(TextDecorationStyle::Solid),
        "double" => Some(TextDecorationStyle::Double),
        "dotted" => Some(TextDecorationStyle::Dotted),
        "dashed" => Some(TextDecorationStyle::Dashed),
        "wavy" => Some(TextDecorationStyle::Wavy),
        _ => None,
    }
}

/// Parses CSS `text-decoration-thickness`.
///
/// CSS Text Decoration Level 4 defines `auto`, `from-font`, and
/// `<length-percentage>` thickness values:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-width-property>.
fn parse_text_decoration_thickness(value: &str, font_size: f32) -> Option<TextDecorationThickness> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(TextDecorationThickness::Auto),
        "from-font" => Some(TextDecorationThickness::FromFont),
        "thin" | "medium" | "thick" => parse_border_width_with_font_size(value, font_size)
            .map(ComputedLengthPercentage::from_length)
            .map(TextDecorationThickness::LengthPercentage),
        _ => parse_computed_length_percentage(value, font_size)
            .map(TextDecorationThickness::LengthPercentage),
    }
}

fn parse_text_decoration_inset(value: &str, font_size: f32) -> Option<TextDecorationInset> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(TextDecorationInset::Auto);
    }
    let parts = split_css_component_values(value);
    match parts.as_slice() {
        [single] => {
            let length = parse_computed_length_percentage(single, font_size)?;
            Some(TextDecorationInset::Lengths {
                start: length,
                end: length,
            })
        }
        [start, end] => Some(TextDecorationInset::Lengths {
            start: parse_computed_length_percentage(start, font_size)?,
            end: parse_computed_length_percentage(end, font_size)?,
        }),
        _ => None,
    }
}

fn parse_text_decoration_skip(
    value: &str,
) -> Option<(
    TextDecorationSkipInk,
    TextDecorationSkipSelf,
    TextDecorationSkipBox,
    TextDecorationSkipSpaces,
)> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some((
            TextDecorationSkipInk::Auto,
            TextDecorationSkipSelf::Auto,
            TextDecorationSkipBox::None,
            TextDecorationSkipSpaces::START_END,
        )),
        "none" => Some((
            TextDecorationSkipInk::None,
            TextDecorationSkipSelf::NoSkip,
            TextDecorationSkipBox::None,
            TextDecorationSkipSpaces::NONE,
        )),
        _ => None,
    }
}

fn parse_text_decoration_skip_ink(value: &str) -> Option<TextDecorationSkipInk> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(TextDecorationSkipInk::Auto),
        "all" => Some(TextDecorationSkipInk::All),
        "none" => Some(TextDecorationSkipInk::None),
        _ => None,
    }
}

fn parse_text_decoration_skip_self(value: &str) -> Option<TextDecorationSkipSelf> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 {
        return match parts[0].to_ascii_lowercase().as_str() {
            "auto" => Some(TextDecorationSkipSelf::Auto),
            "skip-all" => Some(TextDecorationSkipSelf::SkipAll),
            "no-skip" => Some(TextDecorationSkipSelf::NoSkip),
            "skip-underline" => Some(TextDecorationSkipSelf::Lines {
                underline: true,
                overline: false,
                line_through: false,
            }),
            "skip-overline" => Some(TextDecorationSkipSelf::Lines {
                underline: false,
                overline: true,
                line_through: false,
            }),
            "skip-line-through" => Some(TextDecorationSkipSelf::Lines {
                underline: false,
                overline: false,
                line_through: true,
            }),
            _ => None,
        };
    }
    let mut underline = false;
    let mut overline = false;
    let mut line_through = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "skip-underline" if !underline => underline = true,
            "skip-overline" if !overline => overline = true,
            "skip-line-through" if !line_through => line_through = true,
            _ => return None,
        }
    }
    Some(TextDecorationSkipSelf::Lines {
        underline,
        overline,
        line_through,
    })
}

fn parse_text_decoration_skip_box(value: &str) -> Option<TextDecorationSkipBox> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "none" => Some(TextDecorationSkipBox::None),
        "all" => Some(TextDecorationSkipBox::All),
        _ => None,
    }
}

/// Parse CSS `text-decoration-skip-spaces`.
///
/// CSS Text Decoration Level 4 defines the grammar as `none | all |
/// [ start || end ]`, with initial value `start end`:
/// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-skip-spaces-property>.
fn parse_text_decoration_skip_spaces(value: &str) -> Option<TextDecorationSkipSpaces> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }

    if parts.len() == 1 {
        return match parts[0].to_ascii_lowercase().as_str() {
            "none" => Some(TextDecorationSkipSpaces::NONE),
            "all" => Some(TextDecorationSkipSpaces::ALL),
            "start" => Some(TextDecorationSkipSpaces {
                start: true,
                end: false,
                all: false,
            }),
            "end" => Some(TextDecorationSkipSpaces {
                start: false,
                end: true,
                all: false,
            }),
            _ => None,
        };
    }

    let mut start = false;
    let mut end = false;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "start" if !start => start = true,
            "end" if !end => end = true,
            _ => return None,
        }
    }
    if start || end {
        Some(TextDecorationSkipSpaces {
            start,
            end,
            all: false,
        })
    } else {
        None
    }
}

fn parse_text_underline_offset(value: &str, font_size: f32) -> Option<TextUnderlineOffset> {
    if trim_css_value(value).eq_ignore_ascii_case("auto") {
        return Some(TextUnderlineOffset::Auto);
    }
    parse_computed_length_percentage(value, font_size).map(TextUnderlineOffset::LengthPercentage)
}

fn parse_text_underline_position(value: &str) -> Option<TextUnderlinePosition> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    let mut position = TextUnderlinePosition {
        auto: false,
        under: false,
        left: false,
        right: false,
    };
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "auto" if !position.auto && !position.under => position.auto = true,
            "under" if !position.under && !position.auto => position.under = true,
            "left" if !position.left && !position.right => position.left = true,
            "right" if !position.right && !position.left => position.right = true,
            _ => return None,
        }
    }
    Some(position)
}

/// Parses CSS `text-emphasis-style`.
///
/// CSS Text Decoration defines `none`, filled/open shape keywords, and string
/// marks. A missing fill defaults to `filled`; a missing shape is resolved
/// later from the used writing mode:
/// <https://www.w3.org/TR/css-text-decor-3/#text-emphasis-style-property>.
fn parse_text_emphasis_style(value: &str) -> Option<TextEmphasisStyle> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(TextEmphasisStyle::None);
    }
    if let Some((mark, tail)) = parse_css_string_token(value)
        && tail.trim().is_empty()
    {
        return Some(TextEmphasisStyle::String(mark));
    }

    let mut fill = None;
    let mut shape = None;
    for part in split_css_component_values(value) {
        match part.to_ascii_lowercase().as_str() {
            "filled" if fill.is_none() => fill = Some(TextEmphasisFill::Filled),
            "open" if fill.is_none() => fill = Some(TextEmphasisFill::Open),
            "dot" if shape.is_none() => shape = Some(TextEmphasisShape::Dot),
            "circle" if shape.is_none() => shape = Some(TextEmphasisShape::Circle),
            "double-circle" if shape.is_none() => shape = Some(TextEmphasisShape::DoubleCircle),
            "triangle" if shape.is_none() => shape = Some(TextEmphasisShape::Triangle),
            "sesame" if shape.is_none() => shape = Some(TextEmphasisShape::Sesame),
            _ => return None,
        }
    }
    if fill.is_none() && shape.is_none() {
        return None;
    }
    Some(TextEmphasisStyle::Keywords {
        fill: fill.unwrap_or(TextEmphasisFill::Filled),
        shape,
    })
}

fn parse_text_emphasis(value: &str) -> Option<(TextEmphasisStyle, Option<Color>)> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    for split_index in 0..=parts.len() {
        let first = parts[..split_index].join(" ");
        let second = parts[split_index..].join(" ");
        if !first.is_empty()
            && let Some(style) = parse_text_emphasis_style(&first)
        {
            if second.is_empty() {
                return Some((style, None));
            }
            if let Some(color) = parse_color(&second) {
                return Some((style, Some(color)));
            }
        }
        if !second.is_empty()
            && let Some(style) = parse_text_emphasis_style(&second)
        {
            if first.is_empty() {
                return Some((style, None));
            }
            if let Some(color) = parse_color(&first) {
                return Some((style, Some(color)));
            }
        }
    }
    None
}

fn parse_text_emphasis_position(value: &str) -> Option<TextEmphasisPosition> {
    let parts = split_css_component_values(value);
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    let mut over = None;
    let mut right = None;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "over" if over.is_none() => over = Some(true),
            "under" if over.is_none() => over = Some(false),
            "right" if right.is_none() => right = Some(true),
            "left" if right.is_none() => right = Some(false),
            _ => return None,
        }
    }
    Some(TextEmphasisPosition {
        over: over.unwrap_or(true),
        right: right.unwrap_or(true),
    })
}

fn parse_text_emphasis_skip(value: &str) -> Option<TextEmphasisSkip> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    let mut skip = TextEmphasisSkip {
        spaces: false,
        punctuation: false,
        symbols: false,
        narrow: false,
    };
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "spaces" if !skip.spaces => skip.spaces = true,
            "punctuation" if !skip.punctuation => skip.punctuation = true,
            "symbols" if !skip.symbols => skip.symbols = true,
            "narrow" if !skip.narrow => skip.narrow = true,
            _ => return None,
        }
    }
    Some(skip)
}

fn parse_text_shadow(value: &str, font_size: f32) -> Option<Vec<TextShadow>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut shadows = Vec::new();
    for layer in split_css_args(value, ',') {
        shadows.push(parse_text_shadow_layer(layer, font_size)?);
    }
    (!shadows.is_empty()).then_some(shadows)
}

fn parse_text_shadow_layer(value: &str, font_size: f32) -> Option<TextShadow> {
    let mut color = None;
    let mut inset = false;
    let mut lengths = Vec::new();
    for part in split_css_component_values(value) {
        if part.eq_ignore_ascii_case("inset") && !inset {
            inset = true;
            continue;
        }
        if part.eq_ignore_ascii_case("currentcolor") && color.is_none() {
            color = Some(TextShadowColor::CurrentColor);
            continue;
        }
        if color.is_none()
            && let Some(parsed_color) = parse_color(part)
        {
            color = Some(TextShadowColor::Color(parsed_color));
            continue;
        }
        if let Some(length) = parse_shadow_length(part, font_size) {
            lengths.push(length);
            continue;
        }
        return None;
    }
    if !(2..=4).contains(&lengths.len()) {
        return None;
    }
    let spread = lengths
        .get(3)
        .copied()
        .unwrap_or(ComputedLengthPercentage::ZERO);
    if length_percentage_is_definitely_negative(spread) {
        return None;
    }
    Some(TextShadow {
        color: color.unwrap_or(TextShadowColor::CurrentColor),
        offset_x: lengths[0],
        offset_y: lengths[1],
        blur_radius: lengths
            .get(2)
            .copied()
            .filter(|length| !length_percentage_is_definitely_negative(*length))
            .unwrap_or(ComputedLengthPercentage::ZERO),
        spread,
        inset,
    })
}

fn parse_box_shadow(value: &str, font_size: f32) -> Option<Vec<BoxShadow>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut shadows = Vec::new();
    for layer in split_css_args(value, ',') {
        shadows.push(parse_box_shadow_layer(layer, font_size)?);
    }
    (!shadows.is_empty()).then_some(shadows)
}

fn parse_box_shadow_layer(value: &str, font_size: f32) -> Option<BoxShadow> {
    let mut color = None;
    let mut inset = false;
    let mut lengths = Vec::new();
    for part in split_css_component_values(value) {
        if part.eq_ignore_ascii_case("inset") && !inset {
            inset = true;
            continue;
        }
        if part.eq_ignore_ascii_case("currentcolor") && color.is_none() {
            color = Some(BoxShadowColor::CurrentColor);
            continue;
        }
        if color.is_none()
            && let Some(parsed_color) = parse_color(part)
        {
            color = Some(BoxShadowColor::Color(parsed_color));
            continue;
        }
        if let Some(length) = parse_shadow_length(part, font_size) {
            lengths.push(length);
            continue;
        }
        return None;
    }
    if !(2..=4).contains(&lengths.len())
        || lengths
            .get(2)
            .is_some_and(|blur| length_percentage_is_definitely_negative(*blur))
    {
        return None;
    }
    Some(BoxShadow {
        color: color.unwrap_or(BoxShadowColor::CurrentColor),
        offset_x: lengths[0],
        offset_y: lengths[1],
        blur_radius: lengths
            .get(2)
            .copied()
            .unwrap_or(ComputedLengthPercentage::ZERO),
        spread: lengths
            .get(3)
            .copied()
            .unwrap_or(ComputedLengthPercentage::ZERO),
        inset,
    })
}

fn parse_shadow_length(value: &str, font_size: f32) -> Option<ComputedLengthPercentage> {
    let length = parse_computed_length_percentage(value, font_size)?;
    (length.percent == 0.0).then_some(length)
}

fn length_percentage_is_definitely_negative(value: ComputedLengthPercentage) -> bool {
    let components = [
        value.length,
        value.percent,
        value.ch,
        value.vw,
        value.vh,
        value.vmin,
        value.vmax,
        value.vi,
        value.vb,
    ];
    components.iter().any(|component| *component < 0.0)
        && components.iter().all(|component| *component <= 0.0)
}

/// Parses the CSS `text-decoration` shorthand.
///
/// The shorthand accepts line, style, color, and thickness components in any
/// order. Omitted components reset to their initial values:
/// <https://www.w3.org/TR/css-text-decor-4/#text-decoration-property>.
fn parse_text_decoration_shorthand(
    value: &str,
    current_style: &ComputedStyle,
) -> Option<TextDecoration> {
    let mut decoration = ComputedStyle::initial().text_decoration;
    let mut line = TextDecorationLineParts {
        underline: false,
        overline: false,
        line_through: false,
        blink: false,
        spelling_error: false,
        grammar_error: false,
    };
    let mut saw_style = false;
    let mut saw_color = false;
    let mut saw_thickness = false;

    let parts = split_css_component_values(value);
    for part in &parts {
        if let Some(parsed_line) = parse_text_decoration_line(part) {
            let parsed_has_line = parsed_line.underline
                || parsed_line.overline
                || parsed_line.line_through
                || parsed_line.blink
                || parsed_line.spelling_error
                || parsed_line.grammar_error;
            if !parsed_has_line && parts.len() > 1 {
                return None;
            }
            if (parsed_line.underline && line.underline)
                || (parsed_line.overline && line.overline)
                || (parsed_line.line_through && line.line_through)
                || (parsed_line.blink && line.blink)
                || (parsed_line.spelling_error && line.spelling_error)
                || (parsed_line.grammar_error && line.grammar_error)
            {
                return None;
            }
            line.underline |= parsed_line.underline;
            line.overline |= parsed_line.overline;
            line.line_through |= parsed_line.line_through;
            line.blink |= parsed_line.blink;
            line.spelling_error |= parsed_line.spelling_error;
            line.grammar_error |= parsed_line.grammar_error;
            continue;
        }
        if !saw_style && let Some(style) = parse_text_decoration_style(part) {
            decoration.style = style;
            saw_style = true;
            continue;
        }
        if !saw_thickness
            && let Some(thickness) = parse_text_decoration_thickness(part, current_style.font_size)
        {
            decoration.thickness = thickness;
            saw_thickness = true;
            continue;
        }
        if !saw_color && let Some(color) = parse_color(part) {
            decoration.color = Some(color);
            saw_color = true;
            continue;
        }
        return None;
    }
    apply_text_decoration_line(&mut decoration, line);
    Some(decoration)
}

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
