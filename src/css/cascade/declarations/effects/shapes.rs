use super::super::*;

/// Parse the `contain` keywords that affect paint isolation.
///
/// CSS Containment maps `strict` to size/layout/style/paint and `content` to
/// layout/style/paint containment:
/// <https://www.w3.org/TR/css-contain-2/#contain-property>.
pub(in crate::css) fn parse_contain(value: &str) -> Option<Contain> {
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
            inline_size: false,
        });
    }
    if value == "content" {
        return Some(Contain {
            size: false,
            layout: true,
            style: true,
            paint: true,
            inline_size: false,
        });
    }

    let mut contain = Contain::NONE;
    for token in try_split_css_component_values(&value)? {
        match token {
            "size" if !contain.size => contain.size = true,
            "layout" if !contain.layout => contain.layout = true,
            "style" if !contain.style => contain.style = true,
            "inline-size" if !contain.inline_size => contain.inline_size = true,
            "paint" if !contain.paint => contain.paint = true,
            _ => return None,
        }
    }
    (contain.size || contain.layout || contain.style || contain.inline_size || contain.paint)
        .then_some(contain)
}

/// Parse `clip-path` basic-shape values and stacking-context triggers.
///
/// CSS Masking defines basic shapes as clipping geometry and requires each
/// non-`none` value to establish a stacking context:
/// <https://www.w3.org/TR/css-masking-1/#the-clip-path>.
pub(in crate::css) fn parse_clip_path(value: &str, font_size: f32) -> Option<ClipPath> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(ClipPath::None);
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("url(") {
        Some(ClipPath::Url)
    } else if let Some(points) = parse_clip_path_polygon(value, font_size) {
        Some(ClipPath::Polygon(points))
    } else if let Some([top, right, bottom, left]) = parse_clip_path_inset(value, font_size) {
        Some(ClipPath::Inset {
            top,
            right,
            bottom,
            left,
        })
    } else if lower.ends_with(')') {
        Some(ClipPath::Shape)
    } else {
        None
    }
}

/// Parse the CSS 2 legacy `clip` property used by absolutely positioned
/// elements. CSS 2 requires comma separation, but permits user agents to
/// accept the historical whitespace-only form; accept either complete form
/// while rejecting mixed separators.
/// <https://drafts.csswg.org/css2/#propdef-clip>
pub(in crate::css) fn parse_legacy_clip(value: &str, font_size: f32) -> Option<LegacyClip> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(LegacyClip::Auto);
    }
    let calls = css_function_list(value)?;
    let [(name, body)] = calls.as_slice() else {
        return None;
    };
    if !name.eq_ignore_ascii_case("rect") {
        return None;
    }
    let comma_components = split_css_function_arguments(body)?;
    let components = if comma_components.len() == 1 {
        try_split_css_component_values(body)?
    } else if comma_components
        .iter()
        .all(|component| !component.is_empty())
    {
        comma_components
    } else {
        return None;
    };
    let [top, right, bottom, left] = components.as_slice() else {
        return None;
    };
    Some(LegacyClip::Rect([
        parse_legacy_clip_edge(top, font_size)?,
        parse_legacy_clip_edge(right, font_size)?,
        parse_legacy_clip_edge(bottom, font_size)?,
        parse_legacy_clip_edge(left, font_size)?,
    ]))
}

fn parse_legacy_clip_edge(value: &str, font_size: f32) -> Option<LegacyClipEdge> {
    if value.eq_ignore_ascii_case("auto") {
        return Some(LegacyClipEdge::Auto);
    }
    let length = parse_computed_length_percentage(value, font_size)?;
    (!length.contains_percentage()).then_some(LegacyClipEdge::Length(length))
}

