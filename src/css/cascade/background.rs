use super::*;
use crate::css::component_values::{
    css_single_function, css_single_ident, split_css_top_level_once,
    try_split_css_component_values, try_split_css_top_level_delimiter,
};

pub(super) fn apply_background_shorthand(
    style: &mut ComputedStyle,
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) {
    let value = value.trim();
    let Some(layers) =
        parse_background_shorthand_layers(value, style.font_size, base_url, root_url)
    else {
        return;
    };
    let Some(tokens) = background_tokens(value) else {
        return;
    };
    style.background.background_color = if tokens
        .iter()
        .any(|token| css_identifier_is(token, "currentcolor"))
    {
        BackgroundColor::CurrentColor
    } else {
        parse_color_from_currentcolor_in_scheme(value, style.color, style.used_color_scheme)
            .or_else(|| parse_color(value))
            .or_else(|| {
                tokens.iter().find_map(|token| {
                    parse_color_from_currentcolor_in_scheme(
                        token,
                        style.color,
                        style.used_color_scheme,
                    )
                    .or_else(|| parse_color(token))
                })
            })
            .map(BackgroundColor::Color)
            .unwrap_or(BackgroundColor::TRANSPARENT)
    };
    style.background.background_layers = layers;
    style.background.background_image_layer_count = style.background.background_layers.len().max(1);
    if style.background.background_layers.is_empty() {
        sync_background_layers_from_single_fields(style);
    } else {
        sync_background_single_fields_from_layers(style);
    }
}

/// Parses a background box keyword for `background-origin` and
/// `background-clip`.
///
/// CSS Backgrounds Level 4 adds `border-area` for `background-clip`; callers
/// reject it where the grammar only admits a positioning box:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin>.
pub(super) fn parse_background_box(value: &str) -> Option<BackgroundBox> {
    match css_single_ident(trim_css_value(value))?
        .to_ascii_lowercase()
        .as_str()
    {
        "border-box" => Some(BackgroundBox::Border),
        "border-area" => Some(BackgroundBox::BorderArea),
        "padding-box" => Some(BackgroundBox::Padding),
        "content-box" => Some(BackgroundBox::Content),
        _ => None,
    }
}

/// Parses the single-layer `background-repeat` value.
///
/// CSS Backgrounds and Borders defines one- and two-value repeat syntax,
/// including `repeat-x` and `repeat-y` aliases and the `space`/`round`
/// distribution styles:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
pub(super) fn parse_background_repeat(value: &str) -> Option<BackgroundRepeat> {
    let tokens = background_tokens(value)?;
    if tokens.len() == 1 && css_identifier_is(tokens[0], "repeat-x") {
        return Some(BackgroundRepeat::RepeatX);
    }
    if tokens.len() == 1 && css_identifier_is(tokens[0], "repeat-y") {
        return Some(BackgroundRepeat::RepeatY);
    }
    let repeat_tokens = tokens
        .iter()
        .filter_map(|token| {
            css_single_ident(token)
                .and_then(|token| parse_background_repeat_axis(&token.to_ascii_lowercase()))
        })
        .collect::<Vec<_>>();
    match repeat_tokens.as_slice() {
        [] => None,
        [axis] => Some(BackgroundRepeat::new(*axis, *axis)),
        [x, y] => Some(BackgroundRepeat::new(*x, *y)),
        _ => None,
    }
}

fn parse_background_repeat_longhand(value: &str) -> Option<BackgroundRepeat> {
    let tokens = background_tokens(value)?;
    tokens
        .iter()
        .all(|token| {
            css_single_ident(token).is_some_and(|token| {
                matches!(
                    token.to_ascii_lowercase().as_str(),
                    "repeat" | "space" | "round" | "no-repeat" | "repeat-x" | "repeat-y"
                )
            })
        })
        .then(|| parse_background_repeat(value))?
}

fn parse_background_repeat_axis(token: &str) -> Option<BackgroundRepeatAxis> {
    match token {
        "repeat" => Some(BackgroundRepeatAxis::Repeat),
        "space" => Some(BackgroundRepeatAxis::Space),
        "round" => Some(BackgroundRepeatAxis::Round),
        "no-repeat" => Some(BackgroundRepeatAxis::NoRepeat),
        _ => None,
    }
}

