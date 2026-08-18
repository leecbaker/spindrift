use super::super::*;

/// Parses CSS `opacity`.
///
/// CSS CssColor defines `opacity` as a number or percentage clamped to the
/// `[0, 1]` range:
/// <https://www.w3.org/TR/css-color-4/#transparency>.
pub(in crate::css) fn parse_opacity(value: &str) -> Option<Opacity> {
    let value = trim_css_value(value);
    if let Some(percent) = parse_percentage(value) {
        return Opacity::new_clamped(percent);
    }
    value.parse::<f32>().ok().and_then(Opacity::new_clamped)
}

/// Parse CSS Filter Effects functions into their computed representation.
///
/// The current renderer can lower only an exact bounded color subset, but it
/// must retain other syntactically valid values so they continue to establish
/// the containing block and stacking context required by CSS Filter Effects.
/// <https://www.w3.org/TR/filter-effects-1/#filter-functions>
pub(crate) fn parse_filter(value: &str) -> Option<FilterValue> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(FilterValue::None);
    }
    let functions = css_function_list(value)?;
    let mut parsed = Vec::with_capacity(functions.len());
    for (name, arguments) in functions {
        let name = name.to_ascii_lowercase();
        let arguments = try_split_css_component_values(arguments)?;
        let parsed_amount = |default: f32| {
            let value = match arguments.as_slice() {
                [] => default,
                [value] => parse_filter_number_percentage(value)?,
                _ => return None,
            };
            NonNegativeFilterAmount::new(value)
        };
        let function = match name.as_str() {
            "grayscale" => FilterFunction::Grayscale(parsed_amount(1.0)?.clamped_unit_interval()),
            "saturate" => FilterFunction::Saturate(parsed_amount(1.0)?),
            "brightness" => FilterFunction::Brightness(parsed_amount(1.0)?),
            "opacity" => FilterFunction::Opacity(parsed_amount(1.0)?.clamped_unit_interval()),
            // Preserve complete, token-validated unsupported function source
            // until a raster filter backend can execute it.
            _ => FilterFunction::RequiresRasterBackend(format!("{name}({})", arguments.join(" "))),
        };
        parsed.push(function);
    }
    (!parsed.is_empty()).then_some(FilterValue::Functions(parsed))
}

fn parse_filter_number_percentage(value: &str) -> Option<f32> {
    parse_percentage(value).or_else(|| parse_css_number(value))
}