fn parse_clip_path_inset(value: &str, font_size: f32) -> Option<[ComputedLengthPercentage; 4]> {
    let calls = css_function_list(value)?;
    let [(name, body)] = calls.as_slice() else {
        return None;
    };
    if !name.eq_ignore_ascii_case("inset") {
        return None;
    }
    let components = try_split_css_component_values(body)?;
    let values = components
        .iter()
        .take_while(|component| !component.eq_ignore_ascii_case("round"))
        .map(|component| parse_computed_length_percentage(component, font_size))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some([all.clone(), all.clone(), all.clone(), all.clone()]),
        [vertical, horizontal] => Some([
            vertical.clone(),
            horizontal.clone(),
            vertical.clone(),
            horizontal.clone(),
        ]),
        [top, horizontal, bottom] => Some([
            top.clone(),
            horizontal.clone(),
            bottom.clone(),
            horizontal.clone(),
        ]),
        [top, right, bottom, left] => {
            Some([top.clone(), right.clone(), bottom.clone(), left.clone()])
        }
        _ => None,
    }
}

fn parse_clip_path_polygon(value: &str, font_size: f32) -> Option<Vec<ClipPathPolygonPoint>> {
    let value = trim_css_value(value);
    let prefix = "polygon(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let arguments = split_css_function_arguments(&value[prefix.len()..value.len() - 1])?;
    let points = arguments
        .into_iter()
        .map(|argument| {
            let components = split_css_component_values(argument);
            let [x, y] = components.as_slice() else {
                return None;
            };
            Some(ClipPathPolygonPoint {
                x: parse_computed_length_percentage(x, font_size)?,
                y: parse_computed_length_percentage(y, font_size)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (points.len() >= 3).then_some(points)
}

/// Split a CSS function's comma-delimited top-level arguments without
/// confusing nested math functions for polygon vertices.
fn split_css_function_arguments(value: &str) -> Option<Vec<&str>> {
    let mut arguments = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                arguments.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    (depth == 0).then(|| {
        arguments.push(value[start..].trim());
        arguments
    })
}

/// Parse the supported CSS Borders 4 `border-shape` basic shapes.
///
/// The representation intentionally preserves geometry-box selection and
/// computed length percentages for used-value resolution at paint time. Other
/// basic shapes remain unsupported rather than being silently approximated:
/// <https://drafts.csswg.org/css-borders-4/#border-shape>.
pub(in crate::css) fn parse_border_shape(value: &str, font_size: f32) -> Option<BorderShape> {
    let components = split_css_component_values(trim_css_value(value));
    if components.len() > 4 || components.is_empty() {
        return None;
    }
    let mut shapes = Vec::with_capacity(2);
    let mut geometry_box_follows_shape = false;
    for component in components {
        if let Some(shape) = parse_border_shape_basic_shape(component, font_size) {
            if shapes.len() == 2 {
                return None;
            }
            shapes.push(shape);
            geometry_box_follows_shape = true;
            continue;
        }
        if !geometry_box_follows_shape {
            return None;
        }
        shapes
            .last_mut()?
            .set_geometry_box(parse_border_shape_geometry_box(component)?)?;
        geometry_box_follows_shape = false;
    }
    match shapes.as_mut_slice() {
        [shape] => Some(shape.clone()),
        [outer, inner] => {
            // Two shapes default to the border and padding boxes. A single
            // shape remains on its half-border-box default.
            outer.replace_half_border_box(BorderShapeGeometryBox::Border);
            inner.replace_half_border_box(BorderShapeGeometryBox::Padding);
            Some(BorderShape::Pair {
                outer: Box::new(outer.clone()),
                inner: Box::new(inner.clone()),
            })
        }
        _ => None,
    }
}

/// Parse the CSS Shapes Level 1 non-image `shape-outside` subset.
///
/// Image sources deliberately remain unsupported for now: accepting one here
/// would incorrectly leave the rectangular float area in effect.
/// <https://drafts.csswg.org/css-shapes-1/#shape-outside-property>
/// Parse CSS Shapes Level 1 `shape-outside`, retaining declaration URL bases
/// for image-backed contours until layout resolves the resource.
pub(in crate::css) fn parse_shape_outside_with_urls(
    value: &str,
    font_size: f32,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<ShapeOutside> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(ShapeOutside::None);
    }
    if let Some(image) = parse_background_image(value, base_url, root_url) {
        return Some(ShapeOutside::Image(image));
    }
    let components = split_css_component_values(value);
    let mut reference_box = None;
    let mut basic_shape = None;
    for component in components {
        if let Some(shape_box) = parse_shape_box(component) {
            if reference_box.replace(shape_box).is_some() {
                return None;
            }
            continue;
        }
        if basic_shape.is_some() {
            return None;
        }
        basic_shape = parse_shape_outside_basic_shape(component, font_size);
        basic_shape.as_ref()?;
    }
    match (basic_shape, reference_box) {
        (None, Some(shape_box)) => Some(ShapeOutside::Box(shape_box)),
        (Some(shape), reference_box) => Some(ShapeOutside::Basic {
            shape,
            reference_box: reference_box.unwrap_or(ShapeBox::Margin),
        }),
        (None, None) => None,
    }
}

fn parse_shape_box(value: &str) -> Option<ShapeBox> {
    match value.to_ascii_lowercase().as_str() {
        "margin-box" => Some(ShapeBox::Margin),
        "border-box" => Some(ShapeBox::Border),
        "padding-box" => Some(ShapeBox::Padding),
        "content-box" => Some(ShapeBox::Content),
        _ => None,
    }
}

fn parse_shape_outside_basic_shape(value: &str, font_size: f32) -> Option<BasicShape> {
    if let Some(inset) = parse_shape_inset(value, font_size) {
        return Some(BasicShape::Inset(inset));
    }
    if let Some(circle) = parse_shape_circle(value, font_size) {
        return Some(BasicShape::Circle(circle));
    }
    if let Some(ellipse) = parse_shape_ellipse(value, font_size) {
        return Some(BasicShape::Ellipse(ellipse));
    }
    parse_shape_polygon(value, font_size).map(BasicShape::Polygon)
}

fn parse_shape_polygon(value: &str, font_size: f32) -> Option<ShapePolygon> {
    let value = trim_css_value(value);
    let prefix = "polygon(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let mut arguments = split_css_function_arguments(&value[prefix.len()..value.len() - 1])?;
    let fill_rule = match arguments
        .first()
        .map(|argument| argument.to_ascii_lowercase())
    {
        Some(rule) if rule == "evenodd" => {
            arguments.remove(0);
            ShapeFillRule::EvenOdd
        }
        Some(rule) if rule == "nonzero" => {
            arguments.remove(0);
            ShapeFillRule::NonZero
        }
        _ => ShapeFillRule::NonZero,
    };
    let vertices = arguments
        .into_iter()
        .map(|argument| {
            let components = split_css_component_values(argument);
            let [x, y] = components.as_slice() else {
                return None;
            };
            Some(ShapePolygonPoint {
                x: parse_computed_length_percentage(x, font_size)?,
                y: parse_computed_length_percentage(y, font_size)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (vertices.len() >= 3).then_some(ShapePolygon {
        fill_rule,
        vertices,
    })
}

fn parse_shape_inset(value: &str, font_size: f32) -> Option<ShapeInset> {
    let value = trim_css_value(value);
    let prefix = "inset(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let values = split_css_component_values(&value[prefix.len()..value.len() - 1]);
    let round_at = values
        .iter()
        .position(|value| value.eq_ignore_ascii_case("round"));
    let (insets, radii) = match round_at {
        Some(index) => (
            &values[..index],
            parse_border_radius(&values[index + 1..].join(" "), font_size)?,
        ),
        None => (values.as_slice(), BorderRadius::ZERO),
    };
    if insets.is_empty() || insets.len() > 4 {
        return None;
    }
    let values = insets
        .iter()
        .map(|value| parse_computed_length_percentage(value, font_size))
        .collect::<Option<Vec<_>>>()?;
    let (top, right, bottom, left) = match values.as_slice() {
        [all] => (all.clone(), all.clone(), all.clone(), all.clone()),
        [vertical, horizontal] => (
            vertical.clone(),
            horizontal.clone(),
            vertical.clone(),
            horizontal.clone(),
        ),
        [top, horizontal, bottom] => (
            top.clone(),
            horizontal.clone(),
            bottom.clone(),
            horizontal.clone(),
        ),
        [top, right, bottom, left] => (top.clone(), right.clone(), bottom.clone(), left.clone()),
        _ => return None,
    };
    Some(ShapeInset {
        top,
        right,
        bottom,
        left,
        radii,
    })
}

fn parse_shape_circle(value: &str, font_size: f32) -> Option<ShapeCircle> {
    let value = trim_css_value(value);
    let prefix = "circle(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let values = split_css_component_values(&value[prefix.len()..value.len() - 1]);
    let at = values
        .iter()
        .position(|value| value.eq_ignore_ascii_case("at"));
    let (radius, position) = match at {
        Some(at) => (
            parse_shape_circle_radius(values.get(..at)?.first()?, font_size).filter(|_| at == 1)?,
            parse_shape_position(&values[at + 1..], font_size)?,
        ),
        None if values.is_empty() => (ShapeCircleRadius::ClosestSide, ShapePosition::center()),
        None if values.len() == 1 => (
            parse_shape_circle_radius(values[0], font_size)?,
            ShapePosition::center(),
        ),
        _ => return None,
    };
    Some(ShapeCircle { radius, position })
}

fn parse_shape_circle_radius(value: &str, font_size: f32) -> Option<ShapeCircleRadius> {
    match value.to_ascii_lowercase().as_str() {
        "closest-side" => Some(ShapeCircleRadius::ClosestSide),
        "farthest-side" => Some(ShapeCircleRadius::FarthestSide),
        "closest-corner" => Some(ShapeCircleRadius::ClosestCorner),
        "farthest-corner" => Some(ShapeCircleRadius::FarthestCorner),
        _ => parse_computed_length_percentage(value, font_size)
            .filter(|value| !value.is_definitely_negative())
            .map(ShapeCircleRadius::LengthPercentage),
    }
}

fn parse_shape_ellipse(value: &str, font_size: f32) -> Option<ShapeEllipse> {
    let value = trim_css_value(value);
    let prefix = "ellipse(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let values = split_css_component_values(&value[prefix.len()..value.len() - 1]);
    let at = values
        .iter()
        .position(|value| value.eq_ignore_ascii_case("at"));
    let (radii, position) = match at {
        Some(at) => (
            parse_shape_ellipse_radii(&values[..at], font_size)?,
            parse_shape_position(&values[at + 1..], font_size)?,
        ),
        None => (
            parse_shape_ellipse_radii(&values, font_size)?,
            ShapePosition::center(),
        ),
    };
    Some(ShapeEllipse {
        horizontal_radius: radii.0,
        vertical_radius: radii.1,
        position,
    })
}

fn parse_shape_ellipse_radii(
    values: &[&str],
    font_size: f32,
) -> Option<(ShapeEllipseRadius, ShapeEllipseRadius)> {
    if values.is_empty() {
        return Some((
            ShapeEllipseRadius::ClosestSide,
            ShapeEllipseRadius::ClosestSide,
        ));
    }
    let [horizontal, vertical] = values else {
        return None;
    };
    Some((
        parse_shape_ellipse_radius(horizontal, font_size)?,
        parse_shape_ellipse_radius(vertical, font_size)?,
    ))
}

fn parse_shape_ellipse_radius(value: &str, font_size: f32) -> Option<ShapeEllipseRadius> {
    match value.to_ascii_lowercase().as_str() {
        "closest-side" => Some(ShapeEllipseRadius::ClosestSide),
        "farthest-side" => Some(ShapeEllipseRadius::FarthestSide),
        _ => parse_computed_length_percentage(value, font_size)
            .filter(|value| !value.is_definitely_negative())
            .map(ShapeEllipseRadius::LengthPercentage),
    }
}

fn parse_shape_position(values: &[&str], font_size: f32) -> Option<ShapePosition> {
    match values {
        [value] if value.eq_ignore_ascii_case("center") => Some(ShapePosition::center()),
        [value] => {
            let y = parse_shape_position_component(value, false, font_size);
            if matches!(value.to_ascii_lowercase().as_str(), "top" | "bottom") {
                Some(ShapePosition {
                    x: ComputedLengthPercentage::from_percent(0.5),
                    y: y?,
                })
            } else {
                Some(ShapePosition {
                    x: parse_shape_position_component(value, true, font_size)?,
                    y: ComputedLengthPercentage::from_percent(0.5),
                })
            }
        }
        [first, second] => {
            let first_x = parse_shape_position_component(first, true, font_size);
            let first_y = parse_shape_position_component(first, false, font_size);
            let second_x = parse_shape_position_component(second, true, font_size);
            let second_y = parse_shape_position_component(second, false, font_size);
            if let (Some(x), Some(y)) = (first_x, second_y) {
                Some(ShapePosition { x, y })
            } else if let (Some(x), Some(y)) = (second_x, first_y) {
                Some(ShapePosition { x, y })
            } else {
                None
            }
        }
        [first, second, third] => {
            let (offset_horizontal, offset, other_side) = if let Some((offset_horizontal, offset)) =
                parse_shape_position_offset(first, second, font_size)
            {
                (offset_horizontal, offset, *third)
            } else {
                let (offset_horizontal, offset) =
                    parse_shape_position_offset(second, third, font_size)?;
                (offset_horizontal, offset, *first)
            };
            if offset_horizontal {
                Some(ShapePosition {
                    x: offset,
                    y: parse_shape_position_component(other_side, false, font_size)?,
                })
            } else {
                Some(ShapePosition {
                    x: parse_shape_position_component(other_side, true, font_size)?,
                    y: offset,
                })
            }
        }
        [first_side, first_offset, second_side, second_offset] => {
            let (first_horizontal, first) =
                parse_shape_position_offset(first_side, first_offset, font_size)?;
            let (second_horizontal, second) =
                parse_shape_position_offset(second_side, second_offset, font_size)?;
            if first_horizontal == second_horizontal {
                return None;
            }
            Some(if first_horizontal {
                ShapePosition {
                    x: first,
                    y: second,
                }
            } else {
                ShapePosition {
                    x: second,
                    y: first,
                }
            })
        }
        _ => None,
    }
}

fn is_shape_position_side(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "left" | "right" => Some(true),
        "top" | "bottom" => Some(false),
        _ => None,
    }
}

fn parse_shape_position_offset(
    side: &str,
    offset: &str,
    font_size: f32,
) -> Option<(bool, ComputedLengthPercentage)> {
    let horizontal = is_shape_position_side(side)?;
    let parsed_offset = parse_computed_length_percentage(offset, font_size)?;
    match side.to_ascii_lowercase().as_str() {
        "left" | "top" => Some((horizontal, parsed_offset)),
        "right" | "bottom" => Some((
            horizontal,
            parse_computed_length_percentage(&format!("calc(100% - {offset})"), font_size)?,
        )),
        _ => None,
    }
}

fn parse_shape_position_component(
    value: &str,
    horizontal: bool,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    match value.to_ascii_lowercase().as_str() {
        "center" => Some(ComputedLengthPercentage::from_percent(0.5)),
        value if (horizontal && value == "left") || (!horizontal && value == "top") => {
            Some(ComputedLengthPercentage::ZERO)
        }
        value if (horizontal && value == "right") || (!horizontal && value == "bottom") => {
            Some(ComputedLengthPercentage::from_percent(1.0))
        }
        _ => parse_computed_length_percentage(value, font_size),
    }
}

fn parse_border_shape_basic_shape(value: &str, font_size: f32) -> Option<BorderShape> {
    if let Some(circle) = parse_border_shape_circle(value, font_size) {
        return Some(BorderShape::Circle(circle));
    }
    if let Some(ellipse) = parse_border_shape_ellipse(value, font_size) {
        return Some(BorderShape::Ellipse(ellipse));
    }
    if let Some(path) = parse_border_shape_line_path(value, font_size) {
        return Some(BorderShape::Path(path));
    }
    if let Some(inset) = parse_border_shape_inset(value, font_size) {
        return Some(BorderShape::Inset(inset));
    }
    parse_border_shape_polygon(value, font_size).map(BorderShape::Polygon)
}

/// Parse the line-only subset of CSS Shapes 2 `shape()` used by the Borders 4
/// path tests. Arc, curve, and relative commands remain rejected rather than
/// being flattened at computed-value time:
/// <https://drafts.csswg.org/css-shapes-2/#shape-function>.
fn parse_border_shape_line_path(value: &str, font_size: f32) -> Option<BorderShapePath> {
    let value = trim_css_value(value);
    let prefix = "shape(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let segments = split_css_function_arguments(&value[prefix.len()..value.len() - 1])?;
    let (first, rest) = segments.split_first()?;
    let first = split_css_component_values(first);
    let [from, x, y] = first.as_slice() else {
        return None;
    };
    if !from.eq_ignore_ascii_case("from") {
        return None;
    }
    let mut vertices = vec![parse_border_shape_position(&[x, y], font_size)?];
    for segment in rest {
        let values = split_css_component_values(segment);
        if values.len() == 1 && values[0].eq_ignore_ascii_case("close") {
            continue;
        }
        let [line, to, x, y] = values.as_slice() else {
            return None;
        };
        if !line.eq_ignore_ascii_case("line") || !to.eq_ignore_ascii_case("to") {
            return None;
        }
        vertices.push(parse_border_shape_position(&[x, y], font_size)?);
    }
    (vertices.len() >= 3).then_some(BorderShapePath {
        vertices,
        geometry_box: BorderShapeGeometryBox::HalfBorder,
    })
}

/// Parse the non-rounded `inset()` subset used by `border-shape`.
///
/// Border radii are deliberately not accepted until they can retain the full
/// slash-separated radius syntax as typed values.  Rejecting them is more
/// faithful than silently dropping their curvature.
fn parse_border_shape_inset(value: &str, font_size: f32) -> Option<BorderShapeInset> {
    let value = trim_css_value(value);
    let prefix = "inset(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let values = split_css_component_values(&value[prefix.len()..value.len() - 1]);
    let round_at = values
        .iter()
        .position(|component| component.eq_ignore_ascii_case("round"));
    let (values, corner_radius) = match round_at {
        Some(round_at) => {
            let [radius] = &values[round_at + 1..] else {
                return None;
            };
            (
                &values[..round_at],
                Some(
                    parse_computed_length_percentage(radius, font_size)
                        .filter(|radius| !radius.is_definitely_negative())?,
                ),
            )
        }
        None => (values.as_slice(), None),
    };
    if values.is_empty() || values.len() > 4 || values.contains(&"/") {
        return None;
    }
    let values = values
        .iter()
        .map(|value| parse_computed_length_percentage(value, font_size))
        .collect::<Option<Vec<_>>>()?;
    let (top, right, bottom, left) = match values.as_slice() {
        [all] => (all.clone(), all.clone(), all.clone(), all.clone()),
        [vertical, horizontal] => (
            vertical.clone(),
            horizontal.clone(),
            vertical.clone(),
            horizontal.clone(),
        ),
        [top, horizontal, bottom] => (
            top.clone(),
            horizontal.clone(),
            bottom.clone(),
            horizontal.clone(),
        ),
        [top, right, bottom, left] => (top.clone(), right.clone(), bottom.clone(), left.clone()),
        _ => return None,
    };
    Some(BorderShapeInset {
        top,
        right,
        bottom,
        left,
        corner_radius,
        geometry_box: BorderShapeGeometryBox::HalfBorder,
    })
}

/// Parse the default-fill-rule form of CSS Shapes `polygon()`.
///
/// Comma-separated vertices are retained as typed length percentages so the
/// selected geometry box supplies the percentage basis during paint.
fn parse_border_shape_polygon(value: &str, font_size: f32) -> Option<BorderShapePolygon> {
    let value = trim_css_value(value);
    let prefix = "polygon(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let vertices = split_css_function_arguments(&value[prefix.len()..value.len() - 1])?
        .into_iter()
        .map(|vertex| {
            let components = split_css_component_values(vertex);
            parse_border_shape_position(&components, font_size)
        })
        .collect::<Option<Vec<_>>>()?;
    (vertices.len() >= 3).then_some(BorderShapePolygon {
        vertices,
        geometry_box: BorderShapeGeometryBox::HalfBorder,
    })
}

fn parse_border_shape_geometry_box(value: &str) -> Option<BorderShapeGeometryBox> {
    match value.to_ascii_lowercase().as_str() {
        "border-box" => Some(BorderShapeGeometryBox::Border),
        "padding-box" => Some(BorderShapeGeometryBox::Padding),
        "content-box" => Some(BorderShapeGeometryBox::Content),
        "margin-box" => Some(BorderShapeGeometryBox::Margin),
        "half-border-box" => Some(BorderShapeGeometryBox::HalfBorder),
        _ => None,
    }
}

fn parse_border_shape_circle(value: &str, font_size: f32) -> Option<BorderShapeCircle> {
    let value = trim_css_value(value);
    let prefix = "circle(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let components = split_css_component_values(&value[prefix.len()..value.len() - 1]);
    let at = components
        .iter()
        .position(|component| component.eq_ignore_ascii_case("at"));
    let (radius, position) = match at {
        Some(at) => {
            let [radius] = components[..at] else {
                return None;
            };
            (
                parse_border_shape_circle_radius(radius, font_size)?,
                parse_border_shape_position(&components[at + 1..], font_size)?,
            )
        }
        None => match components.as_slice() {
            [] => (
                BorderShapeCircleRadius::ClosestSide,
                BorderShapePosition::center(),
            ),
            [radius] => (
                parse_border_shape_circle_radius(radius, font_size)?,
                BorderShapePosition::center(),
            ),
            _ => return None,
        },
    };
    Some(BorderShapeCircle {
        radius,
        position,
        // This is replaced with the single/two-shape default after the full
        // property is known.
        geometry_box: BorderShapeGeometryBox::HalfBorder,
    })
}

fn parse_border_shape_circle_radius(
    value: &str,
    font_size: f32,
) -> Option<BorderShapeCircleRadius> {
    match value.to_ascii_lowercase().as_str() {
        "closest-side" => Some(BorderShapeCircleRadius::ClosestSide),
        "farthest-side" => Some(BorderShapeCircleRadius::FarthestSide),
        "closest-corner" => Some(BorderShapeCircleRadius::ClosestCorner),
        "farthest-corner" => Some(BorderShapeCircleRadius::FarthestCorner),
        _ => parse_computed_length_percentage(value, font_size)
            .filter(|value| !value.is_definitely_negative())
            .map(BorderShapeCircleRadius::LengthPercentage),
    }
}

fn parse_border_shape_ellipse(value: &str, font_size: f32) -> Option<BorderShapeEllipse> {
    let value = trim_css_value(value);
    let prefix = "ellipse(";
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(prefix) || !value.ends_with(')') {
        return None;
    }
    let components = split_css_component_values(&value[prefix.len()..value.len() - 1]);
    let at = components
        .iter()
        .position(|component| component.eq_ignore_ascii_case("at"));
    let (radii, position) = match at {
        Some(at) => (
            parse_border_shape_ellipse_radii(&components[..at], font_size)?,
            parse_border_shape_position(&components[at + 1..], font_size)?,
        ),
        None => (
            parse_border_shape_ellipse_radii(&components, font_size)?,
            BorderShapePosition::center(),
        ),
    };
    Some(BorderShapeEllipse {
        horizontal_radius: radii.0,
        vertical_radius: radii.1,
        position,
        geometry_box: BorderShapeGeometryBox::HalfBorder,
    })
}

fn parse_border_shape_ellipse_radii(
    values: &[&str],
    font_size: f32,
) -> Option<(BorderShapeEllipseRadius, BorderShapeEllipseRadius)> {
    if values.is_empty() {
        return Some((
            BorderShapeEllipseRadius::ClosestSide,
            BorderShapeEllipseRadius::ClosestSide,
        ));
    }
    let [horizontal, vertical] = values else {
        return None;
    };
    Some((
        parse_border_shape_ellipse_radius(horizontal, font_size)?,
        parse_border_shape_ellipse_radius(vertical, font_size)?,
    ))
}

fn parse_border_shape_ellipse_radius(
    value: &str,
    font_size: f32,
) -> Option<BorderShapeEllipseRadius> {
    match value.to_ascii_lowercase().as_str() {
        "closest-side" => Some(BorderShapeEllipseRadius::ClosestSide),
        "farthest-side" => Some(BorderShapeEllipseRadius::FarthestSide),
        "closest-corner" => Some(BorderShapeEllipseRadius::ClosestCorner),
        "farthest-corner" => Some(BorderShapeEllipseRadius::FarthestCorner),
        _ => parse_computed_length_percentage(value, font_size)
            .filter(|value| !value.is_definitely_negative())
            .map(BorderShapeEllipseRadius::LengthPercentage),
    }
}

fn parse_border_shape_position(values: &[&str], font_size: f32) -> Option<BorderShapePosition> {
    let [x, y] = values else {
        return None;
    };
    Some(BorderShapePosition {
        x: parse_border_shape_position_component(x, true, font_size)?,
        y: parse_border_shape_position_component(y, false, font_size)?,
    })
}

fn parse_border_shape_position_component(
    value: &str,
    horizontal: bool,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    match value.to_ascii_lowercase().as_str() {
        "center" => Some(ComputedLengthPercentage::from_percent(0.5)),
        "left" if horizontal => Some(ComputedLengthPercentage::ZERO),
        "right" if horizontal => Some(ComputedLengthPercentage::from_percent(1.0)),
        "top" if !horizontal => Some(ComputedLengthPercentage::ZERO),
        "bottom" if !horizontal => Some(ComputedLengthPercentage::from_percent(1.0)),
        _ => parse_computed_length_percentage(value, font_size),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_clip_accepts_complete_comma_or_whitespace_rects() {
        assert!(matches!(
            parse_legacy_clip("rect(1px, auto, 3px, 4px)", 12.0),
            Some(LegacyClip::Rect(_))
        ));
        assert!(matches!(
            parse_legacy_clip("rect(1px auto 3px 4px)", 12.0),
            Some(LegacyClip::Rect(_))
        ));
        assert_eq!(parse_legacy_clip("auto", 12.0), Some(LegacyClip::Auto));
    }

    #[test]
    fn legacy_clip_rejects_percentages_and_malformed_rects() {
        assert!(parse_legacy_clip("rect(1%, 2px, 3px, 4px)", 12.0).is_none());
        assert!(parse_legacy_clip("rect(1px, 2px 3px, 4px)", 12.0).is_none());
        assert!(parse_legacy_clip("rect(1px, 2px, 3px)", 12.0).is_none());
    }
}
