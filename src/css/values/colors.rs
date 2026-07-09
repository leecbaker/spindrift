use super::*;

pub(crate) fn parse_color(value: &str) -> Option<Color> {
    let value = remove_css_comments(trim_css_value(value)).to_ascii_lowercase();
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
    if let Some(lab) = parse_lab_function(&value) {
        return Some(lab);
    }
    if let Some(lch) = parse_lch_function(&value) {
        return Some(lch);
    }
    if let Some(oklab) = parse_oklab_function(&value) {
        return Some(oklab);
    }
    if let Some(oklch) = parse_oklch_function(&value) {
        return Some(oklch);
    }
    if let Some(color_function) = parse_color_function(&value) {
        return Some(color_function);
    }
    if !value.contains("currentcolor") {
        if let Some(color_mix) = parse_color_mix(&value, Color::BLACK) {
            return Some(color_mix);
        }
        if !value.contains("currentcolor")
            && let Some(contrast_color) = parse_contrast_color(&value, Color::BLACK)
        {
            return Some(contrast_color);
        }
    }
    if let Some(color) = parse_system_color(&value) {
        return Some(color);
    }
    let (r, g, b) = cssparser::color::parse_named_color(&value).ok()?;
    Some(Color::new(r, g, b))
}

/// Resolve the relative-color forms whose origin is `currentcolor` against a
/// computed foreground color. CSS Color 5 retains the relative syntax through
/// inheritance and resolves its origin at used-value time:
/// <https://www.w3.org/TR/css-color-5/#relative-colors>.
pub(crate) fn parse_color_from_currentcolor(value: &str, current: Color) -> Option<Color> {
    let value = remove_css_comments(trim_css_value(value)).to_ascii_lowercase();
    if let Some(contrast_color) = parse_contrast_color(&value, current) {
        return Some(contrast_color);
    }
    if let Some(color) = parse_light_dark(&value, current) {
        return Some(color);
    }
    if let Some(color_mix) = parse_color_mix(&value, current) {
        return Some(color_mix);
    }
    let (function, components) = value.split_once("(from currentcolor ")?;
    let components = components.strip_suffix(')')?;
    let components = components.split_whitespace().collect::<Vec<_>>();
    match function {
        "rgb" | "rgba" if components.len() == 3 => {
            let channel = |name: &str| match name {
                "r" => Some(current.r),
                "g" => Some(current.g),
                "b" => Some(current.b),
                _ => None,
            };
            Some(Color::srgb(
                channel(components[0])?,
                channel(components[1])?,
                channel(components[2])?,
                current.a,
            ))
        }
        "hsl" | "hsla" if components.len() == 3 => {
            let (hue, saturation, lightness) = srgb_to_hsl(current);
            let hue = match components[0] {
                "h" => hue,
                value => parse_hue_degrees(value)?,
            };
            let saturation = match components[1] {
                "s" => saturation,
                _ => return None,
            };
            let lightness = match components[2] {
                "l" => lightness,
                _ => return None,
            };
            let (r, g, b) = hsl_to_rgb_units(hue, saturation, lightness);
            Some(Color::srgb(r, g, b, current.a))
        }
        "hwb" if components.as_slice() == ["h", "w", "b"] => Some(current),
        "lab" | "oklab" if components.as_slice() == ["l", "a", "b"] => Some(current),
        "lch" | "oklch" if components.as_slice() == ["l", "c", "h"] => Some(current),
        "color"
            if components.len() == 4
                && matches!(
                    components[0],
                    "srgb" | "srgb-linear" | "display-p3" | "a98-rgb" | "prophoto-rgb" | "rec2020"
                )
                && components[1..] == ["r", "g", "b"] =>
        {
            Some(current)
        }
        "color"
            if components.len() == 4
                && matches!(components[0], "xyz" | "xyz-d50" | "xyz-d65")
                && components[1..] == ["x", "y", "z"] =>
        {
            Some(current)
        }
        _ => None,
    }
}