/// Parses CSS 2D and 3D transform functions into typed matrix operations.
///
/// 3D values remain typed through the computed-value phase rather than being
/// flattened while parsing, so the paint backend can reject projective output
/// deliberately and retain the affine subset:
/// <https://drafts.csswg.org/css-transforms-2/#transform-functions>.
pub(crate) fn parse_transform(value: &str, font_size: f32) -> Option<TransformList> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let functions = css_function_list(value)?;
    let mut transform = Vec::new();
    for (name, args) in functions {
        let lower_name = name.to_ascii_lowercase();
        let comma_args = try_split_css_top_level_delimiter(args, ',')?;
        let args = if comma_args.len() > 1 {
            if comma_args.iter().any(|argument| argument.is_empty()) {
                return None;
            }
            comma_args
        } else {
            try_split_css_component_values(args)?
        };
        match lower_name.as_str() {
            "matrix" if args.len() == 6 => {
                transform.push(TransformFunction::Matrix(CssAffineMatrix::new(
                    parse_css_number(args[0])?,
                    parse_css_number(args[1])?,
                    parse_css_number(args[2])?,
                    parse_css_number(args[3])?,
                    parse_css_number(args[4])?,
                    parse_css_number(args[5])?,
                )));
            }
            "matrix3d" if args.len() == 16 => {
                transform.push(TransformFunction::Matrix3D(CssMatrix3D::new(
                    parse_css_number(args[0])?,
                    parse_css_number(args[1])?,
                    parse_css_number(args[2])?,
                    parse_css_number(args[3])?,
                    parse_css_number(args[4])?,
                    parse_css_number(args[5])?,
                    parse_css_number(args[6])?,
                    parse_css_number(args[7])?,
                    parse_css_number(args[8])?,
                    parse_css_number(args[9])?,
                    parse_css_number(args[10])?,
                    parse_css_number(args[11])?,
                    parse_css_number(args[12])?,
                    parse_css_number(args[13])?,
                    parse_css_number(args[14])?,
                    parse_css_number(args[15])?,
                )));
            }
            "translate" if args.len() == 1 || args.len() == 2 => {
                let x = parse_computed_length_percentage(args[0], font_size)?;
                let y = args
                    .get(1)
                    .and_then(|arg| parse_computed_length_percentage(arg, font_size))
                    .unwrap_or(ComputedLengthPercentage::ZERO);
                transform.push(TransformFunction::Translate(CssTransformTranslation {
                    x,
                    y,
                }));
            }
            "translatex" if args.len() == 1 => {
                transform.push(TransformFunction::Translate(CssTransformTranslation {
                    x: parse_computed_length_percentage(args[0], font_size)?,
                    y: ComputedLengthPercentage::ZERO,
                }));
            }
            "translatey" if args.len() == 1 => {
                transform.push(TransformFunction::Translate(CssTransformTranslation {
                    x: ComputedLengthPercentage::ZERO,
                    y: parse_computed_length_percentage(args[0], font_size)?,
                }));
            }
            "translate3d" if args.len() == 3 => {
                let z = parse_computed_length_percentage(args[2], font_size)?;
                if z.contains_percentage() {
                    return None;
                }
                transform.push(TransformFunction::Translate3D(CssTransformTranslation3D {
                    x: parse_computed_length_percentage(args[0], font_size)?,
                    y: parse_computed_length_percentage(args[1], font_size)?,
                    z,
                }));
            }
            "translatez" if args.len() == 1 => {
                let z = parse_computed_length_percentage(args[0], font_size)?;
                if z.contains_percentage() {
                    return None;
                }
                transform.push(TransformFunction::Translate3D(CssTransformTranslation3D {
                    x: ComputedLengthPercentage::ZERO,
                    y: ComputedLengthPercentage::ZERO,
                    z,
                }));
            }
            "scale" if args.len() == 1 || args.len() == 2 => {
                let x = parse_scale_factor(args[0])?;
                let y = args
                    .get(1)
                    .and_then(|arg| parse_scale_factor(arg))
                    .unwrap_or(x);
                transform.push(TransformFunction::Scale(CssScaleFactors { x, y }));
            }
            "scalex" if args.len() == 1 => {
                transform.push(TransformFunction::Scale(CssScaleFactors {
                    x: parse_scale_factor(args[0])?,
                    y: 1.0,
                }));
            }
            "scaley" if args.len() == 1 => {
                transform.push(TransformFunction::Scale(CssScaleFactors {
                    x: 1.0,
                    y: parse_scale_factor(args[0])?,
                }));
            }
            "scale3d" if args.len() == 3 => {
                transform.push(TransformFunction::Scale3D(CssScaleFactors3D {
                    x: parse_scale_factor(args[0])?,
                    y: parse_scale_factor(args[1])?,
                    z: parse_scale_factor(args[2])?,
                }));
            }
            "scalez" if args.len() == 1 => {
                transform.push(TransformFunction::Scale3D(CssScaleFactors3D {
                    x: 1.0,
                    y: 1.0,
                    z: parse_scale_factor(args[0])?,
                }));
            }
            "rotate" if args.len() == 1 => {
                transform.push(TransformFunction::Rotate(euclid::Angle::radians(
                    parse_css_angle_radians(args[0])?,
                )));
            }
            "rotatex" if args.len() == 1 => {
                transform.push(TransformFunction::Rotate3D(CssRotate3D {
                    axis_x: 1.0,
                    axis_y: 0.0,
                    axis_z: 0.0,
                    angle: euclid::Angle::radians(parse_css_angle_radians(args[0])?),
                }));
            }
            "rotatey" if args.len() == 1 => {
                transform.push(TransformFunction::Rotate3D(CssRotate3D {
                    axis_x: 0.0,
                    axis_y: 1.0,
                    axis_z: 0.0,
                    angle: euclid::Angle::radians(parse_css_angle_radians(args[0])?),
                }));
            }
            "rotate3d" if args.len() == 4 => {
                let axis_x = parse_css_number(args[0])?;
                let axis_y = parse_css_number(args[1])?;
                let axis_z = parse_css_number(args[2])?;
                let axis_length = (axis_x * axis_x + axis_y * axis_y + axis_z * axis_z).sqrt();
                if axis_length == 0.0 || !axis_length.is_finite() {
                    return None;
                }
                transform.push(TransformFunction::Rotate3D(CssRotate3D {
                    axis_x: axis_x / axis_length,
                    axis_y: axis_y / axis_length,
                    axis_z: axis_z / axis_length,
                    angle: euclid::Angle::radians(parse_css_angle_radians(args[3])?),
                }));
            }
            "skew" if args.len() == 1 || args.len() == 2 => {
                let x = parse_css_angle_radians(args[0])?;
                let y = args
                    .get(1)
                    .and_then(|arg| parse_css_angle_radians(arg))
                    .unwrap_or(0.0);
                transform.push(TransformFunction::Skew(CssSkewAngles {
                    x: euclid::Angle::radians(x),
                    y: euclid::Angle::radians(y),
                }));
            }
            "skewx" if args.len() == 1 => {
                transform.push(TransformFunction::Skew(CssSkewAngles {
                    x: euclid::Angle::radians(parse_css_angle_radians(args[0])?),
                    y: euclid::Angle::radians(0.0),
                }));
            }
            "skewy" if args.len() == 1 => {
                transform.push(TransformFunction::Skew(CssSkewAngles {
                    x: euclid::Angle::radians(0.0),
                    y: euclid::Angle::radians(parse_css_angle_radians(args[0])?),
                }));
            }
            "perspective" if args.len() == 1 => {
                let perspective = if args[0].eq_ignore_ascii_case("none") {
                    ComputedPerspective::NONE
                } else {
                    let length = parse_computed_length_percentage(args[0], font_size)?;
                    ComputedPerspective::Distance(NonNegativeComputedLength::new(length)?)
                };
                transform.push(TransformFunction::Perspective(perspective));
            }
            _ => return None,
        }
    }
    Some(transform)
}

