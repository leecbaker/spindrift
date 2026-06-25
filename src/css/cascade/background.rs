use super::*;

pub(super) fn apply_background_shorthand(
    style: &mut ComputedStyle,
    value: &str,
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
) {
    style.background_origin = BackgroundBox::Padding;
    style.background_clip = BackgroundBox::Border;
    style.background_color =
        parse_color(value).or_else(|| background_tokens(value).into_iter().find_map(parse_color));
    style.background_image = parse_background_image(value, base_url, root_url);
    style.background_repeat = parse_background_repeat(value).unwrap_or(BackgroundRepeat::Repeat);
    if let Some((position, size)) = split_background_position_size(value) {
        if !position.trim().is_empty()
            && let Some(position) = parse_background_position(position.trim(), style.font_size)
        {
            style.background_position = position;
        }
        if !size.trim().is_empty()
            && let Some(size) = parse_background_size(size.trim(), style.font_size)
        {
            style.background_size = size;
        }
    } else if background_tokens(value)
        .into_iter()
        .any(|token| token.eq_ignore_ascii_case("center"))
    {
        style.background_position = BackgroundPosition {
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
    let boxes = background_tokens(value)
        .into_iter()
        .filter_map(parse_background_box)
        .collect::<Vec<_>>();
    match boxes.as_slice() {
        [box_] => {
            style.background_origin = *box_;
            style.background_clip = *box_;
        }
        [origin, clip, ..] => {
            style.background_origin = *origin;
            style.background_clip = *clip;
        }
        [] => {}
    }
}

/// Parses a background box keyword for `background-origin` and
/// `background-clip`.
///
/// CSS Backgrounds and Borders defines `border-box`, `padding-box`, and
/// `content-box` as the shared keyword set for these properties:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin>.
pub(super) fn parse_background_box(value: &str) -> Option<BackgroundBox> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "border-box" => Some(BackgroundBox::Border),
        "padding-box" => Some(BackgroundBox::Padding),
        "content-box" => Some(BackgroundBox::Content),
        _ => None,
    }
}

/// Parses the single-layer `background-repeat` subset.
///
/// CSS Backgrounds and Borders defines one- and two-value repeat syntax,
/// including `repeat-x` and `repeat-y` aliases:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
pub(super) fn parse_background_repeat(value: &str) -> Option<BackgroundRepeat> {
    let tokens = background_tokens(value)
        .into_iter()
        .map(|token| token.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if tokens.iter().any(|token| token == "repeat-x") {
        return Some(BackgroundRepeat::RepeatX);
    }
    if tokens.iter().any(|token| token == "repeat-y") {
        return Some(BackgroundRepeat::RepeatY);
    }
    let repeat_tokens = tokens
        .iter()
        .filter(|token| token.as_str() == "repeat" || token.as_str() == "no-repeat")
        .map(String::as_str)
        .collect::<Vec<_>>();
    match repeat_tokens.as_slice() {
        [] => None,
        ["repeat"] | ["repeat", "repeat"] => Some(BackgroundRepeat::Repeat),
        ["no-repeat"] | ["no-repeat", "no-repeat"] => Some(BackgroundRepeat::NoRepeat),
        ["repeat", "no-repeat"] => Some(BackgroundRepeat::RepeatX),
        ["no-repeat", "repeat"] => Some(BackgroundRepeat::RepeatY),
        _ => None,
    }
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
    let parts = value.split_whitespace().collect::<Vec<_>>();
    let width = parts
        .first()
        .and_then(|part| parse_background_size_axis(part, font_size))?;
    let height = parts
        .get(1)
        .and_then(|part| parse_background_size_axis(part, font_size))
        .unwrap_or(BackgroundSizeAxis::Auto);
    Some(BackgroundSize::Explicit { width, height })
}

/// Parses one `background-size` axis.
///
/// CSS Backgrounds and Borders uses `auto | <length-percentage>` for explicit
/// background-size axes:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-size>.
fn parse_background_size_axis(value: &str, font_size: f32) -> Option<BackgroundSizeAxis> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        Some(BackgroundSizeAxis::Auto)
    } else {
        parse_computed_length_percentage(value, font_size).map(BackgroundSizeAxis::LengthPercentage)
    }
}