/// Parse CSS Color 5 `contrast-color()`.
///
/// The function must resolve to whichever of black and white has the greater
/// contrast against its argument. CSS leaves the exact contrast algorithm to
/// the user agent; Quire uses the WCAG relative-luminance contrast ratio,
/// whose monotonic ordering makes this choice well-defined:
/// <https://www.w3.org/TR/css-color-5/#contrast-color>.
fn parse_contrast_color(value: &str, current: Color) -> Option<Color> {
    let inner = value.strip_prefix("contrast-color(")?.strip_suffix(')')?;
    let background = if inner.trim().eq_ignore_ascii_case("currentcolor") {
        current
    } else {
        parse_color(inner.trim())?
    };
    let luminance = relative_luminance(background);
    let black_contrast = (luminance + 0.05) / 0.05;
    let white_contrast = 1.05 / (luminance + 0.05);
    Some(if black_contrast >= white_contrast {
        Color::BLACK
    } else {
        Color::WHITE
    })
}

fn relative_luminance(color: Color) -> f32 {
    // The WCAG contrast calculation is defined for sRGB. CSS Color 5 does
    // not yet define the color-space selection for this function.
    let color = color_to_srgb(color);
    0.2126 * srgb_component_to_linear(color.r as f64) as f32
        + 0.7152 * srgb_component_to_linear(color.g as f64) as f32
        + 0.0722 * srgb_component_to_linear(color.b as f64) as f32
}

/// Resolve the light branch of `light-dark()` in Quire's fixed light print
/// color scheme. CSS Color Adjustment selects the branch from the used
/// `color-scheme`: <https://www.w3.org/TR/css-color-adjust-1/#color-scheme-effect>.
fn parse_light_dark(value: &str, current: Color) -> Option<Color> {
    let inner = value.strip_prefix("light-dark(")?.strip_suffix(')')?;
    let components = split_top_level_commas(inner);
    let [light, _dark] = components.as_slice() else {
        return None;
    };
    if light.eq_ignore_ascii_case("currentcolor") {
        Some(current)
    } else {
        parse_color(light)
    }
}

/// Parse `color-mix(in srgb, <color> <percentage>?, <color> <percentage>?)`.
///
/// The mixing calculation uses premultiplied alpha and the normalization rules
/// from CSS Color 5: <https://www.w3.org/TR/css-color-5/#color-mix>.
pub(crate) fn parse_color_mix(value: &str, current: Color) -> Option<Color> {
    let inner = value.strip_prefix("color-mix(")?.strip_suffix(')')?;
    let arguments = split_top_level_commas(inner);
    let [interpolation, left, right] = arguments.as_slice() else {
        return None;
    };
    let interpolation = interpolation.trim().to_ascii_lowercase();
    if !matches!(interpolation.as_str(), "in srgb" | "in lch") {
        return None;
    }
    let (left, left_percentage) = split_color_mix_component(left)?;
    let (right, right_percentage) = split_color_mix_component(right)?;
    let left = parse_color_mix_input(left, current)?;
    let right = parse_color_mix_input(right, current)?;
    match interpolation.as_str() {
        "in srgb" => mix_srgb(left, right, left_percentage, right_percentage),
        "in lch" => mix_lch(left, right, left_percentage, right_percentage),
        _ => unreachable!(),
    }
}

fn split_top_level_commas(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(value[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}

fn split_color_mix_component(value: &str) -> Option<(&str, Option<f32>)> {
    let value = value.trim();
    let mut depth = 0;
    let mut split = None;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            character if character.is_ascii_whitespace() && depth == 0 => split = Some(index),
            _ => {}
        }
    }
    let Some(split) = split else {
        return Some((value, None));
    };
    let color = value[..split].trim_end();
    let suffix = value[split..].trim();
    let percentage = suffix.strip_suffix('%')?.trim().parse::<f32>().ok()?;
    Some((color, Some(percentage)))
}

fn parse_color_mix_input(value: &str, current: Color) -> Option<Color> {
    if value.eq_ignore_ascii_case("currentcolor") {
        Some(current)
    } else {
        parse_color_mix(value, current).or_else(|| parse_color(value))
    }
}