/// A syntactically valid SVG `transform` presentation attribute.
///
/// SVG permits an empty transform-list, which maps to the CSS `none` value;
/// that is distinct from an absent or malformed presentation attribute.
/// <https://drafts.csswg.org/css-transforms-1/#svg-transform>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SvgTransformAttributeValue {
    None,
    Affine(CssAffineMatrix),
}

/// Parse the SVG `transform` presentation-attribute grammar into its used
/// affine value. SVG accepts unitless translate lengths and angles, and its
/// three-argument `rotate()` has no CSS function equivalent:
/// <https://drafts.csswg.org/css-transforms-1/#svg-transform>
pub(crate) fn parse_svg_transform_attribute(value: &str) -> Option<SvgTransformAttributeValue> {
    let functions = parse_svg_transform_function_calls(value)?;
    if functions.is_empty() {
        return Some(SvgTransformAttributeValue::None);
    }
    let mut matrix = CssTransform::identity();
    for (name, arguments) in functions {
        let arguments = split_svg_transform_arguments(arguments)?;
        let operation = match name.to_ascii_lowercase().as_str() {
            "matrix" if arguments.len() == 6 => CssTransform::new(
                parse_css_number(arguments[0])?,
                parse_css_number(arguments[1])?,
                parse_css_number(arguments[2])?,
                parse_css_number(arguments[3])?,
                parse_css_number(arguments[4])?,
                parse_css_number(arguments[5])?,
            ),
            "translate" if arguments.len() == 1 || arguments.len() == 2 => {
                let y = if arguments.len() == 2 {
                    parse_css_number(arguments[1])?
                } else {
                    0.0
                };
                CssTransform::translation(parse_css_number(arguments[0])?, y)
            }
            "scale" if arguments.len() == 1 || arguments.len() == 2 => {
                let x = parse_css_number(arguments[0])?;
                let y = if arguments.len() == 2 {
                    parse_css_number(arguments[1])?
                } else {
                    x
                };
                CssTransform::scale(x, y)
            }
            "rotate" if arguments.len() == 1 || arguments.len() == 3 => {
                let angle = euclid::Angle::degrees(parse_css_number(arguments[0])?);
                if arguments.len() == 1 {
                    CssTransform::rotation(angle)
                } else {
                    let x = parse_css_number(arguments[1])?;
                    let y = parse_css_number(arguments[2])?;
                    CssTransform::translation(-x, -y)
                        .then(&CssTransform::rotation(angle))
                        .then(&CssTransform::translation(x, y))
                }
            }
            "skewx" if arguments.len() == 1 => CssTransform::new(
                1.0,
                0.0,
                euclid::Angle::degrees(parse_css_number(arguments[0])?)
                    .radians
                    .tan(),
                1.0,
                0.0,
                0.0,
            ),
            "skewy" if arguments.len() == 1 => CssTransform::new(
                1.0,
                euclid::Angle::degrees(parse_css_number(arguments[0])?)
                    .radians
                    .tan(),
                0.0,
                1.0,
                0.0,
                0.0,
            ),
            _ => return None,
        };
        matrix = operation.then(&matrix);
    }
    (matrix.m11.is_finite()
        && matrix.m12.is_finite()
        && matrix.m21.is_finite()
        && matrix.m22.is_finite()
        && matrix.m31.is_finite()
        && matrix.m32.is_finite())
    .then_some(SvgTransformAttributeValue::Affine(
        normalize_svg_affine_matrix(matrix),
    ))
}

/// Parse the SVG-specific transform-list production without relaxing CSS
/// transform syntax. CSS Transforms defines each SVG list boundary as an
/// optional `comma-wsp`, so adjacent, whitespace-separated, and
/// comma-separated function calls are all valid:
/// <https://drafts.csswg.org/css-transforms-1/#svg-transform>
fn parse_svg_transform_function_calls(value: &str) -> Option<Vec<(&str, &str)>> {
    let mut calls = Vec::new();
    let mut rest = trim_svg_wsp(value);
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = trim_svg_wsp(&rest[..open]);
        if name.is_empty() {
            return None;
        }
        let close = find_svg_matching_close_paren(rest, open)?;
        calls.push((name, &rest[open + 1..close]));

        rest = &rest[close + 1..];
        let trimmed = trim_svg_wsp(rest);
        if trimmed.is_empty() {
            break;
        }
        rest = if let Some(after_comma) = trimmed.strip_prefix(',') {
            let after_comma = trim_svg_wsp(after_comma);
            if after_comma.is_empty() {
                return None;
            }
            after_comma
        } else {
            trimmed
        };
    }
    Some(calls)
}

