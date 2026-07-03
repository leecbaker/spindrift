use super::*;

pub(crate) fn parse_color(value: &str) -> Option<Color> {
    let value = trim_css_value(value).to_ascii_lowercase();
    if value == "transparent" {
        return Some(Color::TRANSPARENT);
    }
    if value == "none" {
        return None;
    }
    if let Some(hex) = value.strip_prefix('#') {
        let (r, g, b, a) = cssparser::color::parse_hash_color(hex.as_bytes()).ok()?;
        return Some(Color::rgba(r, g, b, a));
    }
    if let Some(rgb) = parse_rgb_function(&value) {
        return Some(rgb);
    }
    if let Some(hsl) = parse_hsl_function(&value) {
        return Some(hsl);
    }
    if let Some(hwb) = parse_hwb_function(&value) {
        return Some(hwb);
    }
    if let Some(color_function) = parse_color_function(&value) {
        return Some(color_function);
    }
    let (r, g, b) = cssparser::color::parse_named_color(&value).ok()?;
    Some(Color::new(r, g, b))
}

/// Parses the currently modeled sRGB subset of CSS Color syntax.
///
/// CSS Color Level 4 allows both legacy comma-separated and modern
/// whitespace-separated `rgb()`/`rgba()` forms:
/// <https://www.w3.org/TR/css-color-4/#rgb-functions>.
pub(crate) fn parse_rgb_function(value: &str) -> Option<Color> {
    let inner = value
        .strip_prefix("rgb(")
        .or_else(|| value.strip_prefix("rgba("))
        .and_then(|value| value.strip_suffix(')'))?;
    let (rgb, alpha) = split_rgb_alpha(inner);
    let channels = if rgb.contains(',') {
        let parts = rgb.split(',').map(str::trim).collect::<Vec<_>>();
        if !(parts.len() == 3 || parts.len() == 4) {
            return None;
        }
        let rgb_parts = parts.iter().take(3).copied();
        let alpha_part = parts
            .get(3)
            .copied()
            .filter(|value| !value.is_empty())
            .or(alpha);
        (
            rgb_parts
                .map(parse_rgb_channel)
                .collect::<Option<Vec<_>>>()?,
            alpha_part,
        )
    } else {
        let parts = rgb.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 3 {
            return None;
        }
        (
            parts
                .into_iter()
                .map(parse_rgb_channel)
                .collect::<Option<Vec<_>>>()?,
            alpha,
        )
    };
    let alpha = if let Some(alpha) = channels.1 {
        parse_alpha_value(alpha)?
    } else {
        1.0
    };
    match channels.0.as_slice() {
        [r, g, b] => Some(Color::rgba(*r, *g, *b, alpha)),
        _ => None,
    }
}

fn split_rgb_alpha(value: &str) -> (&str, Option<&str>) {
    value
        .split_once('/')
        .map(|(rgb, alpha)| (rgb.trim(), Some(alpha.trim())))
        .unwrap_or((value.trim(), None))
}

fn parse_rgb_channel(value: &str) -> Option<u8> {
    let value = value.trim();
    let channel = if let Some(percent) = value.strip_suffix('%') {
        percent.trim().parse::<f32>().ok()? * 255.0 / 100.0
    } else {
        value.parse::<f32>().ok()?
    };
    Some(channel.round().clamp(0.0, 255.0) as u8)
}

/// Parse `hsl()` and `hsla()` color functions.
///
/// CSS Color Level 4 allows both legacy comma-separated and modern
/// whitespace-separated HSL forms, with optional slash alpha:
/// <https://www.w3.org/TR/css-color-4/#the-hsl-notation>.
pub(crate) fn parse_hsl_function(value: &str) -> Option<Color> {
    let inner = value
        .strip_prefix("hsl(")
        .or_else(|| value.strip_prefix("hsla("))
        .and_then(|value| value.strip_suffix(')'))?;
    let (hsl, slash_alpha) = split_rgb_alpha(inner);
    let (hue, saturation, lightness, alpha) = if hsl.contains(',') {
        let parts = hsl.split(',').map(str::trim).collect::<Vec<_>>();
        if !(parts.len() == 3 || parts.len() == 4) {
            return None;
        }
        (
            parse_hue_degrees(parts[0])?,
            parse_percentage(parts[1])?,
            parse_percentage(parts[2])?,
            parts
                .get(3)
                .copied()
                .filter(|value| !value.is_empty())
                .or(slash_alpha),
        )
    } else {
        let parts = hsl.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 3 {
            return None;
        }
        (
            parse_hue_degrees(parts[0])?,
            parse_percentage(parts[1])?,
            parse_percentage(parts[2])?,
            slash_alpha,
        )
    };
    let alpha = if let Some(alpha) = alpha {
        parse_alpha_value(alpha)?
    } else {
        1.0
    };
    let (r, g, b) = hsl_to_rgb(hue, saturation, lightness);
    Some(Color::rgba(r, g, b, alpha))
}

