use super::declarations::split_top_level_once;
use super::*;

pub(super) fn apply_background_shorthand(
    style: &mut ComputedStyle,
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) {
    style.background_color_current_color_expression = None;
    style.background_color_is_current_color = background_tokens(value)
        .iter()
        .any(|token| token.eq_ignore_ascii_case("currentcolor"));
    style.background_color = if style.background_color_is_current_color {
        Some(style.color)
    } else {
        parse_color(value).or_else(|| {
            background_tokens(value)
                .iter()
                .find_map(|token| parse_color(token))
        })
    };
    style.background_layers =
        parse_background_shorthand_layers(value, style.font_size, base_url, root_url);
    style.background_image_layer_count = style.background_layers.len().max(1);
    if style.background_layers.is_empty() {
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
    match trim_css_value(value).to_ascii_lowercase().as_str() {
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
    let tokens = background_tokens(value)
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if tokens.as_slice() == ["repeat-x"] {
        return Some(BackgroundRepeat::RepeatX);
    }
    if tokens.as_slice() == ["repeat-y"] {
        return Some(BackgroundRepeat::RepeatY);
    }
    let repeat_tokens = tokens
        .iter()
        .filter_map(|token| parse_background_repeat_axis(token))
        .collect::<Vec<_>>();
    match repeat_tokens.as_slice() {
        [] => None,
        [axis] => Some(BackgroundRepeat::new(*axis, *axis)),
        [x, y] => Some(BackgroundRepeat::new(*x, *y)),
        _ => None,
    }
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
        style.background_image = None;
        style.background_layers.clear();
        style.background_image_layer_count = 1;
        return;
    }
    style.background_image_layer_count = images.len();
    ensure_background_layer_count(style, images.len());
    for (index, layer) in style.background_layers.iter_mut().enumerate() {
        layer.image = repeated_layer_value(&images, index);
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_size_list(style: &mut ComputedStyle, value: &str) {
    let values = split_background_layer_values(value)
        .into_iter()
        .filter_map(|part| parse_background_size(part, style.font_size))
        .collect::<Vec<_>>();
    if values.is_empty() {
        return;
    }
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background_layers.iter_mut().enumerate() {
        layer.size = repeated_layer_value(&values, index);
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_position_list(style: &mut ComputedStyle, value: &str) {
    let values = split_background_layer_values(value)
        .into_iter()
        .filter_map(|part| parse_background_position(part, style.font_size))
        .collect::<Vec<_>>();
    if values.is_empty() {
        return;
    }
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background_layers.iter_mut().enumerate() {
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
    let values = split_background_layer_values(value)
        .into_iter()
        .filter_map(|part| parse_background_position_axis(part, style.font_size, horizontal))
        .collect::<Vec<_>>();
    if values.is_empty() {
        return;
    }
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background_layers.iter_mut().enumerate() {
        if horizontal {
            layer.position.x = repeated_layer_value(&values, index);
        } else {
            layer.position.y = repeated_layer_value(&values, index);
        }
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_repeat_list(style: &mut ComputedStyle, value: &str) {
    let values = split_background_layer_values(value)
        .into_iter()
        .filter_map(parse_background_repeat)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return;
    }
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background_layers.iter_mut().enumerate() {
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
    let values = split_background_layer_values(value)
        .into_iter()
        .filter_map(
            |value| match trim_css_value(value).to_ascii_lowercase().as_str() {
                "scroll" => Some(BackgroundAttachment::Scroll),
                "fixed" => Some(BackgroundAttachment::Fixed),
                "local" => Some(BackgroundAttachment::Local),
                _ => None,
            },
        )
        .collect::<Vec<_>>();
    if values.is_empty() {
        return;
    }
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background_layers.iter_mut().enumerate() {
        layer.attachment = repeated_layer_value(&values, index);
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_origin_list(style: &mut ComputedStyle, value: &str) {
    let values = split_background_layer_values(value)
        .into_iter()
        .filter_map(parse_background_box)
        .filter(|box_| *box_ != BackgroundBox::BorderArea)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return;
    }
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background_layers.iter_mut().enumerate() {
        layer.origin = repeated_layer_value(&values, index);
    }
    sync_background_single_fields_from_layers(style);
}

pub(super) fn apply_background_clip_list(style: &mut ComputedStyle, value: &str) {
    let values = split_background_layer_values(value)
        .into_iter()
        .filter_map(parse_background_box)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return;
    }
    ensure_background_layer_count(style, values.len());
    for (index, layer) in style.background_layers.iter_mut().enumerate() {
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
    if value.eq_ignore_ascii_case("auto") {
        return Some(BackgroundSize::Auto);
    }
    if value.eq_ignore_ascii_case("cover") {
        return Some(BackgroundSize::Cover);
    }
    if value.eq_ignore_ascii_case("contain") {
        return Some(BackgroundSize::Contain);
    }
    let parts = split_css_top_level_whitespace(value);
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
    if value.eq_ignore_ascii_case("auto") {
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
    let tokens = split_css_top_level_whitespace(trim_css_value(value))
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .collect::<Vec<_>>();
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
        let x = parse_computed_length_percentage(&tokens[0], font_size)?;
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
    let mut position = BackgroundPosition::INITIAL;
    if tokens.iter().any(|token| token == "center") {
        position.x.origin = BackgroundPositionOrigin::Center;
        position.y.origin = BackgroundPositionOrigin::Center;
    }
    if tokens.iter().any(|token| token == "right") {
        position.x.origin = BackgroundPositionOrigin::End;
    } else if tokens.iter().any(|token| token == "left") {
        position.x.origin = BackgroundPositionOrigin::Start;
    }
    if tokens.iter().any(|token| token == "bottom") {
        position.y.origin = BackgroundPositionOrigin::End;
    } else if tokens.iter().any(|token| token == "top") {
        position.y.origin = BackgroundPositionOrigin::Start;
    }
    for pair in tokens.windows(2) {
        if pair[0] == "left" || pair[0] == "right" {
            if let Some(offset) = parse_computed_length_percentage(&pair[1], font_size) {
                position.x.offset = offset;
            }
        } else if (pair[0] == "top" || pair[0] == "bottom")
            && let Some(offset) = parse_computed_length_percentage(&pair[1], font_size)
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
    let tokens = split_css_top_level_whitespace(trim_css_value(value))
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .collect::<Vec<_>>();
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
            let origin = match value.as_str() {
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
            let origin = match side.as_str() {
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

pub(super) fn extract_css_url(value: &str) -> Option<String> {
    parse_first_css_url(value)
}

/// Parses a single supported CSS background image.
///
/// CSS Backgrounds delegates image values to CSS Images. This parser supports
/// URL images and CSS Images Level 3 linear/radial gradients as generated images:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-image> and
/// <https://www.w3.org/TR/css-images-3/#gradients>.
pub(crate) fn parse_background_image(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<BackgroundImage> {
    match parse_image_set(value, base_url, root_url) {
        Some(Ok(Some(image))) => return Some(image),
        Some(Ok(None) | Err(())) => return None,
        None => {}
    }
    if let Some(gradient) = parse_conic_gradient(value) {
        return Some(BackgroundImage::ConicGradient(gradient));
    }
    if let Some(color) = parse_color_image(value) {
        return Some(BackgroundImage::Color(color));
    }
    if let Some(gradient) = parse_linear_gradient(value) {
        return Some(BackgroundImage::LinearGradient(gradient));
    }
    if let Some(gradient) = parse_radial_gradient(value) {
        return Some(BackgroundImage::RadialGradient(gradient));
    }
    parse_first_css_url_with_modifiers(value).map(|url| BackgroundImage::Url {
        src: url.src,
        base_url: base_url.cloned(),
        root_url: root_url.cloned(),
        request_modifiers: url.modifiers,
    })
}

/// Parse CSS Images Level 4's `image(<color>)` subset. A color image has no
/// intrinsic dimensions, so it participates in background sizing like every
/// other generated image.
/// <https://drafts.csswg.org/css-images-4/#image-notation>
fn parse_color_image(value: &str) -> Option<ColorImageColor> {
    let body = strip_ascii_function(trim_css_value(value), "image")?;
    let (argument, tail) = split_function_argument(body)?;
    tail.trim().is_empty().then_some(())?;
    if argument.trim().eq_ignore_ascii_case("currentcolor") {
        Some(ColorImageColor::CurrentColor)
    } else {
        parse_color(argument.trim()).map(ColorImageColor::Color)
    }
}

/// Parse CSS Images Level 4 conic gradients into their angular color-line.
/// <https://drafts.csswg.org/css-images-4/#conic-gradients>
fn parse_conic_gradient(value: &str) -> Option<ConicGradient> {
    let value = trim_css_value(value);
    let repeating = value
        .get(.."repeating-conic-gradient".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("repeating-conic-gradient"));
    let name = if repeating {
        "repeating-conic-gradient"
    } else {
        "conic-gradient"
    };
    let body = strip_ascii_function(value, name)?;
    let (arguments, tail) = split_function_argument(body)?;
    tail.trim().is_empty().then_some(())?;
    let parts = split_comma_function_args(arguments);
    let (start_angle, position, first_stop) = parse_conic_prelude(parts.first()?.trim());
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
        repeating,
        stops,
    })
}

fn parse_conic_prelude(value: &str) -> (f32, BackgroundPosition, bool) {
    let tokens = split_css_top_level_whitespace(value);
    let Some(first) = tokens.first() else {
        return (0.0, radial_gradient_center_position(), false);
    };
    if parse_color(first).is_some() {
        return (0.0, radial_gradient_center_position(), false);
    }
    let mut angle = 0.0;
    let mut position = radial_gradient_center_position();
    if let Some(from) = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("from"))
        && let Some(value) = tokens
            .get(from + 1)
            .and_then(|value| parse_css_angle_degrees(value))
    {
        angle = value;
    }
    if let Some(at) = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("at"))
    {
        let value = tokens[at + 1..].join(" ");
        if let Some(parsed) = parse_radial_gradient_position(&value) {
            position = parsed;
        }
    }
    (angle, position, true)
}

fn parse_conic_gradient_stop(value: &str) -> Option<Vec<ConicGradientStop>> {
    let tokens = split_css_top_level_whitespace(value);
    for split in (1..=tokens.len()).rev() {
        let Some(color) = parse_color(&tokens[..split].join(" ")) else {
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

/// Select the resource appropriate for Quire's fixed 1dppx rendering
/// environment from a CSS Images `image-set()`.
///
/// CSS Images Level 4 resolves an image set by discarding candidates whose
/// image type is unsupported, then choosing the lowest resolution not below
/// the output resolution (or the highest available lower resolution). A
/// malformed candidate invalidates the whole image-set value. Keeping that
/// selection at image-value parsing time means every existing consumer of
/// `BackgroundImage` (backgrounds and generated content) shares exactly the
/// same selected resource without a second, property-specific image-set
/// implementation.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
/// Parse a CSS `image-set()` image while preserving the distinction between a
/// malformed value and a valid set for which this renderer supports no image
/// source. The former invalidates its declaration; the latter computes to a
/// missing image and therefore paints nothing.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
fn parse_image_set(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<Result<Option<BackgroundImage>, ()>> {
    let body = strip_ascii_function(trim_css_value(value), "image-set")?;
    let (body, tail) = match split_function_argument(body) {
        Some(value) => value,
        None => return Some(Err(())),
    };
    if !tail.trim().is_empty() {
        return Some(Err(()));
    }
    let mut options = Vec::new();
    for option in split_background_layer_values(body) {
        match parse_image_set_option(option, base_url, root_url) {
            Ok(Some(option)) => options.push(option),
            Ok(None) => {}
            Err(()) => return Some(Err(())),
        }
    }
    if options.is_empty() {
        return Some(Ok(None));
    }
    options.sort_by(|left, right| left.0.total_cmp(&right.0));
    let selected = options
        .iter()
        .find(|(resolution, _)| *resolution >= 1.0)
        .or_else(|| {
            // Among candidates below the output resolution, select the
            // greatest resolution. Equal-resolution candidates preserve
            // source order, so this keeps the first matching candidate.
            // <https://drafts.csswg.org/css-images-4/#image-set-resolution>
            options.iter().fold(None, |best, candidate| match best {
                Some(best) if best.0 >= candidate.0 => Some(best),
                _ => Some(candidate),
            })
        });
    Some(Ok(selected.map(|(resolution, image)| {
        BackgroundImage::ImageSet {
            image: Box::new(image.clone()),
            resolution: *resolution,
        }
    })))
}

fn parse_image_set_option(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Result<Option<(f32, BackgroundImage)>, ()> {
    let value = trim_css_value(value);
    let (image, tail) = parse_image_set_option_image(value, base_url, root_url).ok_or(())?;
    let tail = tail.trim();
    let type_is_supported = !tail.to_ascii_lowercase().contains("image/unsupported");
    if !type_is_supported {
        return Ok(None);
    }
    let resolution = parse_image_set_resolution(tail).ok_or(())?;
    if !resolution.is_finite() || resolution < 0.0 {
        let descriptor = image_set_resolution_without_type(tail).ok_or(())?;
        // A negative literal is a parse-time grammar error, while a valid
        // `calc()` descriptor that computes negative is an invalid candidate.
        // The latter leaves a valid image-set whose result is an invalid image
        // when no other candidate survives.
        if strip_ascii_function(descriptor, "calc").is_some() {
            return Ok(None);
        }
        return Err(());
    }
    if resolution == 0.0 {
        return Ok(None);
    }
    Ok(Some((resolution, image)))
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
    for name in [
        "linear-gradient",
        "repeating-linear-gradient",
        "radial-gradient",
        "repeating-radial-gradient",
        "conic-gradient",
        "repeating-conic-gradient",
    ] {
        let Some(body) = strip_ascii_function(value, name) else {
            continue;
        };
        let (arguments, tail) = split_function_argument(body)?;
        let image = parse_background_image(&format!("{name}({arguments})"), base_url, root_url)?;
        return Some((image, tail));
    }
    None
}

fn parse_image_set_resolution(value: &str) -> Option<f32> {
    let value = image_set_resolution_without_type(value)?;
    if value.is_empty() {
        // A missing descriptor computes to 1x.
        // <https://drafts.csswg.org/css-images-4/#image-set-notation>
        return Some(1.0);
    }
    let resolution = if let Some(body) = strip_ascii_function(value, "calc") {
        let (expression, tail) = split_function_argument(body)?;
        tail.trim().is_empty().then_some(())?;
        parse_simple_image_set_resolution_expression(expression)?
    } else {
        parse_image_set_resolution_dimension(value)?
    };
    Some(resolution)
}

fn image_set_resolution_without_type(value: &str) -> Option<&str> {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    let Some(type_start) = lower.find("type(") else {
        return Some(value);
    };
    if type_start == 0 {
        let body = strip_ascii_function(value, "type")?;
        let (_, tail) = split_function_argument(body)?;
        return Some(tail.trim());
    }
    // The `type()` descriptor follows the resolution in the common form.
    // Parsing its balanced body rejects a dangling descriptor instead of
    // silently accepting arbitrary trailing tokens.
    let body = strip_ascii_function(&value[type_start..], "type")?;
    let (_, tail) = split_function_argument(body)?;
    tail.trim().is_empty().then_some(value[..type_start].trim())
}

fn parse_simple_image_set_resolution_expression(value: &str) -> Option<f32> {
    let compact = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    if let Some((left, right)) = compact.split_once('*') {
        let left =
            parse_image_set_resolution_dimension(left).or_else(|| left.parse::<f32>().ok())?;
        let right =
            parse_image_set_resolution_dimension(right).or_else(|| right.parse::<f32>().ok())?;
        return Some(left * right);
    }
    if let Some((left, right)) = compact.split_once('/') {
        let left =
            parse_image_set_resolution_dimension(left).or_else(|| left.parse::<f32>().ok())?;
        let right =
            parse_image_set_resolution_dimension(right).or_else(|| right.parse::<f32>().ok())?;
        return (right != 0.0).then_some(left / right);
    }
    parse_image_set_resolution_dimension(&compact)
}

fn parse_image_set_resolution_dimension(value: &str) -> Option<f32> {
    for (unit, factor) in [
        ("dppx", 1.0),
        ("dpi", 1.0 / 96.0),
        ("dpcm", 2.54 / 96.0),
        ("x", 1.0),
    ] {
        if let Some(number) = value.strip_suffix(unit) {
            return Some(number.trim().parse::<f32>().ok()? * factor);
        }
    }
    None
}

pub(super) fn sync_background_layers_from_single_fields(style: &mut ComputedStyle) {
    style.background_layers = vec![layer_from_single_fields(style)];
    style.background_image_layer_count = 1;
}

pub(super) fn sync_background_single_fields_from_layers(style: &mut ComputedStyle) {
    let Some(layer) = style.background_layers.first() else {
        style.background_image = None;
        style.background_size = BackgroundSize::AUTO;
        style.background_position = BackgroundPosition::INITIAL;
        style.background_repeat = BackgroundRepeat::Repeat;
        style.background_origin = BackgroundBox::Padding;
        style.background_clip = BackgroundBox::Border;
        return;
    };
    style.background_image = layer.image.clone();
    style.background_size = layer.size.clone();
    style.background_position = layer.position.clone();
    style.background_repeat = layer.repeat;
    style.background_attachment = layer.attachment;
    style.background_origin = layer.origin;
    style.background_clip = layer.clip;
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
    if style.background_layers.is_empty() {
        style
            .background_layers
            .push(layer_from_single_fields(style));
    }
    style
        .background_layers
        .truncate(style.background_image_layer_count.max(1));
    sync_background_single_fields_from_layers(style);
}

fn layer_from_single_fields(style: &ComputedStyle) -> BackgroundLayer {
    BackgroundLayer {
        image: style.background_image.clone(),
        position: style.background_position.clone(),
        size: style.background_size.clone(),
        repeat: style.background_repeat,
        attachment: style.background_attachment,
        origin: style.background_origin,
        clip: style.background_clip,
    }
}

fn ensure_background_layer_count(style: &mut ComputedStyle, count: usize) {
    if count == 0 {
        return;
    }
    if style.background_layers.is_empty() {
        style
            .background_layers
            .push(layer_from_single_fields(style));
    }
    while style.background_layers.len() < count {
        let layer = style
            .background_layers
            .last()
            .cloned()
            .unwrap_or_else(BackgroundLayer::initial);
        style.background_layers.push(layer);
    }
}

fn repeated_layer_value<T: Clone>(values: &[T], index: usize) -> T {
    values[index % values.len()].clone()
}

fn parse_background_image_layers(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<Vec<Option<BackgroundImage>>> {
    let mut images = Vec::new();
    for part in split_background_layer_values(value) {
        let image = if trim_css_value(part).eq_ignore_ascii_case("none") {
            None
        } else {
            match parse_image_set(part, base_url, root_url) {
                Some(Ok(image)) => image,
                Some(Err(())) => return None,
                None => parse_background_image(part, base_url, root_url),
            }
        };
        if image.is_none()
            && !trim_css_value(part).eq_ignore_ascii_case("none")
            && parse_image_set(part, base_url, root_url).is_none()
        {
            return None;
        }
        images.push(image);
    }
    Some(images)
}

fn parse_background_shorthand_layers(
    value: &str,
    font_size: f32,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Vec<BackgroundLayer> {
    split_background_layer_values(value)
        .into_iter()
        .map(|part| {
            let mut layer = BackgroundLayer::initial();
            layer.image = parse_background_image(part, base_url, root_url);
            layer.repeat = parse_background_repeat(part).unwrap_or(BackgroundRepeat::Repeat);
            layer.attachment = background_tokens(part)
                .into_iter()
                .find_map(|token| match token.to_ascii_lowercase().as_str() {
                    "scroll" => Some(BackgroundAttachment::Scroll),
                    "fixed" => Some(BackgroundAttachment::Fixed),
                    "local" => Some(BackgroundAttachment::Local),
                    _ => None,
                })
                .unwrap_or(BackgroundAttachment::Scroll);
            if let Some((position, size)) = split_background_position_size(part) {
                if !position.trim().is_empty()
                    && let Some(position) = parse_background_position(position.trim(), font_size)
                {
                    layer.position = position;
                }
                if !size.trim().is_empty()
                    && let Some(size) = parse_background_size(size.trim(), font_size)
                {
                    layer.size = size;
                }
            } else if background_tokens(part)
                .into_iter()
                .any(|token| token.eq_ignore_ascii_case("center"))
            {
                layer.position = BackgroundPosition {
                    x: BackgroundPositionAxis {
                        origin: BackgroundPositionOrigin::Center,
                        offset: ComputedLengthPercentage::ZERO,
                    },
                    y: BackgroundPositionAxis {
                        origin: BackgroundPositionOrigin::Center,
                        offset: ComputedLengthPercentage::ZERO,
                    },
                };
            }
            let boxes = background_tokens(part)
                .into_iter()
                .filter_map(|token| parse_background_box(&token))
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
            layer
        })
        .collect()
}

fn split_background_layer_values(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() {
            if character == '\\' {
                escaped = true;
            } else if Some(character) == quote {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let part = trim_css_value(&value[start..index]);
                if !part.is_empty() {
                    parts.push(part);
                }
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    let part = trim_css_value(&value[start..]);
    if !part.is_empty() {
        parts.push(part);
    }
    parts
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
    let value = trim_css_value(value);
    let (repeating, start, name_len) = find_linear_gradient_function(value)?;
    let function_text = &value[start..];
    let mut input = cssparser::ParserInput::new(function_text);
    let mut parser = cssparser::Parser::new(&mut input);
    let name = parser.expect_function().ok()?.clone();
    if repeating {
        if !name.eq_ignore_ascii_case("repeating-linear-gradient") {
            return None;
        }
    } else if !name.eq_ignore_ascii_case("linear-gradient") {
        return None;
    }
    let args_start = start + name_len;
    let args_end = matching_function_end(value, args_start - 1)?;
    let args = &value[args_start..args_end];
    let parts = split_comma_function_args(args);
    if parts.is_empty() {
        return None;
    }
    let mut first_stop = 0usize;
    let direction = if let Some(direction) = parse_linear_gradient_direction(parts[0].trim()) {
        first_stop = 1;
        direction
    } else {
        LinearGradientDirection::Angle(180.0)
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
    let value = trim_css_value(value);
    let (repeating, start, name_len) = find_radial_gradient_function(value)?;
    let function_text = &value[start..];
    let mut input = cssparser::ParserInput::new(function_text);
    let mut parser = cssparser::Parser::new(&mut input);
    let name = parser.expect_function().ok()?.clone();
    if repeating {
        if !name.eq_ignore_ascii_case("repeating-radial-gradient") {
            return None;
        }
    } else if !name.eq_ignore_ascii_case("radial-gradient") {
        return None;
    }
    let args_start = start + name_len;
    let args_end = matching_function_end(value, args_start - 1)?;
    let args = &value[args_start..args_end];
    let parts = split_comma_function_args(args);
    if parts.is_empty() {
        return None;
    }
    let mut first_stop = 0usize;
    let (shape, size, position) = if let Some(prelude) = parse_radial_gradient_prelude(&parts[0]) {
        first_stop = 1;
        prelude
    } else {
        (
            RadialGradientShape::Ellipse,
            RadialGradientSize::Extent(RadialGradientExtent::FarthestCorner),
            radial_gradient_center_position(),
        )
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
        repeating,
        stops,
        hints,
    })
}

fn parse_radial_gradient_prelude(
    value: &str,
) -> Option<(RadialGradientShape, RadialGradientSize, BackgroundPosition)> {
    let (size_text, position) = split_radial_gradient_position(value)?;
    let (shape, size) = parse_radial_gradient_shape_and_size(&size_text)?;
    Some((shape, size, position))
}

fn split_radial_gradient_position(value: &str) -> Option<(String, BackgroundPosition)> {
    let tokens = split_css_top_level_whitespace(value);
    let Some(at_index) = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("at"))
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
    let tokens = split_css_top_level_whitespace(value);
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
        match token.to_ascii_lowercase().as_str() {
            "circle" if shape.is_none() => shape = Some(RadialGradientShape::Circle),
            "ellipse" if shape.is_none() => shape = Some(RadialGradientShape::Ellipse),
            "closest-side" if extent.is_none() => {
                extent = Some(RadialGradientExtent::ClosestSide);
            }
            "farthest-side" if extent.is_none() => {
                extent = Some(RadialGradientExtent::FarthestSide);
            }
            "closest-corner" if extent.is_none() => {
                extent = Some(RadialGradientExtent::ClosestCorner);
            }
            "farthest-corner" if extent.is_none() => {
                extent = Some(RadialGradientExtent::FarthestCorner);
            }
            _ => lengths.push(parse_deferred_length_percentage(&token)?),
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
    let tokens = split_css_top_level_whitespace(value);
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
    let tokens = value
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    let first = tokens.first()?;
    if first != "to" {
        return None;
    }
    let rest = &tokens[1..];
    if rest.is_empty() || rest.len() > 2 {
        return None;
    }
    let mut horizontal = None;
    let mut vertical = None;
    for token in rest {
        match token.as_str() {
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
    let mut parts = split_css_top_level_whitespace(value);
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
    let color = parse_color(&color_text)?;
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

fn find_linear_gradient_function(value: &str) -> Option<(bool, usize, usize)> {
    let lower = value.to_ascii_lowercase();
    let repeating_name = "repeating-linear-gradient(";
    let normal_name = "linear-gradient(";
    let repeating = lower.find(repeating_name);
    let normal = lower.find(normal_name);
    match (repeating, normal) {
        (Some(repeating), Some(normal)) if repeating <= normal => {
            Some((true, repeating, repeating_name.len()))
        }
        (Some(repeating), None) => Some((true, repeating, repeating_name.len())),
        (_, Some(normal)) => Some((false, normal, normal_name.len())),
        _ => None,
    }
}

fn find_radial_gradient_function(value: &str) -> Option<(bool, usize, usize)> {
    let lower = value.to_ascii_lowercase();
    let repeating_name = "repeating-radial-gradient(";
    let normal_name = "radial-gradient(";
    let repeating = lower.find(repeating_name);
    let normal = lower.find(normal_name);
    match (repeating, normal) {
        (Some(repeating), Some(normal)) if repeating <= normal => {
            Some((true, repeating, repeating_name.len()))
        }
        (Some(repeating), None) => Some((true, repeating, repeating_name.len())),
        (_, Some(normal)) => Some((false, normal, normal_name.len())),
        _ => None,
    }
}

fn split_css_top_level_whitespace(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() {
            if character == '\\' {
                escaped = true;
            } else if Some(character) == quote {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '(' => {
                depth += 1;
                start.get_or_insert(index);
            }
            ')' => depth = depth.saturating_sub(1),
            ch if ch.is_whitespace() && depth == 0 => {
                if let Some(token_start) = start.take()
                    && token_start < index
                {
                    parts.push(value[token_start..index].to_string());
                }
            }
            _ => {
                start.get_or_insert(index);
            }
        }
    }
    if let Some(token_start) = start
        && token_start < value.len()
    {
        parts.push(value[token_start..].to_string());
    }
    parts
}

fn matching_function_end(value: &str, open_paren: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in value
        .char_indices()
        .skip_while(|(index, _)| *index < open_paren)
    {
        match ch {
            '(' => depth = depth.saturating_add(1),
            ')' => {
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

fn split_comma_function_args(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, ch) in value.char_indices() {
        match ch {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(value[start..index].trim().to_string());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim().to_string());
    parts
}

pub(super) fn split_background_position_size(value: &str) -> Option<(String, String)> {
    let (position, size) = split_top_level_once(value, '/')?;
    let before = strip_background_noise(position);
    let after = strip_background_noise(size);
    Some((before, after))
}

pub(super) fn strip_background_noise(value: &str) -> String {
    // A background image is a single component value even when its function
    // contains whitespace or commas.  Splitting it with `str::split_whitespace`
    // leaks pieces such as `linear-gradient(red,` into the position grammar,
    // which in turn silently resets a shorthand layer to its initial position.
    // Preserve CSS component-value boundaries while extracting the shorthand's
    // position and size fields:
    // <https://www.w3.org/TR/css-syntax-3/#component-value> and
    // <https://www.w3.org/TR/css-backgrounds-3/#background>.
    split_css_top_level_whitespace(value)
        .into_iter()
        .filter(|token| {
            !token.eq_ignore_ascii_case("none")
                && parse_background_image(token, None, None).is_none()
                && !token.eq_ignore_ascii_case("repeat")
                && !token.eq_ignore_ascii_case("no-repeat")
                && !matches!(
                    token.to_ascii_lowercase().as_str(),
                    "scroll" | "fixed" | "local"
                )
                && parse_background_box(token).is_none()
                && parse_color(token).is_none()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn background_tokens(value: &str) -> Vec<String> {
    // CSS functions are indivisible component values. In particular, a color
    // stop inside `linear-gradient()` is not the background shorthand's
    // color layer. Splitting on ASCII whitespace incorrectly promoted that
    // stop to `background-color`, painting an opaque fallback over the
    // gradient. CSS Syntax's component-value model keeps nested functions
    // intact here.
    // <https://www.w3.org/TR/css-syntax-3/#component-value>
    split_css_top_level_whitespace(value)
        .into_iter()
        .map(|token| token.trim_matches([',', ';']).to_string())
        .filter(|token| !token.is_empty())
        .collect()
}