/// SVG transform-list whitespace is deliberately narrower than Rust's
/// Unicode whitespace predicate: LF, CR, TAB, and SPACE only.
fn trim_svg_wsp(value: &str) -> &str {
    value.trim_matches(|character| matches!(character, '\n' | '\r' | '\t' | ' '))
}

/// SVG transform attributes use their own grammar, including adjacent number
/// tokens.  This byte scanner is deliberately local to that non-CSS parser.
fn find_svg_matching_close_paren(value: &str, open: usize) -> Option<usize> {
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

/// Split SVG transform function number arguments at the grammar's
/// `comma-wsp?` boundaries. Unlike the CSS helper, this retains empty fields
/// long enough to reject doubled or trailing commas instead of normalizing
/// malformed input into a valid argument list.
fn split_svg_transform_arguments(value: &str) -> Option<Vec<&str>> {
    let mut arguments = Vec::new();
    let mut rest = trim_svg_wsp(value);
    while !rest.is_empty() {
        let end = svg_number_token_end(rest)?;
        arguments.push(&rest[..end]);
        rest = &rest[end..];

        let after_whitespace = trim_svg_wsp(rest);
        if after_whitespace.is_empty() {
            break;
        }
        rest = if let Some(after_comma) = after_whitespace.strip_prefix(',') {
            let after_comma = trim_svg_wsp(after_comma);
            if after_comma.is_empty() {
                return None;
            }
            after_comma
        } else {
            after_whitespace
        };
    }
    Some(arguments)
}

/// Return the byte boundary of one CSS `<number-token>`. The SVG transform
/// grammar permits `comma-wsp?` between numbers, so an adjacent sign may begin
/// the next number (for example `translate(10-5)`).
/// <https://drafts.csswg.org/css-syntax-3/#typedef-number-token>
fn svg_number_token_end(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut index = 0;
    if matches!(bytes.get(index), Some(b'+' | b'-')) {
        index += 1;
    }

    let digits_before_decimal = index;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    let has_digits_before_decimal = index > digits_before_decimal;
    let mut has_digits_after_decimal = false;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let digits_after_decimal = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        has_digits_after_decimal = index > digits_after_decimal;
    }
    if !(has_digits_before_decimal || has_digits_after_decimal) {
        return None;
    }

    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        let exponent_start = index;
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let exponent_digits = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == exponent_digits {
            index = exponent_start;
        }
    }
    Some(index)
}

/// Canonicalize numerical identity noise before a scene matrix reaches SVG
/// serialization. A transform parser is a real cross-engine boundary, so
/// `rotate(90)` must not become a near-zero scale term in the SVG payload.
fn normalize_svg_affine_matrix(transform: CssTransform) -> CssAffineMatrix {
    const EPSILON: f32 = 1e-6;
    let canonical = |value: f32| {
        if value.abs() < EPSILON {
            0.0
        } else if (value - 1.0).abs() < EPSILON {
            1.0
        } else if (value + 1.0).abs() < EPSILON {
            -1.0
        } else {
            value
        }
    };
    CssAffineMatrix::new(
        canonical(transform.m11),
        canonical(transform.m12),
        canonical(transform.m21),
        canonical(transform.m22),
        canonical(transform.m31),
        canonical(transform.m32),
    )
}

/// Normalize an SVG `transform-origin` presentation attribute into a CSS
/// declaration. SVG accepts unitless user-coordinate lengths where CSS
/// requires a unit; other components continue through CSS parsing and
/// computed-value resolution unchanged.
/// <https://drafts.csswg.org/css-transforms/#transform-origin-property>
pub(crate) fn svg_transform_origin_presentation_declaration(value: &str) -> Option<String> {
    let values = try_split_css_component_values(trim_css_value(value))?;
    if values.is_empty() || values.len() > 2 {
        return None;
    }
    if !svg_transform_origin_component_order_is_valid(&values) {
        return None;
    }
    let values = values
        .into_iter()
        .map(svg_origin_component_as_css)
        .collect::<Option<Vec<_>>>()?;
    Some(format!("transform-origin: {}", values.join(" ")))
}

/// SVG uses the CSS position grammar, but its legacy presentation-attribute
/// parser must reject token orders that the CSS parser otherwise normalizes
/// (for example `top 100%` and `left right`).
fn svg_transform_origin_component_order_is_valid(values: &[&str]) -> bool {
    let [first, second] = values else {
        return true;
    };
    let vertical = |value: &str| matches!(value.to_ascii_lowercase().as_str(), "top" | "bottom");
    let horizontal = |value: &str| matches!(value.to_ascii_lowercase().as_str(), "left" | "right");
    let center = |value: &str| value.eq_ignore_ascii_case("center");
    match (vertical(first), horizontal(first), center(first)) {
        (true, _, _) => horizontal(second) || center(second),
        (_, true, _) => !horizontal(second),
        (_, _, true) => true,
        _ => !horizontal(second),
    }
}

fn svg_origin_component_as_css(value: &str) -> Option<String> {
    let value = trim_css_value(value);
    if value.is_empty() {
        return None;
    }
    if parse_css_number(value).is_some() {
        return Some(format!("{value}px"));
    }
    Some(value.to_owned())
}