pub(super) fn apply_background_image_list(
    style: &mut ComputedStyle,
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) {
    let Some(images) = parse_background_image_layers(value, base_url, root_url) else {
        return;
    };
    if images.is_empty() {
        style.background.background_image = ComputedImage::None;
        style.background.background_layers.clear();
        style.background.background_image_layer_count = 1;
        return;
    }
    style.background.background_image_layer_count = images.len();
    ensure_background_layer_count(style, images.len());
    for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
        layer.image = repeated_layer_value(&images, index);
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_size_list(style: &mut ComputedStyle, value: &str) {
    let Some(values) = background_layers(value).and_then(|layers| {
        layers
            .into_iter()
            .map(|part| parse_background_size(part, style.font_size))
            .collect::<Option<Vec<_>>>()
    }) else {
        return;
    };
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
        layer.size = repeated_layer_value(&values, index);
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_position_list(style: &mut ComputedStyle, value: &str) {
    let Some(values) = background_layers(value).and_then(|layers| {
        layers
            .into_iter()
            .map(|part| parse_background_position(part, style.font_size))
            .collect::<Option<Vec<_>>>()
    }) else {
        return;
    };
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
        layer.position = repeated_layer_value(&values, index);
    }
    sync_background_single_fields_from_layers(style);
}

/// Applies one axis of the comma-separated `background-position` longhands.
///
/// `background-position-x` and `background-position-y` replace only their
/// respective components, repeating a shorter list to match the established
/// background layer count:
/// <https://www.w3.org/TR/css-backgrounds-4/#background-position>.
pub(super) fn apply_background_position_axis_list(
    style: &mut ComputedStyle,
    value: &str,
    horizontal: bool,
) {
    let Some(values) = background_layers(value).and_then(|layers| {
        layers
            .into_iter()
            .map(|part| parse_background_position_axis(part, style.font_size, horizontal))
            .collect::<Option<Vec<_>>>()
    }) else {
        return;
    };
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
        if horizontal {
            layer.position.x = repeated_layer_value(&values, index);
        } else {
            layer.position.y = repeated_layer_value(&values, index);
        }
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_repeat_list(style: &mut ComputedStyle, value: &str) {
    let Some(values) = background_layers(value).and_then(|layers| {
        layers
            .into_iter()
            .map(parse_background_repeat_longhand)
            .collect::<Option<Vec<_>>>()
    }) else {
        return;
    };
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
        layer.repeat = repeated_layer_value(&values, index);
    }
    sync_background_single_fields_from_layers(style);
}

/// Applies the comma-separated `background-attachment` list.
///
/// CSS Backgrounds repeats shorter layer lists to the number of background
/// image layers and initializes every layer to `scroll`:
/// <https://www.w3.org/TR/css-backgrounds-3/#background-attachment>.
pub(super) fn apply_background_attachment_list(style: &mut ComputedStyle, value: &str) {
    let Some(values) = background_layers(value).and_then(|layers| {
        layers
            .into_iter()
            .map(|value| {
                match css_single_ident(trim_css_value(value))?
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "scroll" => Some(BackgroundAttachment::Scroll),
                    "fixed" => Some(BackgroundAttachment::Fixed),
                    "local" => Some(BackgroundAttachment::Local),
                    _ => None,
                }
            })
            .collect::<Option<Vec<_>>>()
    }) else {
        return;
    };
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
        layer.attachment = repeated_layer_value(&values, index);
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_origin_list(style: &mut ComputedStyle, value: &str) {
    let Some(values) = background_layers(value).and_then(|layers| {
        layers
            .into_iter()
            .map(|value| {
                parse_background_box(value).filter(|box_| *box_ != BackgroundBox::BorderArea)
            })
            .collect::<Option<Vec<_>>>()
    }) else {
        return;
    };
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
        layer.origin = repeated_layer_value(&values, index);
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_clip_list(style: &mut ComputedStyle, value: &str) {
    let Some(values) = background_layers(value).and_then(|layers| {
        layers
            .into_iter()
            .map(parse_background_box)
            .collect::<Option<Vec<_>>>()
    }) else {
        return;
    };
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background.background_layers.iter_mut().enumerate() {
        layer.clip = repeated_layer_value(&values, index);
    }
    sync_background_single_fields_from_layers(style);
}

/// Parses `background-size` for a single background layer.
///
/// CSS Backgrounds and Borders defines `background-size` as
/// `cover | contain | <bg-size>#`; this parser covers the single-layer subset
/// currently supported by the renderer:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>.
pub(crate) fn parse_background_size(value: &str, font_size: f32) -> Option<BackgroundSize> {
    let value = trim_css_value(value);
    if css_identifier_is(value, "auto") {
        return Some(BackgroundSize::Auto);
    }
    if css_identifier_is(value, "cover") {
        return Some(BackgroundSize::Cover);
    }
    if css_identifier_is(value, "contain") {
        return Some(BackgroundSize::Contain);
    }
    let parts = background_components(value)?;
    let (width, height) = match parts.as_slice() {
        [width] => (
            parse_background_size_axis(width, font_size)?,
            BackgroundSizeAxis::Auto,
        ),
        [width, height] => (
            parse_background_size_axis(width, font_size)?,
            parse_background_size_axis(height, font_size)?,
        ),
        _ => return None,
    };
    Some(BackgroundSize::Explicit { width, height })
}

/// Parses one `background-size` axis.
///
/// CSS Backgrounds and Borders uses `auto | <length-percentage [0,∞]>` for
/// explicit background-size axes:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>.
fn parse_background_size_axis(value: &str, font_size: f32) -> Option<BackgroundSizeAxis> {
    let value = trim_css_value(value);
    if css_identifier_is(value, "auto") {
        Some(BackgroundSizeAxis::Auto)
    } else {
        let value = parse_computed_length_percentage(value, font_size)?;
        // A length-percentage that does not depend on a later percentage
        // basis is fully resolved at computed-value time, where the
        // non-negative range is enforced. Expressions with a percentage are
        // intentionally retained: their range depends on the positioning area
        // and therefore cannot yet be determined.
        if value
            .length_if_no_percent()
            .is_some_and(|length| length < 0.0)
        {
            return None;
        }
        Some(BackgroundSizeAxis::LengthPercentage(value))
    }
}

/// Parses `background-position` for a single background layer.
///
/// CSS Backgrounds and Borders defines one-to-four value positioning syntax.
/// This parser preserves the subset already supported by layout: side
/// keywords, `center`, and one offset following a side keyword:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>.
pub(crate) fn parse_background_position(value: &str, font_size: f32) -> Option<BackgroundPosition> {
    let tokens = background_components(trim_css_value(value))?;
    if tokens.is_empty() {
        return None;
    }
    // The common one/two-value numeric grammar assigns the first offset to
    // the horizontal axis and the optional second offset to the vertical
    // axis. A sole numeric value centers the other axis.
    // <https://www.w3.org/TR/css-backgrounds-3/#background-position>
    if tokens.len() <= 2
        && tokens
            .iter()
            .all(|token| parse_computed_length_percentage(token, font_size).is_some())
    {
        let x = parse_computed_length_percentage(tokens[0], font_size)?;
        let y = if let Some(value) = tokens.get(1) {
            BackgroundPositionAxis {
                origin: BackgroundPositionOrigin::Start,
                offset: parse_computed_length_percentage(value, font_size)?,
            }
        } else {
            BackgroundPositionAxis {
                origin: BackgroundPositionOrigin::Center,
                offset: ComputedLengthPercentage::ZERO,
            }
        };
        return Some(BackgroundPosition {
            x: BackgroundPositionAxis {
                origin: BackgroundPositionOrigin::Start,
                offset: x,
            },
            y,
        });
    }
    if !tokens.iter().all(|token| {
        matches!(
            css_single_ident(token).as_deref(),
            Some("left" | "right" | "top" | "bottom" | "center")
        ) || parse_computed_length_percentage(token, font_size).is_some()
    }) {
        return None;
    }
    let mut position = BackgroundPosition::INITIAL;
    if tokens
        .iter()
        .any(|token| css_identifier_is(token, "center"))
    {
        position.x.origin = BackgroundPositionOrigin::Center;
        position.y.origin = BackgroundPositionOrigin::Center;
    }
    if tokens.iter().any(|token| css_identifier_is(token, "right")) {
        position.x.origin = BackgroundPositionOrigin::End;
    } else if tokens.iter().any(|token| css_identifier_is(token, "left")) {
        position.x.origin = BackgroundPositionOrigin::Start;
    }
    if tokens
        .iter()
        .any(|token| css_identifier_is(token, "bottom"))
    {
        position.y.origin = BackgroundPositionOrigin::End;
    } else if tokens.iter().any(|token| css_identifier_is(token, "top")) {
        position.y.origin = BackgroundPositionOrigin::Start;
    }
    for pair in tokens.windows(2) {
        if css_identifier_is(pair[0], "left") || css_identifier_is(pair[0], "right") {
            if let Some(offset) = parse_computed_length_percentage(pair[1], font_size) {
                position.x.offset = offset;
            }
        } else if (css_identifier_is(pair[0], "top") || css_identifier_is(pair[0], "bottom"))
            && let Some(offset) = parse_computed_length_percentage(pair[1], font_size)
        {
            position.y.offset = offset;
        }
    }
    Some(position)
}

/// Parses one axis of `background-position-x` or `background-position-y`.
///
/// The longhands accept a length-percentage, `center`, or the axis's start/end
/// side with an optional offset. Keeping this independent from the shorthand
/// parser prevents a one-value `background-position-y` from being interpreted
/// as an x-axis value:
/// <https://www.w3.org/TR/css-backgrounds-4/#background-position>.
fn parse_background_position_axis(
    value: &str,
    font_size: f32,
    horizontal: bool,
) -> Option<BackgroundPositionAxis> {
    let tokens = background_components(trim_css_value(value))?;
    let (start, end) = if horizontal {
        ("left", "right")
    } else {
        ("top", "bottom")
    };
    match tokens.as_slice() {
        [value] => {
            if let Some(offset) = parse_computed_length_percentage(value, font_size) {
                return Some(BackgroundPositionAxis {
                    origin: BackgroundPositionOrigin::Start,
                    offset,
                });
            }
            let origin = match css_single_ident(value)?.to_ascii_lowercase().as_str() {
                "center" => BackgroundPositionOrigin::Center,
                value if value == start => BackgroundPositionOrigin::Start,
                value if value == end => BackgroundPositionOrigin::End,
                _ => return None,
            };
            Some(BackgroundPositionAxis {
                origin,
                offset: ComputedLengthPercentage::ZERO,
            })
        }
        [side, offset] => {
            let offset = parse_computed_length_percentage(offset, font_size)?;
            let origin = match css_single_ident(side)?.to_ascii_lowercase().as_str() {
                value if value == start => BackgroundPositionOrigin::Start,
                "center" => BackgroundPositionOrigin::Center,
                value if value == end => BackgroundPositionOrigin::End,
                _ => return None,
            };
            Some(BackgroundPositionAxis { origin, offset })
        }
        _ => None,
    }
}

/// Parses a single supported CSS background image.
///
/// CSS Backgrounds delegates image values to CSS Images. This parser supports
/// URL images and CSS Images Level 3 linear/radial gradients as generated images:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-image> and
/// <https://www.w3.org/TR/css-images-3/#gradients>.
/// Result of parsing a CSS `<image>` value at a property boundary.
///
/// `Invalid` is deliberately distinct from syntax failure: CSS Images says an
/// exhausted `image-set()` remains a valid value that represents an invalid
/// image.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedImage {
    NotAnImage,
    SyntaxError,
    Image(ComputedImage),
}

/// Parse one complete CSS image value while retaining computed invalid-image
/// state for every property that accepts `<image>`.
pub(crate) fn parse_css_image(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> ParsedImage {
    match parse_image_set(value, base_url, root_url) {
        ParsedImage::NotAnImage => {}
        image => return image,
    }
    parse_css_image_without_image_set(value, base_url, root_url)
}

/// Parse a supported concrete `<image>` while deliberately excluding
/// `image-set()`. Image-set candidates use this entry point so a set cannot
/// directly or indirectly contain another image-set.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
fn parse_css_image_without_image_set(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> ParsedImage {
    if let Some(gradient) = parse_conic_gradient(value) {
        return ParsedImage::Image(ComputedImage::image(BackgroundImage::ConicGradient(
            gradient,
        )));
    }
    if let Some(color) = parse_color_image(value) {
        return ParsedImage::Image(ComputedImage::image(BackgroundImage::CssColor(color)));
    }
    if let Some(gradient) = parse_linear_gradient(value) {
        return ParsedImage::Image(ComputedImage::image(BackgroundImage::LinearGradient(
            gradient,
        )));
    }
    if let Some(gradient) = parse_radial_gradient(value) {
        return ParsedImage::Image(ComputedImage::image(BackgroundImage::RadialGradient(
            gradient,
        )));
    }
    let Some(url) = parse_first_css_url_with_modifiers(value) else {
        return ParsedImage::NotAnImage;
    };
    ParsedImage::Image(ComputedImage::image(BackgroundImage::Url {
        src: url.src,
        base_url: base_url.cloned(),
        root_url: root_url.cloned(),
        request_modifiers: url.modifiers,
    }))
}

/// Parse a concrete image for legacy consumers that cannot represent invalid
/// images. Property parsers must use [`parse_css_image`] instead.
pub(crate) fn parse_background_image(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<BackgroundImage> {
    match parse_css_image(value, base_url, root_url) {
        ParsedImage::Image(ComputedImage::Image(image)) => Some(*image),
        ParsedImage::NotAnImage | ParsedImage::SyntaxError | ParsedImage::Image(_) => None,
    }
}

/// Parse CSS Images Level 4's `image(<color>)` subset. A color image has no
/// intrinsic dimensions, so it participates in background sizing like every
/// other generated image.
/// <https://drafts.csswg.org/css-images-4/#image-notation>
fn parse_color_image(value: &str) -> Option<ColorImageColor> {
    let (argument, tail) = css_leading_function_matching(trim_css_value(value), "image")?;
    tail.trim().is_empty().then_some(())?;
    if argument.trim().eq_ignore_ascii_case("currentcolor") {
        Some(ColorImageColor::CurrentColor)
    } else {
        parse_color(argument.trim()).map(ColorImageColor::CssColor)
    }
}

/// Parse CSS Images Level 4 conic gradients into their angular color-line.
/// <https://drafts.csswg.org/css-images-4/#conic-gradients>
fn parse_conic_gradient(value: &str) -> Option<ConicGradient> {
    let value = trim_css_value(value);
    let (repeating, parts) =
        gradient_arguments(value, "conic-gradient", "repeating-conic-gradient")?;
    let (start_angle, position, interpolation, first_stop) =
        parse_conic_prelude(parts.first()?.trim())?;
    let stop_parts = if first_stop {
        &parts[1..]
    } else {
        parts.as_slice()
    };
    let mut stops = Vec::new();
    for part in stop_parts {
        stops.extend(parse_conic_gradient_stop(part.trim())?);
    }
    (!stops.is_empty()).then_some(())?;
    Some(ConicGradient {
        start_angle,
        position,
        interpolation,
        repeating,
        stops,
    })
}

fn parse_conic_prelude(
    value: &str,
) -> Option<(f32, BackgroundPosition, GradientInterpolationMethod, bool)> {
    let (interpolation, value) = split_gradient_interpolation_method(value)?;
    let tokens = background_components(&value)?;
    let Some(first) = tokens.first() else {
        return interpolation.map(|method| (0.0, radial_gradient_center_position(), method, true));
    };
    if interpolation.is_none() && parse_gradient_color(first).is_some() {
        return Some((
            0.0,
            radial_gradient_center_position(),
            GradientInterpolationMethod::CSS_IMAGES_3,
            false,
        ));
    }
    let mut angle = 0.0;
    let mut position = radial_gradient_center_position();
    if let Some(from) = tokens
        .iter()
        .position(|token| css_identifier_is(token, "from"))
        && let Some(value) = tokens
            .get(from + 1)
            .and_then(|value| parse_css_angle_degrees(value))
    {
        angle = value;
    }
    if let Some(at) = tokens
        .iter()
        .position(|token| css_identifier_is(token, "at"))
    {
        let value = tokens[at + 1..].join(" ");
        if let Some(parsed) = parse_radial_gradient_position(&value) {
            position = parsed;
        }
    }
    Some((
        angle,
        position,
        interpolation.unwrap_or(GradientInterpolationMethod::CSS_IMAGES_3),
        true,
    ))
}

fn parse_conic_gradient_stop(value: &str) -> Option<Vec<ConicGradientStop>> {
    let tokens = background_components(value)?;
    for split in (1..=tokens.len()).rev() {
        let Some(color) = parse_gradient_color(&tokens[..split].join(" ")) else {
            continue;
        };
        let positions = tokens[split..]
            .iter()
            .map(|value| parse_conic_angle_percentage(value))
            .collect::<Option<Vec<_>>>()?;
        if positions.len() <= 2 {
            return Some(match positions.as_slice() {
                [] => vec![ConicGradientStop {
                    color,
                    position: None,
                }],
                [position] => vec![ConicGradientStop {
                    color,
                    position: Some(*position),
                }],
                [first, second] => vec![
                    ConicGradientStop {
                        color,
                        position: Some(*first),
                    },
                    ConicGradientStop {
                        color,
                        position: Some(*second),
                    },
                ],
                _ => unreachable!(),
            });
        }
    }
    None
}

fn parse_conic_angle_percentage(value: &str) -> Option<f32> {
    parse_css_angle_degrees(value).or_else(|| {
        value
            .strip_suffix('%')?
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| value * 3.6)
    })
}

/// Parse a CSS `image-set()` image while preserving the distinction between a
/// malformed value and a valid set for which the selected rendering
/// environment supports no image source. Candidate negotiation happens after
/// cascade, not while parsing a declaration.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
fn parse_image_set(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> ParsedImage {
    let value = trim_css_value(value);
    let Some((name, body, tail)) = crate::css::component_values::css_leading_function(value) else {
        return ParsedImage::NotAnImage;
    };
    if !name.eq_ignore_ascii_case("image-set") && !name.eq_ignore_ascii_case("-webkit-image-set") {
        return ParsedImage::NotAnImage;
    }
    if !tail.trim().is_empty() {
        return ParsedImage::SyntaxError;
    }
    let mut options = Vec::new();
    for option in crate::css::component_values::split_css_top_level_delimiter(body, ',') {
        match parse_image_set_option(option, base_url, root_url) {
            Ok(option) => options.push(option),
            Err(()) => return ParsedImage::SyntaxError,
        }
    }
    if options.is_empty() {
        return ParsedImage::SyntaxError;
    }
    ParsedImage::Image(ComputedImage::image(BackgroundImage::ImageSet(ImageSet {
        options,
    })))
}

fn parse_image_set_option(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Result<ImageSetOption, ()> {
    let value = trim_css_value(value);
    let (image, tail) = parse_image_set_option_image(value, base_url, root_url).ok_or(())?;
    let (resolution_descriptor, mime_type) = parse_image_set_option_descriptors(tail).ok_or(())?;
    let resolution = parse_image_set_resolution(resolution_descriptor).ok_or(())?;
    if !resolution.is_finite() {
        return Err(());
    }
    if resolution < 0.0 && css_leading_function_matching(resolution_descriptor, "calc").is_none() {
        // Literal negative values are outside the grammar. Calculated
        // non-positive values are valid syntax but do not define an option.
        return Err(());
    }
    Ok(ImageSetOption {
        image: Box::new(image),
        resolution_dppx: resolution,
        mime_type,
    })
}

/// Split the optional resolution and `type()` descriptors of one
/// `image-set()` option. The descriptors are an unordered pair, but each may
/// occur at most once.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
fn parse_image_set_option_descriptors(value: &str) -> Option<(&str, Option<String>)> {
    let value = value.trim();
    if value.is_empty() {
        return Some(("", None));
    }
    let tokens = split_css_component_values(value);
    let mut resolution = None;
    let mut mime_type = None;
    for token in tokens {
        if let Some((name, body, tail)) = crate::css::component_values::css_leading_function(token)
            && name.eq_ignore_ascii_case("type")
        {
            if mime_type.is_some() {
                return None;
            }
            if !tail.trim().is_empty() {
                return None;
            }
            let (mime, tail) = parse_css_string_token(body.trim())?;
            if !tail.trim().is_empty() {
                return None;
            }
            mime_type = Some(mime);
        } else if resolution.replace(token).is_some() {
            return None;
        }
    }
    Some((resolution.unwrap_or(""), mime_type))
}

fn parse_image_set_option_image<'a>(
    value: &'a str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<(BackgroundImage, &'a str)> {
    if let Some((url, tail)) = parse_css_url_token_with_modifiers(value) {
        return Some((
            BackgroundImage::Url {
                src: url.src,
                base_url: base_url.cloned(),
                root_url: root_url.cloned(),
                request_modifiers: url.modifiers,
            },
            tail,
        ));
    }
    if let Some((url, tail)) = parse_css_string_token(value) {
        return Some((
            BackgroundImage::Url {
                src: url,
                base_url: base_url.cloned(),
                root_url: root_url.cloned(),
                request_modifiers: RequestUrlModifiers::default(),
            },
            tail,
        ));
    }
    let (name, arguments, tail) = crate::css::component_values::css_leading_function(value)?;
    if [
        "image",
        "linear-gradient",
        "repeating-linear-gradient",
        "radial-gradient",
        "repeating-radial-gradient",
        "conic-gradient",
        "repeating-conic-gradient",
    ]
    .iter()
    .any(|known| name.eq_ignore_ascii_case(known))
    {
        let ParsedImage::Image(ComputedImage::Image(image)) =
            parse_css_image_without_image_set(&format!("{name}({arguments})"), base_url, root_url)
        else {
            return None;
        };
        return Some((*image, tail));
    }
    None
}

fn parse_image_set_resolution(value: &str) -> Option<f32> {
    if value.is_empty() {
        // A missing descriptor computes to 1x.
        // <https://drafts.csswg.org/css-images-4/#image-set-notation>
        return Some(1.0);
    }
    let calculated =
        crate::css::component_values::css_leading_function(value).is_some_and(|(name, _, tail)| {
            tail.trim().is_empty()
                && matches!(
                    name.to_ascii_lowercase().as_str(),
                    "calc" | "min" | "max" | "clamp" | "sign"
                )
        });
    let resolution = crate::css::values::parse_math_resolution(value)?;
    // CSS Values range checking happens after a top-level calculation. A
    // literal negative `<resolution>` is syntax-invalid, whereas a calculated
    // negative value computes to the closed lower bound of zero.
    // <https://drafts.csswg.org/css-values-4/#calc-range>
    if !calculated && resolution < 0.0 {
        return None;
    }
    Some(if calculated {
        resolution.max(0.0)
    } else {
        resolution
    })
}

pub(super) fn sync_background_layers_from_single_fields(style: &mut ComputedStyle) {
    let layer = layer_from_single_fields(style);
    style.background.background_layers = vec![layer];
    style.background.background_image_layer_count = 1;
}

pub(super) fn sync_background_single_fields_from_layers(style: &mut ComputedStyle) {
    let Some(layer) = style.background.background_layers.first().cloned() else {
        style.background.background_image = ComputedImage::None;
        style.background.background_size = BackgroundSize::AUTO;
        style.background.background_position = BackgroundPosition::INITIAL;
        style.background.background_repeat = BackgroundRepeat::Repeat;
        style.background.background_origin = BackgroundBox::Padding;
        style.background.background_clip = BackgroundBox::Border;
        return;
    };
    style.background.background_image = layer.image;
    style.background.background_size = layer.size;
    style.background.background_position = layer.position;
    style.background.background_repeat = layer.repeat;
    style.background.background_attachment = layer.attachment;
    style.background.background_origin = layer.origin;
    style.background.background_clip = layer.clip;
}

/// Resolve all list-valued background longhands to the number of layers in
/// `background-image` after the cascade has selected every winning declaration.
///
/// The declarations may be encountered in any source order, so this is the
/// one point where a longer `background-clip`, `background-size`, or related
/// list is safely truncated without losing a later `background-image` value.
/// <https://www.w3.org/TR/css-backgrounds-3/#layering>
pub(super) fn normalize_background_layers(style: &mut ComputedStyle) {
    // CSS-wide defaulting can update a list-valued background longhand before
    // any ordinary background declaration has materialized the initial
    // `background-image: none` layer. Preserve that layer before syncing the
    // single-field compatibility view; otherwise sync would replace an
    // inherited `background-clip` (or any sibling longhand) with its initial
    // value. `none` still establishes one background layer.
    // <https://www.w3.org/TR/css-backgrounds-3/#layering>
    if style.background.background_layers.is_empty() {
        let layer = layer_from_single_fields(style);
        style.background.background_layers.push(layer);
    }
    let layer_count = style.background.background_image_layer_count.max(1);
    style.background.background_layers.truncate(layer_count);
    sync_background_single_fields_from_layers(style);
}

fn layer_from_single_fields(style: &ComputedStyle) -> BackgroundLayer {
    BackgroundLayer {
        image: style.background.background_image.clone(),
        position: style.background.background_position.clone(),
        size: style.background.background_size.clone(),
        repeat: style.background.background_repeat,
        attachment: style.background.background_attachment,
        origin: style.background.background_origin,
        clip: style.background.background_clip,
    }
}

fn ensure_background_layer_count(style: &mut ComputedStyle, count: usize) {
    if count == 0 {
        return;
    }
    if style.background.background_layers.is_empty() {
        let layer = layer_from_single_fields(style);
        style.background.background_layers.push(layer);
    }
    while style.background.background_layers.len() < count {
        let layer = style
            .background
            .background_layers
            .last()
            .cloned()
            .unwrap_or_else(BackgroundLayer::initial);
        style.background.background_layers.push(layer);
    }
}

fn repeated_layer_value<T: Clone>(values: &[T], index: usize) -> T {
    values[index % values.len()].clone()
}

fn parse_background_image_layers(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<Vec<ComputedImage>> {
    let mut images = Vec::new();
    for part in background_layers(value)? {
        let image = if css_identifier_is(trim_css_value(part), "none") {
            ComputedImage::None
        } else {
            match parse_css_image(part, base_url, root_url) {
                ParsedImage::Image(image) => image,
                ParsedImage::NotAnImage | ParsedImage::SyntaxError => return None,
            }
        };
        images.push(image);
    }
    Some(images)
}

fn parse_background_shorthand_layers(
    value: &str,
    font_size: f32,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<Vec<BackgroundLayer>> {
    background_layers(value)?
        .into_iter()
        .map(|part| {
            background_shorthand_layer_is_valid(part, font_size)?;
            let mut layer = BackgroundLayer::initial();
            layer.image = match parse_background_shorthand_image(part, base_url, root_url) {
                ParsedImage::Image(image) => image,
                ParsedImage::NotAnImage => ComputedImage::None,
                ParsedImage::SyntaxError => return None,
            };
            layer.repeat = parse_background_repeat(part).unwrap_or(BackgroundRepeat::Repeat);
            layer.attachment = background_tokens(part)?
                .into_iter()
                .find_map(
                    |token| match css_single_ident(token)?.to_ascii_lowercase().as_str() {
                        "scroll" => Some(BackgroundAttachment::Scroll),
                        "fixed" => Some(BackgroundAttachment::Fixed),
                        "local" => Some(BackgroundAttachment::Local),
                        _ => None,
                    },
                )
                .unwrap_or(BackgroundAttachment::Scroll);
            let (position, size) = match split_background_position_size(part)? {
                Some((position, size)) => (position, Some(size)),
                None => (strip_background_noise(part)?, None),
            };
            if !position.trim().is_empty()
                && let Some(position) = parse_background_position(position.trim(), font_size)
            {
                layer.position = position;
            }
            if let Some(size) = size
                && !size.trim().is_empty()
                && let Some(size) = parse_background_size(size.trim(), font_size)
            {
                layer.size = size;
            }
            let boxes = background_tokens(part)?
                .into_iter()
                .filter_map(parse_background_box)
                .collect::<Vec<_>>();
            match boxes.as_slice() {
                [BackgroundBox::BorderArea] => layer.clip = BackgroundBox::BorderArea,
                [box_] => {
                    layer.origin = *box_;
                    layer.clip = *box_;
                }
                [origin, clip, ..] => {
                    layer.origin = *origin;
                    layer.clip = *clip;
                }
                [] => {}
            }
            Some(layer)
        })
        .collect::<Option<Vec<_>>>()
}

fn parse_background_shorthand_image(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> ParsedImage {
    let Some(components) = background_components(value) else {
        return ParsedImage::SyntaxError;
    };
    let mut image = None;
    for component in components {
        match parse_css_image(component, base_url, root_url) {
            ParsedImage::Image(candidate) => {
                if image.replace(candidate).is_some() {
                    return ParsedImage::SyntaxError;
                }
            }
            ParsedImage::NotAnImage => {}
            ParsedImage::SyntaxError => return ParsedImage::SyntaxError,
        }
    }
    image.map_or(ParsedImage::NotAnImage, ParsedImage::Image)
}

/// Validate the still-unmodeled parts of a background layer before the
/// shorthand resets any of its longhands. Existing component parsers provide
/// the grammar checks; this ensures stray identifiers are not treated as an
/// otherwise-valid color-only layer.
fn background_shorthand_layer_is_valid(value: &str, font_size: f32) -> Option<()> {
    let (position, size) = match split_background_position_size(value)? {
        Some((position, size)) => (position, Some(size)),
        None => (strip_background_noise(value)?, None),
    };
    if !position.trim().is_empty() && parse_background_position(&position, font_size).is_none() {
        return None;
    }
    if let Some(size) = size
        && !size.trim().is_empty()
        && parse_background_size(&size, font_size).is_none()
    {
        return None;
    }
    Some(())
}

fn background_layers(value: &str) -> Option<Vec<&str>> {
    let layers = try_split_css_top_level_delimiter(value, ',')?;
    (!layers.is_empty() && layers.iter().all(|layer| !trim_css_value(layer).is_empty()))
        .then_some(layers)
}

fn background_components(value: &str) -> Option<Vec<&str>> {
    try_split_css_component_values(value)
}

fn css_identifier_is(value: &str, expected: &str) -> bool {
    css_single_ident(value).is_some_and(|ident| ident.eq_ignore_ascii_case(expected))
}

/// Parses CSS Images Level 3 `linear-gradient()` and
/// `repeating-linear-gradient()`.
///
/// The parser accepts the Level 3 direction grammar, color stops, two-position
/// stops, omitted stop positions, and interpolation hints. Stop-position fixup
/// runs at paint time because percentages resolve against the concrete
/// gradient line:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
pub(crate) fn parse_linear_gradient(value: &str) -> Option<LinearGradient> {
    let (repeating, parts) =
        gradient_arguments(value, "linear-gradient", "repeating-linear-gradient")?;
    let mut first_stop = 0usize;
    let (direction, interpolation) = match parse_linear_gradient_prelude(parts[0].trim())? {
        Some(prelude) => {
            first_stop = 1;
            prelude
        }
        None => (
            LinearGradientDirection::Angle(180.0),
            GradientInterpolationMethod::CSS_IMAGES_3,
        ),
    };
    let mut stops = Vec::new();
    let mut hints = Vec::new();
    for part in &parts[first_stop..] {
        parse_gradient_item(part.trim(), &mut stops, &mut hints)?;
    }
    if stops.is_empty() {
        return None;
    }
    // CSS Images defines the color line of a one-stop gradient by extending
    // that stop across both endpoints.  Represent it as two independent
    // stops so the shared color-stop fixup and raster/vector paint paths keep
    // their ordinary two-endpoint invariants.
    // <https://drafts.csswg.org/css-images-3/#color-stop-fixup>
    if stops.len() == 1 {
        stops.push(stops[0].clone());
    }
    if hints.iter().any(|hint| hint.after_stop + 1 >= stops.len()) {
        return None;
    }
    Some(LinearGradient {
        direction,
        interpolation,
        repeating,
        stops,
        hints,
    })
}

/// Parses CSS Images Level 3 `radial-gradient()` and
/// `repeating-radial-gradient()`.
///
/// This covers the Level 3 shape, extent keyword, explicit radius/radii,
/// `at <position>`, color-stop, two-position stop, and interpolation hint
/// grammar. Radius percentages remain unresolved until paint, where the
/// concrete gradient box is known:
/// <https://www.w3.org/TR/css-images-3/#radial-gradients>.
pub(crate) fn parse_radial_gradient(value: &str) -> Option<RadialGradient> {
    let (repeating, parts) =
        gradient_arguments(value, "radial-gradient", "repeating-radial-gradient")?;
    let mut first_stop = 0usize;
    let (shape, size, position, interpolation) = match parse_radial_gradient_prelude(parts[0])? {
        Some(prelude) => {
            first_stop = 1;
            prelude
        }
        None => (
            RadialGradientShape::Ellipse,
            RadialGradientSize::Extent(RadialGradientExtent::FarthestCorner),
            radial_gradient_center_position(),
            GradientInterpolationMethod::CSS_IMAGES_3,
        ),
    };
    let mut stops = Vec::new();
    let mut hints = Vec::new();
    for part in &parts[first_stop..] {
        parse_gradient_item(part.trim(), &mut stops, &mut hints)?;
    }
    if stops.is_empty() {
        return None;
    }
    if stops.len() == 1 {
        stops.push(stops[0].clone());
    }
    if hints.iter().any(|hint| hint.after_stop + 1 >= stops.len()) {
        return None;
    }
    Some(RadialGradient {
        shape,
        size,
        position,
        interpolation,
        repeating,
        stops,
        hints,
    })
}

fn parse_radial_gradient_prelude(
    value: &str,
) -> Option<
    Option<(
        RadialGradientShape,
        RadialGradientSize,
        BackgroundPosition,
        GradientInterpolationMethod,
    )>,
> {
    let (interpolation, value) = split_gradient_interpolation_method(value)?;
    if interpolation.is_none() && parse_color(&value).is_some() {
        return Some(None);
    }
    let (size_text, position) = split_radial_gradient_position(&value)?;
    let (shape, size) = parse_radial_gradient_shape_and_size(&size_text)?;
    Some(Some((
        shape,
        size,
        position,
        interpolation.unwrap_or(GradientInterpolationMethod::CSS_IMAGES_3),
    )))
}

fn split_radial_gradient_position(value: &str) -> Option<(String, BackgroundPosition)> {
    let tokens = background_components(value)?;
    let Some(at_index) = tokens
        .iter()
        .position(|token| css_identifier_is(token, "at"))
    else {
        return Some((value.to_string(), radial_gradient_center_position()));
    };
    let before = tokens[..at_index].join(" ");
    let after = tokens[at_index + 1..].join(" ");
    if after.trim().is_empty() {
        return None;
    }
    let position = parse_radial_gradient_position(&after)?;
    Some((before, position))
}

fn parse_radial_gradient_shape_and_size(
    value: &str,
) -> Option<(RadialGradientShape, RadialGradientSize)> {
    let tokens = background_components(value)?;
    if tokens.is_empty() {
        return Some((
            RadialGradientShape::Ellipse,
            RadialGradientSize::Extent(RadialGradientExtent::FarthestCorner),
        ));
    }
    let mut shape = None;
    let mut extent = None;
    let mut lengths = Vec::new();
    for token in tokens {
        let keyword = css_single_ident(token).map(|keyword| keyword.to_ascii_lowercase());
        match keyword.as_deref() {
            Some("circle") if shape.is_none() => shape = Some(RadialGradientShape::Circle),
            Some("ellipse") if shape.is_none() => shape = Some(RadialGradientShape::Ellipse),
            Some("closest-side") if extent.is_none() => {
                extent = Some(RadialGradientExtent::ClosestSide);
            }
            Some("farthest-side") if extent.is_none() => {
                extent = Some(RadialGradientExtent::FarthestSide);
            }
            Some("closest-corner") if extent.is_none() => {
                extent = Some(RadialGradientExtent::ClosestCorner);
            }
            Some("farthest-corner") if extent.is_none() => {
                extent = Some(RadialGradientExtent::FarthestCorner);
            }
            _ => lengths.push(parse_deferred_length_percentage(token)?),
        }
    }
    if extent.is_some() && !lengths.is_empty() {
        return None;
    }
    let shape = shape.unwrap_or(if lengths.len() == 1 {
        RadialGradientShape::Circle
    } else {
        RadialGradientShape::Ellipse
    });
    let size = if let Some(extent) = extent {
        RadialGradientSize::Extent(extent)
    } else {
        match lengths.as_slice() {
            [] => RadialGradientSize::Extent(RadialGradientExtent::FarthestCorner),
            [radius] if shape == RadialGradientShape::Circle => {
                RadialGradientSize::CircleRadius(radius.clone())
            }
            [x, y] if shape == RadialGradientShape::Ellipse => RadialGradientSize::EllipseRadii {
                x: x.clone(),
                y: y.clone(),
            },
            _ => return None,
        }
    };
    Some((shape, size))
}

fn parse_radial_gradient_position(value: &str) -> Option<BackgroundPosition> {
    let tokens = background_components(value)?;
    let lengths = tokens
        .iter()
        .map(|token| parse_deferred_length_percentage(token))
        .collect::<Option<Vec<_>>>();
    match lengths.as_deref() {
        Some([x]) => {
            return Some(BackgroundPosition {
                x: BackgroundPositionAxis {
                    origin: BackgroundPositionOrigin::Start,
                    offset: x.clone(),
                },
                y: BackgroundPositionAxis {
                    origin: BackgroundPositionOrigin::Center,
                    offset: ComputedLengthPercentage::ZERO,
                },
            });
        }
        Some([x, y]) => {
            return Some(BackgroundPosition {
                x: BackgroundPositionAxis {
                    origin: BackgroundPositionOrigin::Start,
                    offset: x.clone(),
                },
                y: BackgroundPositionAxis {
                    origin: BackgroundPositionOrigin::Start,
                    offset: y.clone(),
                },
            });
        }
        _ => {}
    }
    parse_background_position(value, ROOT_FONT_SIZE_PT)
}

fn radial_gradient_center_position() -> BackgroundPosition {
    BackgroundPosition {
        x: BackgroundPositionAxis {
            origin: BackgroundPositionOrigin::Center,
            offset: ComputedLengthPercentage::ZERO,
        },
        y: BackgroundPositionAxis {
            origin: BackgroundPositionOrigin::Center,
            offset: ComputedLengthPercentage::ZERO,
        },
    }
}

fn parse_linear_gradient_direction(value: &str) -> Option<LinearGradientDirection> {
    let value = trim_css_value(value);
    if let Some(angle) = parse_css_angle_degrees(value) {
        return Some(LinearGradientDirection::Angle(angle));
    }
    let tokens = background_components(value)?;
    let first = tokens.first()?;
    if !css_identifier_is(first, "to") {
        return None;
    }
    let rest = &tokens[1..];
    if rest.is_empty() || rest.len() > 2 {
        return None;
    }
    let mut horizontal = None;
    let mut vertical = None;
    for token in rest {
        match css_single_ident(token)?.to_ascii_lowercase().as_str() {
            "left" if horizontal.is_none() => horizontal = Some(GradientHorizontalDirection::Left),
            "right" if horizontal.is_none() => {
                horizontal = Some(GradientHorizontalDirection::Right);
            }
            "top" if vertical.is_none() => vertical = Some(GradientVerticalDirection::Top),
            "bottom" if vertical.is_none() => vertical = Some(GradientVerticalDirection::Bottom),
            _ => return None,
        }
    }
    match (horizontal, vertical) {
        (None, Some(GradientVerticalDirection::Top)) => Some(LinearGradientDirection::Angle(0.0)),
        (Some(GradientHorizontalDirection::Right), None) => {
            Some(LinearGradientDirection::Angle(90.0))
        }
        (None, Some(GradientVerticalDirection::Bottom)) => {
            Some(LinearGradientDirection::Angle(180.0))
        }
        (Some(GradientHorizontalDirection::Left), None) => {
            Some(LinearGradientDirection::Angle(270.0))
        }
        (Some(horizontal), Some(vertical)) => Some(LinearGradientDirection::Corner {
            horizontal,
            vertical,
        }),
        _ => None,
    }
}

/// Split CSS Images 4's unordered `in <color-space>` production from a
/// gradient prelude. The remaining top-level tokens are interpreted by the
/// gradient-specific geometry parser.
/// <https://drafts.csswg.org/css-color-4/#interpolation>
fn split_gradient_interpolation_method(
    value: &str,
) -> Option<(Option<GradientInterpolationMethod>, String)> {
    let tokens = background_components(value)?;
    let in_positions = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| css_identifier_is(token, "in").then_some(index))
        .collect::<Vec<_>>();
    if in_positions.is_empty() {
        return Some((None, value.trim().to_string()));
    }
    if in_positions.len() != 1 {
        return None;
    }
    let index = in_positions[0];
    let space = css_single_ident(tokens.get(index + 1)?)?.to_ascii_lowercase();
    let space = match space.as_str() {
        "srgb" => GradientInterpolationSpace::Srgb,
        "srgb-linear" => GradientInterpolationSpace::SrgbLinear,
        "display-p3" => GradientInterpolationSpace::DisplayP3,
        "display-p3-linear" => GradientInterpolationSpace::DisplayP3Linear,
        "a98-rgb" => GradientInterpolationSpace::A98Rgb,
        "prophoto-rgb" => GradientInterpolationSpace::ProphotoRgb,
        "rec2020" => GradientInterpolationSpace::Rec2020,
        // CSS CssColor aliases `xyz` to the D65-referenced predefined space.
        "xyz-d50" => GradientInterpolationSpace::XyzD50,
        "xyz" | "xyz-d65" => GradientInterpolationSpace::XyzD65,
        "lab" => GradientInterpolationSpace::Lab,
        "oklab" => GradientInterpolationSpace::Oklab,
        "hsl" => GradientInterpolationSpace::Hsl,
        "hwb" => GradientInterpolationSpace::Hwb,
        "lch" => GradientInterpolationSpace::Lch,
        "oklch" => GradientInterpolationSpace::Oklch,
        _ => return None,
    };
    let mut consumed = 2;
    let hue = if space.is_polar()
        && let (Some(method), Some(keyword)) = (tokens.get(index + 2), tokens.get(index + 3))
        && css_identifier_is(keyword, "hue")
    {
        consumed += 2;
        match css_single_ident(method)?.to_ascii_lowercase().as_str() {
            "shorter" => HueInterpolationMethod::Shorter,
            "longer" => HueInterpolationMethod::Longer,
            "increasing" => HueInterpolationMethod::Increasing,
            "decreasing" => HueInterpolationMethod::Decreasing,
            _ => return None,
        }
    } else {
        HueInterpolationMethod::Shorter
    };
    // `hue` is only legal for polar spaces; leave no stray interpolation
    // tokens for a geometry parser to accidentally accept.
    if !space.is_polar()
        && tokens.get(index + 2).is_some_and(|token| {
            matches!(
                css_single_ident(token).as_deref(),
                Some("shorter" | "longer" | "increasing" | "decreasing" | "hue")
            )
        })
    {
        return None;
    }
    let mut remaining = Vec::with_capacity(tokens.len().saturating_sub(consumed));
    for (token_index, token) in tokens.into_iter().enumerate() {
        if token_index < index || token_index >= index + consumed {
            remaining.push(token);
        }
    }
    Some((
        Some(GradientInterpolationMethod { space, hue }),
        remaining.join(" "),
    ))
}

fn parse_linear_gradient_prelude(
    value: &str,
) -> Option<Option<(LinearGradientDirection, GradientInterpolationMethod)>> {
    let (interpolation, geometry) = split_gradient_interpolation_method(value)?;
    if let Some(interpolation) = interpolation {
        let direction = if geometry.trim().is_empty() {
            LinearGradientDirection::Angle(180.0)
        } else {
            parse_linear_gradient_direction(&geometry)?
        };
        return Some(Some((direction, interpolation)));
    }
    Some(
        parse_linear_gradient_direction(&geometry)
            .map(|direction| (direction, GradientInterpolationMethod::CSS_IMAGES_3)),
    )
}

fn parse_css_angle_degrees(value: &str) -> Option<f32> {
    let value = trim_css_value(value);
    let mut input = cssparser::ParserInput::new(value);
    let mut parser = cssparser::Parser::new(&mut input);
    let token = parser.next().ok()?.clone();
    let angle = match token {
        cssparser::Token::Number { value: 0.0, .. } => 0.0,
        cssparser::Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("deg") => {
            value
        }
        cssparser::Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("grad") => {
            value * 0.9
        }
        cssparser::Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("turn") => {
            value * 360.0
        }
        cssparser::Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("rad") => {
            value * 180.0 / std::f32::consts::PI
        }
        _ => return None,
    };
    parser.is_exhausted().then_some(angle)
}

fn parse_gradient_item(
    value: &str,
    stops: &mut Vec<GradientColorStop>,
    hints: &mut Vec<GradientColorHint>,
) -> Option<()> {
    if let Some(position) = parse_deferred_length_percentage(value) {
        if stops.is_empty() {
            return None;
        }
        hints.push(GradientColorHint {
            after_stop: stops.len() - 1,
            position,
        });
        return Some(());
    }
    let mut parts = background_components(value)?;
    if parts.is_empty() {
        return None;
    }
    let second_position = parts
        .last()
        .and_then(|part| parse_deferred_length_percentage(part));
    let first_position = if second_position.is_some() {
        parts.pop();
        parts
            .last()
            .and_then(|part| parse_deferred_length_percentage(part))
    } else {
        None
    };
    if first_position.is_some() {
        parts.pop();
    }
    let color_text = parts.join(" ");
    let color = parse_gradient_color(&color_text)?;
    stops.push(GradientColorStop {
        color,
        position: first_position.clone().or(second_position.clone()),
    });
    if let Some(second_position) = first_position.and(second_position) {
        stops.push(GradientColorStop {
            color,
            position: Some(second_position),
        });
    }
    Some(())
}

fn parse_gradient_color(value: &str) -> Option<GradientColor> {
    if value.trim().eq_ignore_ascii_case("currentcolor") {
        Some(GradientColor::CurrentColor)
    } else {
        let color = parse_color(value)?;
        let (missing, source) = gradient_missing_components(value);
        (!missing.is_empty())
            .then_some(GradientColor::ColorWithMissing {
                color,
                missing,
                source,
            })
            .or(Some(GradientColor::CssColor(color)))
    }
}

/// Retain `none` in a gradient stop instead of letting ordinary computed
/// color parsing collapse it to zero. Its replacement is deliberately delayed
/// until the selected interpolation space is known.
/// <https://drafts.csswg.org/css-color-4/#interpolation-missing>
fn gradient_missing_components(
    value: &str,
) -> (GradientMissingComponents, GradientMissingComponentSpace) {
    let Some((name, body)) = css_single_function(value.trim()) else {
        return (
            GradientMissingComponents::default(),
            GradientMissingComponentSpace::Rgb,
        );
    };
    let name = name.to_ascii_lowercase();
    if !matches!(
        name.as_str(),
        "rgb" | "rgba" | "hsl" | "hsla" | "hwb" | "lab" | "lch" | "oklab" | "oklch" | "color"
    ) {
        return (
            GradientMissingComponents::default(),
            GradientMissingComponentSpace::Rgb,
        );
    }
    let (components, slash_alpha) = split_css_top_level_once(body, '/')
        .map(|(components, alpha)| (components, Some(alpha)))
        .unwrap_or((body, None));
    let Some(tokens) = try_split_css_top_level_delimiter(components, ',').and_then(|groups| {
        groups
            .into_iter()
            .map(try_split_css_component_values)
            .collect::<Option<Vec<_>>>()
            .map(|groups| groups.into_iter().flatten().collect::<Vec<_>>())
    }) else {
        return (
            GradientMissingComponents::default(),
            GradientMissingComponentSpace::Rgb,
        );
    };
    let component_offset = usize::from(name == "color");
    let source = match name.as_str() {
        "rgb" | "rgba" => GradientMissingComponentSpace::Rgb,
        "hsl" | "hsla" => GradientMissingComponentSpace::Hsl,
        "hwb" => GradientMissingComponentSpace::Hwb,
        "lab" => GradientMissingComponentSpace::Lab,
        "lch" => GradientMissingComponentSpace::Lch,
        "oklab" => GradientMissingComponentSpace::Oklab,
        "oklch" => GradientMissingComponentSpace::Oklch,
        "color" => match tokens
            .first()
            .and_then(|token| css_single_ident(token))
            .map(|ident| ident.to_ascii_lowercase())
            .as_deref()
        {
            Some("xyz") | Some("xyz-d50") | Some("xyz-d65") => GradientMissingComponentSpace::Xyz,
            _ => GradientMissingComponentSpace::Rgb,
        },
        _ => unreachable!("validated gradient color function"),
    };
    if tokens.len() < component_offset + 3 {
        return (GradientMissingComponents::default(), source);
    }
    let mut bits = 0;
    for component in 0..3 {
        if css_identifier_is(tokens[component + component_offset], "none") {
            bits |= 1 << component;
        }
    }
    if slash_alpha.is_some_and(|alpha| css_identifier_is(alpha, "none"))
        || slash_alpha.is_none()
            && tokens
                .get(component_offset + 3)
                .is_some_and(|alpha| css_identifier_is(alpha, "none"))
    {
        bits |= 1 << 3;
    }
    (GradientMissingComponents::new(bits), source)
}

fn gradient_arguments<'a>(
    value: &'a str,
    normal_name: &str,
    repeating_name: &str,
) -> Option<(bool, Vec<&'a str>)> {
    let (name, body) = css_single_function(trim_css_value(value))?;
    let repeating = if name.eq_ignore_ascii_case(normal_name) {
        false
    } else if name.eq_ignore_ascii_case(repeating_name) {
        true
    } else {
        return None;
    };
    let arguments = try_split_css_top_level_delimiter(body, ',')?;
    (!arguments.is_empty() && arguments.iter().all(|argument| !argument.trim().is_empty()))
        .then_some((repeating, arguments))
}

fn split_background_position_size(value: &str) -> Option<Option<(String, String)>> {
    match split_css_top_level_once(value, '/') {
        Some((position, size)) => Some(Some((
            strip_background_noise(position)?,
            strip_background_noise(size)?,
        ))),
        None => background_components(value).map(|_| None),
    }
}

fn strip_background_noise(value: &str) -> Option<String> {
    // A background image is a single component value even when its function
    // contains whitespace or commas.  Splitting it with `str::split_whitespace`
    // leaks pieces such as `linear-gradient(red,` into the position grammar,
    // which in turn silently resets a shorthand layer to its initial position.
    // Preserve CSS component-value boundaries while extracting the shorthand's
    // position and size fields:
    // <https://www.w3.org/TR/css-syntax-3/#component-value> and
    // <https://www.w3.org/TR/css-backgrounds-3/#background>.
    Some(
        background_components(value)?
            .into_iter()
            .filter(|token| {
                !css_identifier_is(token, "none")
                    && !matches!(parse_css_image(token, None, None), ParsedImage::Image(_))
                    && !css_identifier_is(token, "repeat")
                    && !css_identifier_is(token, "no-repeat")
                    && !css_identifier_is(token, "repeat-x")
                    && !css_identifier_is(token, "repeat-y")
                    && !matches!(
                        css_single_ident(token).as_deref(),
                        Some("scroll" | "fixed" | "local")
                    )
                    && parse_background_box(token).is_none()
                    && parse_color(token).is_none()
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

pub(super) fn background_tokens(value: &str) -> Option<Vec<&str>> {
    // CSS functions are indivisible component values. In particular, a color
    // stop inside `linear-gradient()` is not the background shorthand's
    // color layer. Splitting on ASCII whitespace incorrectly promoted that
    // stop to `background-color`, painting an opaque fallback over the
    // gradient. CSS Syntax's component-value model keeps nested functions
    // intact here.
    // <https://www.w3.org/TR/css-syntax-3/#component-value>
    background_components(value)
}