/// Parses `background-position` for a single background layer.
///
/// CSS Backgrounds and Borders defines one-to-four value positioning syntax.
/// This parser preserves the subset already supported by layout: side
/// keywords, `center`, and one offset following a side keyword:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>.
pub(crate) fn parse_background_position(value: &str, font_size: f32) -> Option<BackgroundPosition> {
    let tokens = trim_css_value(value)
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return None;
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

pub(super) fn extract_css_url(value: &str) -> Option<String> {
    parse_first_css_url(value)
}

/// Parses a single supported CSS background image.
///
/// CSS Backgrounds delegates image values to CSS Images. This parser supports
/// URL images and an axis-aligned `linear-gradient()` subset for generated
/// images:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-image> and
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
pub(super) fn parse_background_image(
    value: &str,
    base_url: Option<&std::path::Path>,
    root_url: Option<&std::path::Path>,
) -> Option<BackgroundImage> {
    if let Some(gradient) = parse_linear_gradient(value) {
        return Some(BackgroundImage::LinearGradient(gradient));
    }
    parse_first_css_url(value).map(|src| BackgroundImage::Url {
        src,
        base_url: base_url.map(std::path::Path::to_path_buf),
        root_url: root_url.map(std::path::Path::to_path_buf),
    })
}

/// Parses the supported `linear-gradient()` subset.
///
/// CSS Images permits angles, corners, interpolation hints, percentages, and
/// omitted positions. The current subset accepts `to top/right/bottom/left`
/// with explicit length stops, which covers WPT hard-stop print references:
/// <https://www.w3.org/TR/css-images-3/#linear-gradients>.
pub(crate) fn parse_linear_gradient(value: &str) -> Option<LinearGradient> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    let start = lower.find("linear-gradient(")?;
    let args_start = start + "linear-gradient(".len();
    let args_end = matching_function_end(value, args_start - 1)?;
    let args = &value[args_start..args_end];
    let parts = split_comma_function_args(args);
    if parts.len() < 3 {
        return None;
    }
    let direction = parse_linear_gradient_direction(parts[0].trim())?;
    let mut stops = Vec::new();
    for part in &parts[1..] {
        stops.push(parse_gradient_color_stop(part.trim())?);
    }
    if stops.len() < 2 {
        return None;
    }
    Some(LinearGradient { direction, stops })
}

fn parse_linear_gradient_direction(value: &str) -> Option<LinearGradientDirection> {
    match value.to_ascii_lowercase().as_str() {
        "to bottom" => Some(LinearGradientDirection::Bottom),
        "to top" => Some(LinearGradientDirection::Top),
        "to right" => Some(LinearGradientDirection::Right),
        "to left" => Some(LinearGradientDirection::Left),
        _ => None,
    }
}

fn parse_gradient_color_stop(value: &str) -> Option<GradientColorStop> {
    let mut parts = value.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    let position = parse_length(parts.pop()?)?;
    let color_text = parts.join(" ");
    let color = parse_color(&color_text)?;
    Some(GradientColorStop { color, position })
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
    let slash = value.find('/')?;
    let before = strip_background_noise(&value[..slash]);
    let after = strip_background_noise(&value[slash + 1..]);
    Some((before, after))
}

pub(super) fn strip_background_noise(value: &str) -> String {
    background_tokens(value)
        .into_iter()
        .filter(|token| {
            !token.starts_with("url(")
                && !token.eq_ignore_ascii_case("repeat")
                && !token.eq_ignore_ascii_case("no-repeat")
                && parse_color(token).is_none()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn background_tokens(value: &str) -> Vec<&str> {
    value
        .split_whitespace()
        .map(|token| token.trim_matches([',', ';']))
        .filter(|token| !token.is_empty())
        .collect()
}