/// Parses the 2D subset of the CSS Transforms Level 2 `translate` property.
///
/// The unsupported third (Z) component deliberately rejects the declaration,
/// rather than silently treating a 3D value as 2D:
/// <https://drafts.csswg.org/css-transforms-2/#propdef-translate>.
pub(crate) fn parse_individual_translate(
    value: &str,
    font_size: f32,
) -> Option<Option<CssTransformTranslation>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    let args = try_split_css_component_values(value)?;
    match args.as_slice() {
        [x] => Some(Some(CssTransformTranslation {
            x: parse_computed_length_percentage(x, font_size)?,
            y: ComputedLengthPercentage::ZERO,
        })),
        [x, y] => Some(Some(CssTransformTranslation {
            x: parse_computed_length_percentage(x, font_size)?,
            y: parse_computed_length_percentage(y, font_size)?,
        })),
        _ => None,
    }
}

/// Parses the 2D subset of CSS Transforms Level 2 `rotate`.
///
/// <https://drafts.csswg.org/css-transforms-2/#propdef-rotate>.
pub(crate) fn parse_individual_rotate(value: &str) -> Option<Option<euclid::Angle<f32>>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        Some(None)
    } else {
        parse_css_angle_radians(value)
            .map(euclid::Angle::radians)
            .map(Some)
    }
}

/// Parses CSS Transforms Level 2's independent 2D `scale` property.
///
/// <https://drafts.csswg.org/css-transforms-2/#propdef-scale>.
pub(crate) fn parse_individual_scale(value: &str) -> Option<Option<CssScaleFactors>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    let args = try_split_css_component_values(value)?;
    match args.as_slice() {
        [x] => {
            let x = parse_scale_factor(x)?;
            Some(Some(CssScaleFactors { x, y: x }))
        }
        [x, y] => Some(Some(CssScaleFactors {
            x: parse_scale_factor(x)?,
            y: parse_scale_factor(y)?,
        })),
        _ => None,
    }
}

/// Parses CSS Images Level 5 `object-view-box` basic rectangle forms.
pub(crate) fn parse_object_view_box(value: &str, font_size: f32) -> Option<ObjectViewBox> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(ObjectViewBox::None);
    }
    let calls = css_function_list(value)?;
    let [(name, body)] = calls.as_slice() else {
        return None;
    };
    let name = name.to_ascii_lowercase();
    let components = try_split_css_component_values(body)?;
    let round_index = components
        .iter()
        .position(|component| component.eq_ignore_ascii_case("round"));
    let (rect_components, radii) = match round_index {
        Some(index) => {
            let radius = components[index + 1..].join(" ");
            (!radius.is_empty()).then(|| parse_border_radius(&radius, font_size))??;
            (
                &components[..index],
                parse_border_radius(&radius, font_size),
            )
        }
        None => (components.as_slice(), None),
    };
    let parse = |part: &str| parse_computed_length_percentage(part, font_size);
    match name.as_str() {
        "inset" => {
            let values = rect_components
                .iter()
                .map(|value| parse(value))
                .collect::<Option<Vec<_>>>()?;
            let [top, right, bottom, left] = match values.as_slice() {
                [all] => [all.clone(), all.clone(), all.clone(), all.clone()],
                [vertical, horizontal] => [
                    vertical.clone(),
                    horizontal.clone(),
                    vertical.clone(),
                    horizontal.clone(),
                ],
                [top, horizontal, bottom] => [
                    top.clone(),
                    horizontal.clone(),
                    bottom.clone(),
                    horizontal.clone(),
                ],
                [top, right, bottom, left] => {
                    [top.clone(), right.clone(), bottom.clone(), left.clone()]
                }
                _ => return None,
            };
            Some(ObjectViewBox::Inset {
                top,
                right,
                bottom,
                left,
                radii,
            })
        }
        "xywh" if rect_components.len() == 4 => Some(ObjectViewBox::Xywh {
            x: parse(rect_components[0])?,
            y: parse(rect_components[1])?,
            width: parse(rect_components[2])?,
            height: parse(rect_components[3])?,
            radii,
        }),
        "rect" if round_index.is_none() => {
            // CSS Images accepts the modern whitespace-separated basic-shape
            // rect syntax; retain the legacy comma form for compatibility.
            // <https://drafts.csswg.org/css-images-5/#the-object-view-box-property>
            let comma_values = try_split_css_top_level_delimiter(body, ',')?;
            let values = if comma_values.len() > 1 {
                comma_values
            } else {
                rect_components.to_vec()
            };
            let [top, right, bottom, left] = values.as_slice() else {
                return None;
            };
            Some(ObjectViewBox::Rect {
                top: parse(top)?,
                right: parse(right)?,
                bottom: parse(bottom)?,
                left: parse(left)?,
            })
        }
        _ => None,
    }
}

