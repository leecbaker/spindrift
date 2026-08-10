use super::*;

/// CSS Backgrounds and Borders defines `border` as setting the width, style,
/// and color of all four physical border sides, with omitted components
/// resetting to their initial values:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-shorthands>.
pub(in crate::css) fn expand_border_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    Some(
        ["border-top", "border-right", "border-bottom", "border-left"]
            .into_iter()
            .map(|side| expand_border_side_shorthand(side, value))
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect(),
    )
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
        if color.is_none() && parse_border_color(part).is_some() {
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
    Some(
        logical_sides
            .into_iter()
            .map(|logical_side| {
                let side = physical_border_side_shorthand(logical_border_side(
                    logical_side,
                    direction,
                    writing_mode,
                )?);
                expand_border_side_shorthand(side, value)
            })
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect(),
    )
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

/// Expand one physical side radius shorthand into its two corner longhands.
///
/// CSS Borders and Box Decorations Level 4 defines `border-*-radius` as a
/// pair of adjacent corner radii. Its optional slash separates the two
/// adjacent corner values, unlike the horizontal/vertical component lists of
/// `border-radius`:
/// <https://drafts.csswg.org/css-borders-4/#border-radius-sides>.
pub(in crate::css) fn expand_border_side_radius_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let [first, second] = match name {
        "border-top-radius" => ["border-top-left-radius", "border-top-right-radius"],
        "border-right-radius" => ["border-top-right-radius", "border-bottom-right-radius"],
        "border-bottom-radius" => ["border-bottom-left-radius", "border-bottom-right-radius"],
        "border-left-radius" => ["border-top-left-radius", "border-bottom-left-radius"],
        _ => return None,
    };
    expand_two_corner_radius_shorthand(value, first, second)
}

/// Expand a logical side radius shorthand after mapping its adjacent logical
/// corners through the element's writing mode and direction.
pub(in crate::css) fn expand_logical_border_side_radius_shorthand(
    name: &str,
    value: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    let [first_logical, second_logical] = match name {
        "border-block-start-radius" => ["border-start-start-radius", "border-start-end-radius"],
        "border-block-end-radius" => ["border-end-start-radius", "border-end-end-radius"],
        "border-inline-start-radius" => ["border-start-start-radius", "border-end-start-radius"],
        "border-inline-end-radius" => ["border-start-end-radius", "border-end-end-radius"],
        _ => return None,
    };
    let first = logical_corner_radius_longhand(first_logical, direction, writing_mode)?;
    let second = logical_corner_radius_longhand(second_logical, direction, writing_mode)?;
    expand_two_corner_radius_shorthand(value, first, second)
}

/// Expand the two adjacent corner radii of one side shorthand.
///
/// Unlike `border-radius`, the optional top-level slash separates the two
/// adjacent `<length-percentage>{1,2}` corner values. It does not separate
/// horizontal and vertical component lists:
/// <https://drafts.csswg.org/css-borders-4/#border-radius-sides>.
fn expand_two_corner_radius_shorthand(
    value: &str,
    first: &'static str,
    second: &'static str,
) -> Option<Vec<(&'static str, String)>> {
    let (first_value, second_value) = split_top_level_once(value, '/').unwrap_or((value, value));
    let first_value = trim_css_value(first_value);
    let second_value = trim_css_value(second_value);
    // Validate the exact corner-radius grammar before emitting physical
    // longhands. The cascade subsequently parses the same typed values.
    let first_components = split_css_component_values(first_value);
    let second_components = split_css_component_values(second_value);
    if !(1..=2).contains(&first_components.len()) || !(1..=2).contains(&second_components.len()) {
        return None;
    }
    Some(vec![
        (first, first_value.to_string()),
        (second, second_value.to_string()),
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