fn mix_srgb(
    left: Color,
    right: Color,
    left_percentage: Option<f32>,
    right_percentage: Option<f32>,
) -> Option<Color> {
    // This initial color-mix implementation supports the legacy `in srgb`
    // path only. Convert retained CSS Color 4 coordinates at that boundary.
    let left = color_to_srgb(left);
    let right = color_to_srgb(right);
    let (left_percentage, right_percentage) = match (left_percentage, right_percentage) {
        (Some(left), Some(right)) => (left, right),
        (Some(left), None) => (left, 100.0 - left),
        (None, Some(right)) => (100.0 - right, right),
        (None, None) => (50.0, 50.0),
    };
    if left_percentage < 0.0 || right_percentage < 0.0 {
        return None;
    }
    let total = left_percentage + right_percentage;
    if total == 0.0 {
        return None;
    }
    let alpha_multiplier = (total / 100.0).min(1.0);
    let left_weight = left_percentage / total;
    let right_weight = right_percentage / total;
    let alpha = (left.a * left_weight + right.a * right_weight) * alpha_multiplier;
    if alpha == 0.0 {
        return Some(Color::TRANSPARENT);
    }
    let mix = |left_channel: f32, right_channel: f32| {
        (left_channel * left.a * left_weight + right_channel * right.a * right_weight)
            * alpha_multiplier
            / alpha
    };
    Some(Color::srgb(
        mix(left.r, right.r),
        mix(left.g, right.g),
        mix(left.b, right.b),
        alpha,
    ))
}

fn mix_lch(
    left: Color,
    right: Color,
    left_percentage: Option<f32>,
    right_percentage: Option<f32>,
) -> Option<Color> {
    let (left_percentage, right_percentage) =
        normalize_color_mix_percentages(left_percentage, right_percentage)?;
    let total = left_percentage + right_percentage;
    let alpha_multiplier = (total / 100.0).min(1.0);
    let left_weight = left_percentage / total;
    let right_weight = right_percentage / total;
    let alpha = (left.a * left_weight + right.a * right_weight) * alpha_multiplier;
    if alpha == 0.0 {
        return Some(Color::TRANSPARENT);
    }
    let [left_lightness, left_chroma, left_hue] = srgb_to_lch(left);
    let [right_lightness, right_chroma, right_hue] = srgb_to_lch(right);
    let component = |left_component: f64, right_component: f64| {
        (left_component * left.a as f64 * left_weight as f64
            + right_component * right.a as f64 * right_weight as f64)
            * alpha_multiplier as f64
            / alpha as f64
    };
    let hue_difference = (right_hue - left_hue + 180.0).rem_euclid(360.0) - 180.0;
    let hue = (left_hue + hue_difference * right_weight as f64).rem_euclid(360.0);
    let rgb = lch_to_srgb(
        component(left_lightness, right_lightness),
        component(left_chroma, right_chroma),
        hue,
    );
    Some(Color::srgb(
        rgb[0] as f32,
        rgb[1] as f32,
        rgb[2] as f32,
        alpha,
    ))
}

fn normalize_color_mix_percentages(
    left_percentage: Option<f32>,
    right_percentage: Option<f32>,
) -> Option<(f32, f32)> {
    let (left, right) = match (left_percentage, right_percentage) {
        (Some(left), Some(right)) => (left, right),
        (Some(left), None) => (left, 100.0 - left),
        (None, Some(right)) => (100.0 - right, right),
        (None, None) => (50.0, 50.0),
    };
    (left >= 0.0 && right >= 0.0 && left + right > 0.0).then_some((left, right))
}

/// CSS Syntax Level 3 comments are whitespace tokens, including inside color
/// functions: <https://www.w3.org/TR/css-syntax-3/#comment-diagram>.
fn remove_css_comments(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut remainder = value;
    while let Some(start) = remainder.find("/*") {
        result.push_str(&remainder[..start]);
        let after_start = &remainder[start + 2..];
        let Some(end) = after_start.find("*/") else {
            return value.to_string();
        };
        result.push(' ');
        remainder = &after_start[end + 2..];
    }
    result.push_str(remainder);
    result
}