/// Parses the `<number> | <percentage>` scale-factor grammar shared by the
/// legacy functions and the Level 2 `scale` property.
///
/// Percentages are normalized to multiplicative factors at computed-value
/// time; unlike lengths they do not resolve against a box dimension:
/// <https://drafts.csswg.org/css-transforms-2/#typedef-scale-value>.
pub(in crate::css) fn parse_scale_factor(value: &str) -> Option<f32> {
    parse_percentage(value).or_else(|| parse_css_number(value))
}

/// Parses the 2D reference-box origin and optional absolute z-origin.
///
/// CSS Transforms resolves keyword origins to percentages over the border box:
/// <https://www.w3.org/TR/css-transforms-1/#transform-origin-property>.
/// The unordered keyword form assigns one component to each physical axis;
/// `center` supplies the other axis when it appears alongside an edge:
/// <https://drafts.csswg.org/css-values-4/#position>.
pub(crate) fn parse_transform_origin(value: &str, font_size: f32) -> Option<TransformOrigin> {
    let parts = try_split_css_component_values(trim_css_value(value))?;
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let z = match parts.get(2) {
        Some(value) => {
            let value = parse_computed_length_percentage(value, font_size)?;
            if value.contains_percentage() {
                return None;
            }
            value
        }
        None => ComputedLengthPercentage::ZERO,
    };
    let parts = &parts[..parts.len().min(2)];
    match parts {
        [single] if is_vertical_origin_keyword(single) => Some(TransformOrigin::specified(
            ComputedLengthPercentage::from_percent(0.5),
            parse_origin_component(single, true, font_size)?,
            z,
        )),
        [single] => Some(TransformOrigin::specified(
            parse_origin_component(single, false, font_size)?,
            ComputedLengthPercentage::from_percent(0.5),
            z,
        )),
        [first, second] if is_vertical_origin_keyword(first) => Some(TransformOrigin::specified(
            parse_origin_component(second, false, font_size)?,
            parse_origin_component(first, true, font_size)?,
            z,
        )),
        // The CSS `&&` grammar permits the horizontal and vertical keyword
        // components in either order. Here `center` supplies the vertical
        // component before the horizontal edge, as in `center left`.
        [first, second]
            if first.eq_ignore_ascii_case("center") && is_horizontal_origin_keyword(second) =>
        {
            Some(TransformOrigin::specified(
                parse_origin_component(second, false, font_size)?,
                parse_origin_component(first, true, font_size)?,
                z,
            ))
        }
        [first, second] => Some(TransformOrigin::specified(
            parse_origin_component(first, false, font_size)?,
            parse_origin_component(second, true, font_size)?,
            z,
        )),
        _ => None,
    }
}

/// Parses CSS Transforms 2 `perspective-origin`.
///
/// This deliberately has a distinct semantic result from `transform-origin`:
/// perspective origins have no z component.
/// <https://drafts.csswg.org/css-transforms-2/#perspective-origin-property>
pub(crate) fn parse_perspective_origin(value: &str, font_size: f32) -> Option<PerspectiveOrigin> {
    let origin = parse_transform_origin(value, font_size)?;
    (origin.z == ComputedLengthPercentage::ZERO).then(|| PerspectiveOrigin::new(origin.x, origin.y))
}

/// Parse the transform reference-box keyword without collapsing SVG values
/// into an HTML box. The SVG scene adapter resolves the latter against its
/// concrete geometry after layout.
/// <https://drafts.csswg.org/css-transforms-1/#transform-box-property>
pub(crate) fn parse_transform_box(value: &str) -> Option<TransformBox> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "content-box" => Some(TransformBox::ContentBox),
        "border-box" => Some(TransformBox::BorderBox),
        "fill-box" => Some(TransformBox::FillBox),
        "stroke-box" => Some(TransformBox::StrokeBox),
        "view-box" => Some(TransformBox::ViewBox),
        _ => None,
    }
}

/// Parse the CSS Transforms 2 3D rendering-context selector.
///
/// Grouping properties can later force a specified `preserve-3d` to its used
/// `flat` value; that is deliberately a paint/layout used-value decision.
/// <https://drafts.csswg.org/css-transforms-2/#transform-style-property>
pub(crate) fn parse_transform_style(value: &str) -> Option<TransformStyle> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "flat" => Some(TransformStyle::Flat),
        "preserve-3d" => Some(TransformStyle::Preserve3d),
        _ => None,
    }
}

pub(in crate::css) fn parse_origin_component(
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

pub(in crate::css) fn is_vertical_origin_keyword(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "top" | "bottom")
}

fn is_horizontal_origin_keyword(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "left" | "right")
}

