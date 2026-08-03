use super::super::*;

/// Parses CSS `opacity`.
///
/// CSS CssColor defines `opacity` as a number or percentage clamped to the
/// `[0, 1]` range:
/// <https://www.w3.org/TR/css-color-4/#transparency>.
pub(in crate::css) fn parse_opacity(value: &str) -> Option<f32> {
    let value = trim_css_value(value);
    if let Some(percent) = parse_percentage(value) {
        return Some(percent.clamp(0.0, 1.0));
    }
    value.parse::<f32>().ok().map(|value| value.clamp(0.0, 1.0))
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
                let length = parse_computed_length_percentage(args[0], font_size)?;
                if length.contains_percentage() || length.length_if_no_percent()? <= 0.0 {
                    return None;
                }
                transform.push(TransformFunction::Perspective(length));
            }
            _ => return None,
        }
    }
    Some(transform)
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
    let args = split_css_whitespace_args(value);
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
    let args = split_css_whitespace_args(value);
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
    let calls = parse_transform_function_calls(value)?;
    let [(name, body)] = calls.as_slice() else {
        return None;
    };
    let name = name.to_ascii_lowercase();
    let components = split_css_component_values(body);
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
            let values = if body.contains(',') {
                split_css_args(body, ',')
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
pub(crate) fn parse_transform_origin(value: &str, font_size: f32) -> Option<TransformOrigin> {
    let parts = split_css_whitespace_args(trim_css_value(value));
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
        [single] if is_vertical_origin_keyword(single) => Some(TransformOrigin {
            x: ComputedLengthPercentage::from_percent(0.5),
            y: parse_origin_component(single, true, font_size)?,
            z,
        }),
        [single] => Some(TransformOrigin {
            x: parse_origin_component(single, false, font_size)?,
            y: ComputedLengthPercentage::from_percent(0.5),
            z,
        }),
        [first, second] if is_vertical_origin_keyword(first) => Some(TransformOrigin {
            x: parse_origin_component(second, false, font_size)?,
            y: parse_origin_component(first, true, font_size)?,
            z,
        }),
        [first, second] => Some(TransformOrigin {
            x: parse_origin_component(first, false, font_size)?,
            y: parse_origin_component(second, true, font_size)?,
            z,
        }),
        _ => None,
    }
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