/// Resolve CSS system colors to Quire's deterministic print palette.
///
/// CSS Color 4 leaves these colors dependent on the user agent and operating
/// system, while requiring deprecated system-color aliases to equal their
/// modern counterparts: <https://www.w3.org/TR/css-color-4/#css-system-colors>.
fn parse_system_color(value: &str) -> Option<Color> {
    let color = match value {
        "canvas" | "buttonface" | "buttonhighlight" | "buttonshadow" | "threedface" | "field"
        | "mark" | "highlight" | "selecteditem" | "accentcolor" | "activetext"
        | "activecaption" | "inactivecaption" | "appworkspace" | "background"
        | "infobackground" | "menu" | "scrollbar" | "window" => Color::WHITE,
        "canvastext"
        | "buttontext"
        | "fieldtext"
        | "marktext"
        | "highlighttext"
        | "selecteditemtext"
        | "accentcolortext"
        | "linktext"
        | "visitedtext"
        | "graytext"
        | "captiontext"
        | "inactivecaptiontext"
        | "infotext"
        | "menutext"
        | "windowtext" => Color::BLACK,
        "buttonborder" | "activeborder" | "inactiveborder" | "threeddarkshadow"
        | "threedhighlight" | "threedlightshadow" | "threedshadow" | "windowframe" => {
            Color::new(128, 128, 128)
        }
        _ => return None,
    };
    Some(color)
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
        let rgb_parts = parts.iter().take(3).cloned();
        let alpha_part = parts
            .get(3)
            .cloned()
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
                .cloned()
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
                .cloned()
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

/// Parse CSS Color 4's predefined `color()` spaces into Quire's sRGB paint
/// representation. Color conversion follows the conversion matrices and
/// transfer functions in <https://www.w3.org/TR/css-color-4/#color-conversion-code>.
pub(crate) fn parse_color_function(value: &str) -> Option<Color> {
    let inner = value
        .strip_prefix("color(")
        .and_then(|value| value.strip_suffix(')'))?;
    let (components, slash_alpha) = split_rgb_alpha(inner);
    let parts = components.split_whitespace().collect::<Vec<_>>();
    let [space, red, green, blue] = parts.as_slice() else {
        return None;
    };
    let alpha = if let Some(alpha) = slash_alpha {
        parse_alpha_value(alpha)?
    } else {
        1.0
    };
    let components = [
        parse_srgb_color_component(red)? as f64,
        parse_srgb_color_component(green)? as f64,
        parse_srgb_color_component(blue)? as f64,
    ];
    let [r, g, b] = components.map(|component| component as f32);
    match *space {
        "srgb" => Some(Color::in_space(ColorSpace::Srgb, r, g, b, alpha)),
        "srgb-linear" => {
            let encoded = linear_to_srgb(components);
            Some(Color::in_space(
                ColorSpace::Srgb,
                encoded[0] as f32,
                encoded[1] as f32,
                encoded[2] as f32,
                alpha,
            ))
        }
        "display-p3" => Some(Color::in_space(ColorSpace::DisplayP3, r, g, b, alpha)),
        "display-p3-linear" => {
            let encoded = linear_to_srgb(components);
            Some(Color::in_space(
                ColorSpace::DisplayP3,
                encoded[0] as f32,
                encoded[1] as f32,
                encoded[2] as f32,
                alpha,
            ))
        }
        "a98-rgb" => Some(Color::in_space(ColorSpace::A98Rgb, r, g, b, alpha)),
        "prophoto-rgb" => Some(Color::in_space(ColorSpace::ProphotoRgb, r, g, b, alpha)),
        "rec2020" => Some(Color::in_space(ColorSpace::Rec2020, r, g, b, alpha)),
        "xyz" | "xyz-d65" => {
            let xyz = adapt_d65_to_d50(components);
            Some(Color::in_space(
                ColorSpace::XyzD50,
                xyz[0] as f32,
                xyz[1] as f32,
                xyz[2] as f32,
                alpha,
            ))
        }
        "xyz-d50" => Some(Color::in_space(ColorSpace::XyzD50, r, g, b, alpha)),
        _ => None,
    }
}

/// Parse `lab()` using the D50 CIE Lab space defined by CSS Color 4.
fn parse_lab_function(value: &str) -> Option<Color> {
    let ([lightness, a, b], alpha) = parse_four_component_function(value, "lab(")?;
    let lightness = parse_lab_lightness(lightness)? as f64;
    let a = parse_lab_axis(a)? as f64;
    let b = parse_lab_axis(b)? as f64;
    let xyz = lab_to_xyz_d50(lightness, a, b);
    Some(Color::in_space(
        ColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        alpha,
    ))
}

/// Parse `lch()` using CSS Color 4's D50 CIE LCH space.
fn parse_lch_function(value: &str) -> Option<Color> {
    let ([lightness, chroma, hue], alpha) = parse_four_component_function(value, "lch(")?;
    let lightness = parse_lab_lightness(lightness)? as f64;
    let chroma = parse_lch_chroma(chroma)? as f64;
    let hue = parse_hue_degrees(hue)? as f64;
    let radians = hue.to_radians();
    let xyz = lab_to_xyz_d50(lightness, chroma * radians.cos(), chroma * radians.sin());
    Some(Color::in_space(
        ColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        alpha,
    ))
}

/// Parse `oklab()` according to CSS Color 4's D65 OKLab conversion.
fn parse_oklab_function(value: &str) -> Option<Color> {
    let ([lightness, a, b], alpha) = parse_four_component_function(value, "oklab(")?;
    let rgb = oklab_to_srgb(
        parse_oklab_lightness(lightness)? as f64,
        parse_oklab_axis(a)? as f64,
        parse_oklab_axis(b)? as f64,
    );
    let xyz = adapt_d65_to_d50(srgb_to_xyz_d65(rgb));
    Some(Color::in_space(
        ColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        alpha,
    ))
}

/// Parse `oklch()` according to CSS Color 4's D65 OKLCH conversion.
fn parse_oklch_function(value: &str) -> Option<Color> {
    let ([lightness, chroma, hue], alpha) = parse_four_component_function(value, "oklch(")?;
    let lightness = parse_oklab_lightness(lightness)? as f64;
    let chroma = parse_oklch_chroma(chroma)? as f64;
    let radians = (parse_hue_degrees(hue)? as f64).to_radians();
    let rgb = oklab_to_srgb(lightness, chroma * radians.cos(), chroma * radians.sin());
    let xyz = adapt_d65_to_d50(srgb_to_xyz_d65(rgb));
    Some(Color::in_space(
        ColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        alpha,
    ))
}

/// Parse the modern, space-separated three-component color function grammar.
fn parse_four_component_function<'a>(value: &'a str, prefix: &str) -> Option<([&'a str; 3], f32)> {
    let inner = value.strip_prefix(prefix)?.strip_suffix(')')?;
    let (components, slash_alpha) = split_rgb_alpha(inner);
    let components = components.split_whitespace().collect::<Vec<_>>();
    let [first, second, third] = components.as_slice() else {
        return None;
    };
    Some((
        [*first, *second, *third],
        slash_alpha.map(parse_alpha_value).unwrap_or(Some(1.0))?,
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

fn parse_lab_lightness(value: &str) -> Option<f32> {
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    value
        .strip_suffix('%')
        .and_then(|percent| percent.trim().parse::<f32>().ok())
        .or_else(|| value.parse::<f32>().ok())
        .map(|lightness| lightness.clamp(0.0, 100.0))
}

fn parse_oklab_lightness(value: &str) -> Option<f32> {
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    value
        .strip_suffix('%')
        .and_then(|percent| percent.trim().parse::<f32>().ok())
        .map(|percent| percent / 100.0)
        .or_else(|| value.parse::<f32>().ok())
        .map(|lightness| {
            let lightness = lightness.clamp(0.0, 1.0);
            if lightness <= 0.000_001 {
                0.0
            } else if lightness >= 0.999_999 {
                1.0
            } else {
                lightness
            }
        })
}

fn parse_lab_axis(value: &str) -> Option<f32> {
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    value
        .strip_suffix('%')
        .and_then(|percent| percent.trim().parse::<f32>().ok())
        .map(|percent| percent * 1.25)
        .or_else(|| value.parse::<f32>().ok())
}

fn parse_lch_chroma(value: &str) -> Option<f32> {
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    value
        .strip_suffix('%')
        .and_then(|percent| percent.trim().parse::<f32>().ok())
        .map(|percent| percent * 1.5)
        .or_else(|| value.parse::<f32>().ok())
}

fn parse_oklab_axis(value: &str) -> Option<f32> {
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    value
        .strip_suffix('%')
        .and_then(|percent| percent.trim().parse::<f32>().ok())
        .map(|percent| percent * 0.004)
        .or_else(|| value.parse::<f32>().ok())
}

fn parse_oklch_chroma(value: &str) -> Option<f32> {
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    value
        .strip_suffix('%')
        .and_then(|percent| percent.trim().parse::<f32>().ok())
        .map(|percent| percent * 0.004)
        .or_else(|| value.parse::<f32>().ok())
}

type Triplet = [f64; 3];

fn multiply_matrix(matrix: [[f64; 3]; 3], values: Triplet) -> Triplet {
    matrix.map(|row| row[0] * values[0] + row[1] * values[1] + row[2] * values[2])
}

fn linear_to_srgb(linear: Triplet) -> Triplet {
    linear.map(linear_component_to_srgb)
}

fn linear_component_to_srgb(value: f64) -> f64 {
    let sign = value.signum();
    let magnitude = value.abs();
    sign * if magnitude > 0.003_130_8 {
        1.055 * magnitude.powf(1.0 / 2.4) - 0.055
    } else {
        12.92 * magnitude
    }
}

fn xyz_d65_to_srgb(xyz: Triplet) -> Triplet {
    linear_to_srgb(multiply_matrix(
        [
            [
                3.240_969_941_904_522_6,
                -1.537_383_177_570_094,
                -0.498_610_760_293_003_4,
            ],
            [
                -0.969_243_636_280_879_6,
                1.875_967_501_507_720_2,
                0.041_555_057_407_175_59,
            ],
            [
                0.055_630_079_696_993_66,
                -0.203_976_958_888_976_52,
                1.056_971_514_242_878_6,
            ],
        ],
        xyz,
    ))
}

fn linear_display_p3_to_xyz(values: Triplet) -> Triplet {
    linear_display_p3_to_xyz_linear(values.map(srgb_component_to_linear))
}

fn linear_display_p3_to_xyz_linear(values: Triplet) -> Triplet {
    multiply_matrix(
        [
            [
                0.486_570_948_648_216_2,
                0.265_667_693_169_093_06,
                0.198_217_285_234_362_5,
            ],
            [
                0.228_974_564_069_748_8,
                0.691_738_521_836_506_4,
                0.079_286_914_093_745,
            ],
            [0.0, 0.045_113_381_858_902_64, 1.043_944_368_900_976],
        ],
        values.map(srgb_component_to_linear),
    )
}

fn linear_a98_rgb_to_xyz(values: Triplet) -> Triplet {
    multiply_matrix(
        [
            [
                0.576_669_042_910_130_5,
                0.185_558_237_906_546_3,
                0.188_228_646_234_994_7,
            ],
            [
                0.297_344_975_250_536_05,
                0.627_363_566_255_466_1,
                0.075_291_458_493_997_88,
            ],
            [
                0.027_031_361_386_412_34,
                0.070_688_852_535_827_23,
                0.991_337_536_837_738_8,
            ],
        ],
        values,
    )
}

fn linear_prophoto_rgb_to_xyz(values: Triplet) -> Triplet {
    multiply_matrix(
        [
            [
                0.797_760_489_672_302_4,
                0.135_185_837_175_740_3,
                0.031_349_349_581_524_8,
            ],
            [
                0.288_071_128_229_293_4,
                0.711_843_217_810_101_4,
                0.000_085_653_960_605_3,
            ],
            [0.0, 0.0, 0.825_104_602_510_460_2],
        ],
        values,
    )
}

fn linear_rec2020_to_xyz(values: Triplet) -> Triplet {
    multiply_matrix(
        [
            [
                0.636_958_048_301_291_4,
                0.144_616_903_586_208_32,
                0.168_880_975_164_172_1,
            ],
            [
                0.262_700_212_011_267_1,
                0.677_998_071_518_870_8,
                0.059_301_716_469_861_96,
            ],
            [0.0, 0.028_072_693_049_087_428, 1.060_985_057_710_791],
        ],
        values,
    )
}

fn srgb_component_to_linear(value: f64) -> f64 {
    let sign = value.signum();
    let magnitude = value.abs();
    sign * if magnitude < 0.040_45 {
        magnitude / 12.92
    } else {
        ((magnitude + 0.055) / 1.055).powf(2.4)
    }
}

fn a98_to_linear(value: f64) -> f64 {
    value.signum() * value.abs().powf(563.0 / 256.0)
}

fn prophoto_to_linear(value: f64) -> f64 {
    let sign = value.signum();
    let magnitude = value.abs();
    sign * if magnitude <= 16.0 / 512.0 {
        magnitude / 16.0
    } else {
        magnitude.powf(1.8)
    }
}

fn rec2020_to_linear(value: f64) -> f64 {
    let sign = value.signum();
    let magnitude = value.abs();
    sign * if magnitude < 0.081_45 {
        magnitude / 4.5
    } else {
        ((magnitude + 0.099_3) / 1.099_3).powf(1.0 / 0.45)
    }
}

fn adapt_d50_to_d65(xyz: Triplet) -> Triplet {
    multiply_matrix(
        [
            [
                0.955_473_421_488_075,
                -0.023_098_454_948_764_71,
                0.063_259_308_661_021_7,
            ],
            [
                -0.028_369_709_333_863_7,
                1.009_995_398_081_304_1,
                0.021_041_441_191_917_323,
            ],
            [
                0.012_314_014_864_481_998,
                -0.020_507_649_298_898_964,
                1.330_365_926_242_124,
            ],
        ],
        xyz,
    )
}

fn adapt_d65_to_d50(xyz: Triplet) -> Triplet {
    multiply_matrix(
        [
            [
                1.047_929_820_840_548_8,
                0.022_946_793_341_019_088,
                -0.050_192_229_543_135_57,
            ],
            [
                0.029_627_815_688_159_344,
                0.990_434_484_573_249,
                -0.017_073_825_029_385_14,
            ],
            [
                -0.009_243_058_152_591_178,
                0.015_055_144_896_577_895,
                0.751_874_289_958_000_8,
            ],
        ],
        xyz,
    )
}

/// Convert a retained CSS color to the legacy sRGB paint boundary.
///
/// Non-gradient raster images and advanced color operations still use the
/// legacy sRGB boundary. Keeping the conversion explicit prevents those
/// subsystems from silently treating wide-gamut coordinates as DeviceRGB
/// components.
pub(crate) fn color_to_srgb(color: Color) -> Color {
    if color.space() == ColorSpace::Srgb {
        return Color::srgb(color.r, color.g, color.b, color.a);
    }
    let source = [color.r as f64, color.g as f64, color.b as f64];
    let rgb = match color.space() {
        ColorSpace::Srgb => unreachable!(),
        ColorSpace::DisplayP3 => xyz_d65_to_srgb(linear_display_p3_to_xyz(
            source.map(srgb_component_to_linear),
        )),
        ColorSpace::A98Rgb => xyz_d65_to_srgb(linear_a98_rgb_to_xyz(source.map(a98_to_linear))),
        ColorSpace::ProphotoRgb => xyz_d65_to_srgb(adapt_d50_to_d65(linear_prophoto_rgb_to_xyz(
            source.map(prophoto_to_linear),
        ))),
        ColorSpace::Rec2020 => {
            xyz_d65_to_srgb(linear_rec2020_to_xyz(source.map(rec2020_to_linear)))
        }
        ColorSpace::XyzD50 => xyz_d65_to_srgb(adapt_d50_to_d65(source)),
    };
    Color::srgb(rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, color.a)
}

fn srgb_to_xyz_d65(rgb: Triplet) -> Triplet {
    multiply_matrix(
        [
            [
                0.412_390_799_265_959_34,
                0.357_584_339_383_877_96,
                0.180_480_788_401_834_3,
            ],
            [
                0.212_639_005_871_510_27,
                0.715_168_678_767_755_9,
                0.072_192_315_360_733_71,
            ],
            [
                0.019_330_818_715_591_82,
                0.119_194_779_794_625_99,
                0.950_532_152_249_660_7,
            ],
        ],
        rgb.map(srgb_component_to_linear),
    )
}

fn srgb_to_lch(color: Color) -> Triplet {
    let color = color_to_srgb(color);
    let xyz_d65 = srgb_to_xyz_d65([color.r as f64, color.g as f64, color.b as f64]);
    let [lightness, a, b] = xyz_d50_to_lab(adapt_d65_to_d50(xyz_d65));
    let chroma = a.hypot(b);
    [lightness, chroma, b.atan2(a).to_degrees().rem_euclid(360.0)]
}

fn xyz_d50_to_lab(xyz: Triplet) -> Triplet {
    let [x, y, z] = [xyz[0] / 0.964_22, xyz[1], xyz[2] / 0.825_21];
    let epsilon = 216.0 / 24_389.0;
    let kappa = 24_389.0 / 27.0;
    let xyz_to_f = |value: f64| {
        if value > epsilon {
            value.cbrt()
        } else {
            (kappa * value + 16.0) / 116.0
        }
    };
    let [fx, fy, fz] = [xyz_to_f(x), xyz_to_f(y), xyz_to_f(z)];
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn lch_to_srgb(lightness: f64, chroma: f64, hue: f64) -> Triplet {
    let hue = hue.to_radians();
    xyz_d65_to_srgb(adapt_d50_to_d65(lab_to_xyz_d50(
        lightness,
        chroma * hue.cos(),
        chroma * hue.sin(),
    )))
}

fn lab_to_xyz_d50(lightness: f64, a: f64, b: f64) -> Triplet {
    let f1 = (lightness + 16.0) / 116.0;
    let f0 = a / 500.0 + f1;
    let f2 = f1 - b / 200.0;
    let epsilon = 216.0 / 24_389.0;
    let kappa = 24_389.0 / 27.0;
    let f_to_xyz = |value: f64| {
        let cube = value.powi(3);
        if cube > epsilon {
            cube
        } else {
            (116.0 * value - 16.0) / kappa
        }
    };
    [
        f_to_xyz(f0) * 0.964_22,
        f_to_xyz(f1),
        f_to_xyz(f2) * 0.825_21,
    ]
}

fn oklab_to_srgb(lightness: f64, a: f64, b: f64) -> Triplet {
    let l = (lightness + 0.396_337_777_376_174_9 * a + 0.215_803_757_309_913_6 * b).powi(3);
    let m = (lightness - 0.105_561_345_815_658_6 * a - 0.063_854_172_825_813_3 * b).powi(3);
    let s = (lightness - 0.089_484_177_529_811_9 * a - 1.291_485_548_019_409_2 * b).powi(3);
    linear_to_srgb(multiply_matrix(
        [
            [
                4.076_741_636_075_958,
                -3.307_711_539_258_062,
                0.230_969_903_182_104,
            ],
            [
                -1.268_438_004_092_176_3,
                2.609_757_401_154,
                -0.341_319_397_061_877,
            ],
            [
                -0.004_196_086_541_837_188,
                -0.703_418_614_459_449_3,
                1.707_614_700_999_286,
            ],
        ],
        [l, m, s],
    ))
}

fn parse_hue_degrees(value: &str) -> Option<f32> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
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

fn srgb_to_hsl(color: Color) -> (f32, f32, f32) {
    let color = color_to_srgb(color);
    let maximum = color.r.max(color.g).max(color.b);
    let minimum = color.r.min(color.g).min(color.b);
    let lightness = (maximum + minimum) / 2.0;
    let delta = maximum - minimum;
    if delta == 0.0 {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if maximum == color.r {
        60.0 * ((color.g - color.b) / delta).rem_euclid(6.0)
    } else if maximum == color.g {
        60.0 * ((color.b - color.r) / delta + 2.0)
    } else {
        60.0 * ((color.r - color.g) / delta + 4.0)
    };
    (hue, saturation, lightness)
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

#[cfg(test)]
mod color_space_tests {
    use super::*;

    #[test]
    fn predefined_rgb_spaces_retain_coordinates_until_output() {
        let cases = [
            ("color(srgb 1.2 -0.1 0.3)", ColorSpace::Srgb),
            ("color(display-p3 1.2 -0.1 0.3)", ColorSpace::DisplayP3),
            ("color(a98-rgb 1.1 0.2 0.3)", ColorSpace::A98Rgb),
            ("color(prophoto-rgb 1.3 0.2 0.3)", ColorSpace::ProphotoRgb),
            ("color(rec2020 1.1 0.2 0.3)", ColorSpace::Rec2020),
        ];
        for (input, expected_space) in cases {
            let color = parse_color(input).unwrap();
            assert_eq!(color.space(), expected_space, "{input}");
            assert!(color.r > 1.0 || color.g < 0.0, "{input}");
        }
    }

    #[test]
    fn xyz_d65_is_adapted_to_retained_d50_pcs() {
        // CSS Color 4's D65 reference white adapted to D50.
        let color = parse_color("color(xyz-d65 .950455927 1 1.089057751)").unwrap();
        assert_eq!(color.space(), ColorSpace::XyzD50);
        assert!((color.r - 0.96422).abs() < 0.0001);
        assert!((color.g - 1.0).abs() < 0.0001);
        assert!((color.b - 0.82510).abs() < 0.0001);
    }

    #[test]
    fn lab_and_oklab_normalize_to_unbounded_d50_xyz() {
        for input in [
            "lab(50% 120 -110)",
            "lch(50% 160 35)",
            "oklab(0.7 0.3 -0.2)",
            "oklch(0.7 0.4 35)",
        ] {
            let color = parse_color(input).unwrap();
            assert_eq!(color.space(), ColorSpace::XyzD50, "{input}");
            assert!(color.a == 1.0);
        }
    }
}