/// Parse CSS Compositing's `mix-blend-mode` keywords.
///
/// Non-`normal` values establish a stacking context:
/// <https://www.w3.org/TR/compositing-1/#mix-blend-mode>.
pub(in crate::css) fn parse_mix_blend_mode(value: &str) -> Option<MixBlendMode> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_transform_attribute_accepts_svg_angle_and_centered_rotate_syntax() {
        let SvgTransformAttributeValue::Affine(matrix) =
            parse_svg_transform_attribute("rotate(90, 20, 20)").expect("valid SVG rotate")
        else {
            panic!("non-empty SVG transform must resolve to an affine matrix");
        };
        let transform: CssTransform = matrix.into_space(euclid::Scale::new(1.0));

        assert_eq!(transform.m11, 0.0);
        assert_eq!(transform.m12, 1.0);
        assert_eq!(transform.m21, -1.0);
        assert_eq!(transform.m22, 0.0);
        assert_eq!(transform.m31, 40.0);
        assert_eq!(transform.m32, 0.0);
    }

    #[test]
    fn svg_transform_attribute_rejects_invalid_optional_components() {
        assert!(parse_svg_transform_attribute("translate(10 invalid)").is_none());
        assert!(parse_svg_transform_attribute("scale(2 invalid)").is_none());
    }

    #[test]
    fn svg_transform_attribute_accepts_all_transform_list_separators() {
        for separator in ["", " ", "\t", "\n", "\r", ",", ", ", " ,\t"] {
            let value = format!("translate(5, 7){separator}scale(2)");
            assert!(
                matches!(
                    parse_svg_transform_attribute(&value),
                    Some(SvgTransformAttributeValue::Affine(_))
                ),
                "{value:?}"
            );
        }
    }

    #[test]
    fn svg_transform_attribute_accepts_adjacent_css_number_tokens() {
        for value in ["translate(10-5)", "scale(2+3)", "translate(1e2-3e1)"] {
            assert!(
                matches!(
                    parse_svg_transform_attribute(value),
                    Some(SvgTransformAttributeValue::Affine(_))
                ),
                "{value:?}"
            );
        }
        assert!(parse_svg_transform_attribute("translate(1e, 2)").is_none());
    }

    #[test]
    fn svg_transform_attribute_rejects_invalid_list_and_argument_separators() {
        for value in [
            "none",
            ",translate(1)",
            "translate(1),",
            "translate(1),,scale(2)",
            "translate(1) , , scale(2)",
            "translate(1",
            "unknown(1)",
            "matrix(1 0 0 1 0)",
            "translate(1,,2)",
            "translate(1,)",
        ] {
            assert!(parse_svg_transform_attribute(value).is_none(), "{value:?}");
        }
    }

    #[test]
    fn svg_transform_attribute_distinguishes_empty_list_from_invalid_input() {
        for value in ["", " \t\n\r "] {
            assert_eq!(
                parse_svg_transform_attribute(value),
                Some(SvgTransformAttributeValue::None)
            );
        }
        assert!(parse_svg_transform_attribute("none").is_none());
    }

    #[test]
    fn svg_origin_attribute_normalizes_unitless_user_lengths_but_preserves_css_syntax() {
        assert_eq!(
            svg_transform_origin_presentation_declaration("100 0"),
            Some("transform-origin: 100px 0px".to_owned())
        );
        assert_eq!(
            svg_transform_origin_presentation_declaration("center 25%"),
            Some("transform-origin: center 25%".to_owned())
        );
        assert_eq!(
            svg_transform_origin_presentation_declaration("2cm 1in"),
            Some("transform-origin: 2cm 1in".to_owned())
        );
        assert_eq!(svg_transform_origin_presentation_declaration(""), None);
        assert_eq!(svg_transform_origin_presentation_declaration("0 0 0"), None);
        assert_eq!(
            svg_transform_origin_presentation_declaration("top 100%"),
            None
        );
        assert_eq!(
            svg_transform_origin_presentation_declaration("left right"),
            None
        );
    }

    #[test]
    fn svg_origin_attribute_preserves_unordered_center_and_edge_keywords() {
        for (value, x) in [("center left", 0.0), ("center right", 1.0)] {
            let declaration = svg_transform_origin_presentation_declaration(value)
                .expect("the SVG presentation attribute is valid");
            let origin = parse_transform_origin(
                declaration
                    .strip_prefix("transform-origin: ")
                    .expect("the declaration has the transform-origin prefix"),
                12.0,
            )
            .expect("the normalized CSS declaration remains valid");
            assert_eq!(
                origin,
                TransformOrigin::specified(
                    if x == 0.0 {
                        ComputedLengthPercentage::ZERO
                    } else {
                        ComputedLengthPercentage::from_percent(x)
                    },
                    ComputedLengthPercentage::from_percent(0.5),
                    ComputedLengthPercentage::ZERO,
                ),
                "{value}"
            );
        }
    }

    #[test]
    fn transform_origin_accepts_all_valid_two_keyword_axis_orders() {
        let keyword_position = |value| {
            if value == 0.0 {
                ComputedLengthPercentage::ZERO
            } else {
                ComputedLengthPercentage::from_percent(value)
            }
        };
        for (value, x, y) in [
            ("left top", 0.0, 0.0),
            ("left center", 0.0, 0.5),
            ("left bottom", 0.0, 1.0),
            ("center top", 0.5, 0.0),
            ("center center", 0.5, 0.5),
            ("center bottom", 0.5, 1.0),
            ("right top", 1.0, 0.0),
            ("right center", 1.0, 0.5),
            ("right bottom", 1.0, 1.0),
            ("top left", 0.0, 0.0),
            ("top center", 0.5, 0.0),
            ("top right", 1.0, 0.0),
            ("bottom left", 0.0, 1.0),
            ("bottom center", 0.5, 1.0),
            ("bottom right", 1.0, 1.0),
            ("center left", 0.0, 0.5),
            ("center right", 1.0, 0.5),
        ] {
            assert_eq!(
                parse_transform_origin(value, 12.0),
                Some(TransformOrigin::specified(
                    keyword_position(x),
                    keyword_position(y),
                    ComputedLengthPercentage::ZERO,
                )),
                "{value}"
            );
        }
    }

    #[test]
    fn transform_origin_preserves_z_after_unordered_center_and_edge_keywords() {
        for (value, x) in [("center left 12px", 0.0), ("center right 12px", 1.0)] {
            assert_eq!(
                parse_transform_origin(value, 12.0),
                Some(TransformOrigin::specified(
                    if x == 0.0 {
                        ComputedLengthPercentage::ZERO
                    } else {
                        ComputedLengthPercentage::from_percent(x)
                    },
                    ComputedLengthPercentage::from_percent(0.5),
                    ComputedLengthPercentage::from_points(12.0 * crate::css::CSS_PX_TO_PT),
                )),
                "{value}"
            );
        }
    }

    #[test]
    fn transform_origin_rejects_two_edge_keywords_on_the_same_axis() {
        for value in [
            "left left",
            "left right",
            "right left",
            "right right",
            "top top",
            "top bottom",
            "bottom top",
            "bottom bottom",
        ] {
            assert!(parse_transform_origin(value, 12.0).is_none(), "{value}");
        }
    }

    #[test]
    fn transform_style_accepts_only_its_two_css_keywords() {
        assert_eq!(parse_transform_style("flat"), Some(TransformStyle::Flat));
        assert_eq!(
            parse_transform_style("  preserve-3d "),
            Some(TransformStyle::Preserve3d)
        );
        assert_eq!(parse_transform_style("inherit"), None);
        assert_eq!(parse_transform_style("preserve3d"), None);
    }

    #[test]
    fn css_transform_uses_tokenized_function_and_argument_boundaries() {
        assert!(parse_transform(r"tr\61 nslate(10px/**/, 20px) sc\61 le(2)", 12.0).is_some());
        assert!(parse_transform("translate(10px,)", 12.0).is_none());
        assert!(parse_transform("translate(10px) nope", 12.0).is_none());
    }

    #[test]
    fn perspective_function_preserves_zero_and_none_but_rejects_negative_lengths() {
        let zero = parse_transform("perspective(0)", 12.0).expect("zero is valid");
        let [TransformFunction::Perspective(ComputedPerspective::Distance(distance))] =
            zero.as_slice()
        else {
            panic!("expected a perspective distance");
        };
        assert_eq!(distance.length(), 0.0);
        assert_eq!(
            ComputedPerspective::Distance(distance.clone())
                .used_for_rendering()
                .expect("a non-none perspective has a used distance")
                .points(),
            crate::css::CSS_PX_TO_PT,
        );
        assert!(matches!(
            parse_transform("perspective(none)", 12.0),
            Some(transform) if matches!(transform.as_slice(), [TransformFunction::Perspective(ComputedPerspective::None)])
        ));
        assert!(parse_transform("perspective(-1px)", 12.0).is_none());
    }

    #[test]
    fn perspective_origin_has_no_z_component() {
        assert_eq!(
            parse_perspective_origin("right bottom", 12.0),
            Some(PerspectiveOrigin::new(
                ComputedLengthPercentage::from_percent(1.0),
                ComputedLengthPercentage::from_percent(1.0),
            ))
        );
        for (value, x) in [("center left", 0.0), ("center right", 1.0)] {
            assert_eq!(
                parse_perspective_origin(value, 12.0),
                Some(PerspectiveOrigin::new(
                    if x == 0.0 {
                        ComputedLengthPercentage::ZERO
                    } else {
                        ComputedLengthPercentage::from_percent(x)
                    },
                    ComputedLengthPercentage::from_percent(0.5),
                )),
                "{value}"
            );
        }
        assert!(parse_perspective_origin("50% 50% 1px", 12.0).is_none());
    }

    #[test]
    fn filter_parser_applies_defaults_and_retains_raster_only_functions() {
        let Some(FilterValue::Functions(functions)) =
            parse_filter("grayscale() opacity(50%) blur(1px)")
        else {
            panic!("expected a valid filter list");
        };
        assert!(matches!(functions[0], FilterFunction::Grayscale(amount) if amount.value() == 1.0));
        assert!(matches!(functions[1], FilterFunction::Opacity(amount) if amount.value() == 0.5));
        assert!(matches!(
            functions[2],
            FilterFunction::RequiresRasterBackend(_)
        ));
    }

    #[test]
    fn filter_parser_rejects_negative_amounts() {
        assert!(parse_filter("brightness(-1)").is_none());
        assert!(parse_filter("grayscale(-1%)").is_none());
    }
}