/// Parse `hwb()` color functions into sRGB.
///
/// CSS Color Level 4 defines HWB as a cylindrical sRGB color notation whose
/// hue is mixed with whiteness and blackness; when whiteness plus blackness is
/// at least 100%, the result is the corresponding gray:
/// <https://www.w3.org/TR/css-color-4/#the-hwb-notation>.
pub(crate) fn parse_hwb_function(value: &str) -> Option<Color> {
    let inner = value
        .strip_prefix("hwb(")
        .and_then(|value| value.strip_suffix(')'))?;
    let (hwb, slash_alpha) = split_rgb_alpha(inner);
    let (hue, whiteness, blackness, alpha) = if hwb.contains(',') {
        let parts = hwb.split(',').map(str::trim).collect::<Vec<_>>();
        if !(parts.len() == 3 || parts.len() == 4) {
            return None;
        }
        (
            parse_hue_degrees(parts[0])?,
            parse_percentage(parts[1])?,
            parse_percentage(parts[2])?,
            parts
                .get(3)
                .copied()
                .filter(|value| !value.is_empty())
                .or(slash_alpha),
        )
    } else {
        let parts = hwb.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 3 {
            return None;
        }
        (
            parse_hue_degrees(parts[0])?,
            parse_percentage(parts[1])?,
            parse_percentage(parts[2])?,
            slash_alpha,
        )
    };
    let alpha = if let Some(alpha) = alpha {
        parse_alpha_value(alpha)?
    } else {
        1.0
    };
    let (r, g, b) = hwb_to_rgb(hue, whiteness, blackness);
    Some(Color::rgba(r, g, b, alpha))
}

/// Parse the supported `color()` subset.
///
/// CSS Color Level 4 defines `color(<colorspace> ...)` for predefined color
/// spaces. Reasyprint currently implements the `srgb` predefined space because
/// the internal paint model stores colors as sRGB/RGBA:
/// <https://www.w3.org/TR/css-color-4/#color-function> and
/// <https://www.w3.org/TR/css-color-4/#predefined-sRGB>.
pub(crate) fn parse_color_function(value: &str) -> Option<Color> {
    let inner = value
        .strip_prefix("color(")
        .and_then(|value| value.strip_suffix(')'))?;
    let (components, slash_alpha) = split_rgb_alpha(inner);
    let parts = components.split_whitespace().collect::<Vec<_>>();
    let [space, red, green, blue] = parts.as_slice() else {
        return None;
    };
    if !space.eq_ignore_ascii_case("srgb") {
        return None;
    }
    let alpha = if let Some(alpha) = slash_alpha {
        parse_alpha_value(alpha)?
    } else {
        1.0
    };
    Some(Color::srgb(
        parse_srgb_color_component(red)?,
        parse_srgb_color_component(green)?,
        parse_srgb_color_component(blue)?,
        alpha,
    ))
}

fn parse_srgb_color_component(value: &str) -> Option<f32> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    if let Some(percent) = value.strip_suffix('%') {
        return percent
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| value / 100.0);
    }
    value.parse::<f32>().ok()
}

fn parse_hue_degrees(value: &str) -> Option<f32> {
    let value = value.trim();
    if let Some(degrees) = value.strip_suffix("deg") {
        degrees.trim().parse::<f32>().ok()
    } else if let Some(turns) = value.strip_suffix("turn") {
        turns.trim().parse::<f32>().ok().map(|turns| turns * 360.0)
    } else if let Some(radians) = value.strip_suffix("rad") {
        radians
            .trim()
            .parse::<f32>()
            .ok()
            .map(|radians| radians.to_degrees())
    } else if let Some(gradians) = value.strip_suffix("grad") {
        gradians
            .trim()
            .parse::<f32>()
            .ok()
            .map(|gradians| gradians * 0.9)
    } else {
        value.parse::<f32>().ok()
    }
}

fn hwb_to_rgb(hue_degrees: f32, whiteness: f32, blackness: f32) -> (u8, u8, u8) {
    let whiteness = whiteness.clamp(0.0, 1.0);
    let blackness = blackness.clamp(0.0, 1.0);
    let sum = whiteness + blackness;
    if sum >= 1.0 {
        let gray = whiteness / sum;
        let channel = rgb_unit_to_u8(gray);
        return (channel, channel, channel);
    }
    let factor = 1.0 - sum;
    let (r, g, b) = hsl_to_rgb_units(hue_degrees, 1.0, 0.5);
    (
        rgb_unit_to_u8(r * factor + whiteness),
        rgb_unit_to_u8(g * factor + whiteness),
        rgb_unit_to_u8(b * factor + whiteness),
    )
}

fn hsl_to_rgb(hue_degrees: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let (r, g, b) = hsl_to_rgb_units(hue_degrees, saturation, lightness);
    (rgb_unit_to_u8(r), rgb_unit_to_u8(g), rgb_unit_to_u8(b))
}

fn hsl_to_rgb_units(hue_degrees: f32, saturation: f32, lightness: f32) -> (f32, f32, f32) {
    let hue = hue_degrees.rem_euclid(360.0) / 360.0;
    let saturation = saturation.clamp(0.0, 1.0);
    let lightness = lightness.clamp(0.0, 1.0);
    if saturation == 0.0 {
        return (lightness, lightness, lightness);
    }
    let q = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let p = 2.0 * lightness - q;
    (
        hue_channel(p, q, hue + 1.0 / 3.0),
        hue_channel(p, q, hue),
        hue_channel(p, q, hue - 1.0 / 3.0),
    )
}

fn hue_channel(p: f32, q: f32, mut hue: f32) -> f32 {
    if hue < 0.0 {
        hue += 1.0;
    } else if hue > 1.0 {
        hue -= 1.0;
    }
    if hue < 1.0 / 6.0 {
        p + (q - p) * 6.0 * hue
    } else if hue < 1.0 / 2.0 {
        q
    } else if hue < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - hue) * 6.0
    } else {
        p
    }
}

fn rgb_unit_to_u8(value: f32) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

fn parse_alpha_value(value: &str) -> Option<f32> {
    let value = value.trim();
    let alpha = if let Some(percent) = value.strip_suffix('%') {
        percent.trim().parse::<f32>().ok()? / 100.0
    } else {
        value.parse::<f32>().ok()?
    };
    Some(alpha.clamp(0.0, 1.0))
}
