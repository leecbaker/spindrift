use super::shorthands::split_css_top_level_slashes;
use super::*;

/// Expand `corner-shape` into physical per-corner shape longhands.
///
/// CSS Borders and Box Decorations Level 4 uses the same four-corner expansion
/// order as `border-radius`:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape-shorthand>.
pub(in crate::css) fn expand_corner_shape_shorthand(
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let parts = split_css_component_values(value)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.iter().any(|part| parse_corner_shape(part).is_none()) {
        return None;
    }
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
pub(in crate::css) fn expand_corner_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
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
    let declarations = corner_names
        .into_iter()
        .zip(expanded)
        .flat_map(|((radius_name, shape_name), (radius, shape))| {
            [(radius_name, radius.clone()), (shape_name, shape.clone())]
        })
        .collect::<Vec<_>>();
    Some(declarations)
}

pub(in crate::css) fn split_corner_radius_shape_component(value: &str) -> Option<(String, String)> {
    let mut shape = None;
    let mut radius_parts = Vec::new();
    for part in split_css_component_values(value) {
        if shape.is_none() && parse_corner_shape(part).is_some() {
            shape = Some(part.to_string());
        } else {
            radius_parts.push(part.to_string());
        }
    }
    (!radius_parts.is_empty()).then_some((radius_parts.join(" "), shape.unwrap_or("round".into())))
}
/// Parses one CSS margin side into its typed computed value.
///
/// CSS Box Model defines margin side properties, including `auto`, and CSS
/// Values defines length-percentage values:
/// <https://www.w3.org/TR/CSS22/box.html#margin-properties> and
/// <https://www.w3.org/TR/css-values-4/#mixed-percentages>.
pub(in crate::css) fn set_margin_side(
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
pub(in crate::css) fn apply_logical_margin_axis(
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
pub(in crate::css) fn apply_logical_margin_side(
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

pub(in crate::css) fn set_margin_box_side(
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

pub(in crate::css) fn set_ua_margin_em_side(
    style: &mut ComputedStyle,
    side: BoxSide,
    factor: Option<f32>,
) {
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
pub(in crate::css) fn set_computed_length_percentage(
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
pub(in crate::css) fn apply_logical_padding_axis(
    value: &str,
    style: &mut ComputedStyle,
    name: &str,
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
    apply_logical_padding_side(start_value, style, start);
    apply_logical_padding_side(end_value, style, end);
}

/// Applies one logical padding longhand to its resolved physical side.
///
/// CSS Logical Properties defines flow-relative padding longhands as aliases
/// for physical padding sides:
/// <https://www.w3.org/TR/css-logical-1/#padding-properties>.
pub(in crate::css) fn apply_logical_padding_side(
    value: &str,
    style: &mut ComputedStyle,
    name: &str,
) {
    let Some(side) = logical_box_side(name, style.direction, style.writing_mode) else {
        return;
    };
    set_computed_length_percentage(value, style.font_size, |typed| {
        set_padding_box_side(style, side, typed);
    });
}

pub(in crate::css) fn set_padding_box_side(
    style: &mut ComputedStyle,
    side: BoxSide,
    typed: ComputedLengthPercentage,
) {
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
pub(in crate::css) fn legacy_edge_lengths(
    values: CssEdges<ComputedLengthPercentage>,
) -> Option<Edges> {
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
pub(in crate::css) fn legacy_margin_edges(
    values: CssEdges<ComputedLengthPercentageOrAuto>,
) -> Edges {
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
pub(in crate::css) fn parse_margin_em_edges(value: &str) -> OptionalEdges<f32> {
    let Some(parts) = try_split_css_component_values(value) else {
        return OptionalEdges::NONE;
    };
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

pub(in crate::css) fn parse_em_length_factor(value: &str) -> Option<f32> {
    trim_css_value(value)
        .to_ascii_lowercase()
        .strip_suffix("em")
        .and_then(|factor| factor.trim().parse::<f32>().ok())
}
