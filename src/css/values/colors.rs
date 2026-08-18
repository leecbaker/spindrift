use super::*;
use crate::css::component_values::split_css_component_values;
use palette::{
    Lab, Oklab, Xyz,
    convert::FromColorUnclamped,
    white_point::{D50, D65},
};

/// Whether a parsed CSS color value contains the `currentcolor` keyword.
///
/// This deliberately walks CSS component values rather than searching source
/// text, so comments, strings, escapes, and nested color functions retain
/// their CSS Syntax meaning.
pub(crate) fn color_depends_on_currentcolor(value: &str) -> bool {
    fn contains_currentcolor(parser: &mut cssparser::Parser<'_, '_>) -> bool {
        let mut found = false;
        while let Ok(token) = parser.next_including_whitespace_and_comments() {
            let token = token.clone();
            match token {
                cssparser::Token::Ident(ident) if ident.eq_ignore_ascii_case("currentcolor") => {
                    found = true;
                }
                cssparser::Token::Function(_)
                | cssparser::Token::ParenthesisBlock
                | cssparser::Token::SquareBracketBlock
                | cssparser::Token::CurlyBracketBlock => {
                    let _ = parser.parse_nested_block(|nested| {
                        found |= contains_currentcolor(nested);
                        Ok::<_, cssparser::ParseError<'_, ()>>(())
                    });
                }
                _ => {}
            }
        }
        found
    }

    let mut input = cssparser::ParserInput::new(value);
    contains_currentcolor(&mut cssparser::Parser::new(&mut input))
}

pub(crate) fn parse_color(value: &str) -> Option<CssColor> {
    let value = normalize_css_comments(trim_css_value(value));
    let value = value.trim();
    if let Some(ident) = crate::css::component_values::css_single_ident(value) {
        let ident = ident.to_ascii_lowercase();
        if ident == "transparent" {
            return Some(CssColor::TRANSPARENT);
        }
        if ident == "none" {
            return None;
        }
        if let Some(color) = parse_system_color(&ident) {
            return Some(color);
        }
        let (r, g, b) = cssparser::color::parse_named_color(&ident).ok()?;
        return Some(CssColor::new(r, g, b));
    }
    if let Some(hex) = value.strip_prefix('#') {
        let (r, g, b, a) = cssparser::color::parse_hash_color(hex.as_bytes()).ok()?;
        return Some(CssColor::rgba(r, g, b, a));
    }
    if let Some(rgb) = parse_rgb_function(value) {
        return Some(rgb);
    }
    if let Some(hsl) = parse_hsl_function(value) {
        return Some(hsl);
    }
    if let Some(hwb) = parse_hwb_function(value) {
        return Some(hwb);
    }
    if let Some(lab) = parse_lab_function(value) {
        return Some(lab);
    }
    if let Some(relative_lch) = parse_relative_lch_function(value) {
        return Some(relative_lch);
    }
    if let Some(lch) = parse_lch_function(value) {
        return Some(lch);
    }
    if let Some(oklab) = parse_oklab_function(value) {
        return Some(oklab);
    }
    if let Some(oklch) = parse_oklch_function(value) {
        return Some(oklch);
    }
    if let Some(color_function) = parse_color_function(value) {
        return Some(color_function);
    }
    if !value.to_ascii_lowercase().contains("currentcolor") {
        if let Some(color_mix) = parse_color_mix(value, CssColor::BLACK) {
            return Some(color_mix);
        }
        if !value.to_ascii_lowercase().contains("currentcolor")
            && let Some(contrast_color) = parse_contrast_color(value, CssColor::BLACK)
        {
            return Some(contrast_color);
        }
    }
    None
}

/// Resolve the relative-color forms whose origin is `currentcolor` against a
/// computed foreground color. CSS CssColor 5 retains the relative syntax through
/// inheritance and resolves its origin at used-value time:
/// <https://www.w3.org/TR/css-color-5/#relative-colors>.
pub(crate) fn parse_color_from_currentcolor(value: &str, current: CssColor) -> Option<CssColor> {
    parse_color_from_currentcolor_in_scheme(value, current, UsedColorScheme::Light)
}

/// Parses a color whose computed value depends on the owning element's used
/// `color-scheme`.
pub(crate) fn parse_color_from_currentcolor_in_scheme(
    value: &str,
    current: CssColor,
    used_color_scheme: UsedColorScheme,
) -> Option<CssColor> {
    let value = normalize_css_comments(trim_css_value(value));
    let value = value.trim().to_ascii_lowercase();
    if value == "currentcolor" {
        return Some(current);
    }
    if let Some(contrast_color) = parse_contrast_color(&value, current) {
        return Some(contrast_color);
    }
    if let Some(color) = parse_light_dark(&value, current, used_color_scheme) {
        return Some(color);
    }
    if let Some(color_mix) = parse_color_mix(&value, current) {
        return Some(color_mix);
    }
    let (function, body) = crate::css::component_values::css_single_function(&value)?;
    let components = crate::css::component_values::split_css_component_values(body);
    let [from, origin, components @ ..] = components.as_slice() else {
        return None;
    };
    if !from.eq_ignore_ascii_case("from") || !origin.eq_ignore_ascii_case("currentcolor") {
        return None;
    }
    match function.to_ascii_lowercase().as_str() {
        "rgb" | "rgba" if components.len() == 3 => {
            let current = current.to_rgb_space(RgbColorSpace::Srgb);
            let current_components = current.components();
            let channel = |name: &str| match name {
                "r" => Some(current_components[0]),
                "g" => Some(current_components[1]),
                "b" => Some(current_components[2]),
                _ => None,
            };
            Some(CssColor::rgb(
                RgbColorSpace::Srgb,
                channel(components[0])?,
                channel(components[1])?,
                channel(components[2])?,
                current.alpha(),
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
            let (r, g, b) = hsl_to_rgb_units_unclamped(hue, saturation, lightness);
            Some(CssColor::rgb(RgbColorSpace::Srgb, r, g, b, current.alpha()))
        }
        "hwb" if components == ["h", "w", "b"] => Some(current),
        "lab" | "oklab" if components == ["l", "a", "b"] => Some(current),
        "lch" | "oklch" if components == ["l", "c", "h"] => Some(current),
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

/// Parse CSS CssColor 5 `contrast-color()`.
///
/// The function must resolve to whichever of black and white has the greater
/// contrast against its argument. CSS leaves the exact contrast algorithm to
/// the user agent; Quire uses the WCAG relative-luminance contrast ratio,
/// whose monotonic ordering makes this choice well-defined:
/// <https://www.w3.org/TR/css-color-5/#contrast-color>.
fn parse_contrast_color(value: &str, current: CssColor) -> Option<CssColor> {
    let inner = crate::css::component_values::css_function_body(value, "contrast-color")?;
    let background = if inner.trim().eq_ignore_ascii_case("currentcolor") {
        current
    } else {
        parse_color(inner.trim())?
    };
    let luminance = relative_luminance(background);
    let black_contrast = (luminance + 0.05) / 0.05;
    let white_contrast = 1.05 / (luminance + 0.05);
    Some(if black_contrast >= white_contrast {
        CssColor::BLACK
    } else {
        CssColor::WHITE
    })
}

fn relative_luminance(color: CssColor) -> f32 {
    // The WCAG contrast calculation is defined for sRGB. CSS CssColor 5 does
    // not yet define the color-space selection for this function.
    let color = color_to_predefined_rgb(color, CssColorSpace::Srgb)
        .expect("sRGB is a predefined CSS RGB space");
    0.2126 * srgb_component_to_linear(color.components()[0] as f64) as f32
        + 0.7152 * srgb_component_to_linear(color.components()[1] as f64) as f32
        + 0.0722 * srgb_component_to_linear(color.components()[2] as f64) as f32
}

/// Resolve the light branch of `light-dark()` in Quire's fixed light print
/// color scheme. CSS CssColor Adjustment selects the branch from the used
/// `color-scheme`: <https://www.w3.org/TR/css-color-adjust-1/#color-scheme-effect>.
fn parse_light_dark(
    value: &str,
    current: CssColor,
    used_color_scheme: UsedColorScheme,
) -> Option<CssColor> {
    let inner = crate::css::component_values::css_function_body(value, "light-dark")?;
    let components = split_top_level_commas(inner);
    let [light, dark] = components.as_slice() else {
        return None;
    };
    let selected = match used_color_scheme {
        UsedColorScheme::Light => light,
        UsedColorScheme::Dark => dark,
    };
    if selected.eq_ignore_ascii_case("currentcolor") {
        Some(current)
    } else {
        parse_color(selected)
    }
}

/// Parse a CSS CssColor 5 `color-mix()` value.
///
/// The mixing calculation shares the CSS CssColor interpolation implementation
/// used by generated gradients. This keeps the interpolation color space,
/// polar hue route, premultiplied alpha, and missing-component handling
/// consistent for every consumer of a CSS color.
/// <https://drafts.csswg.org/css-color-5/#color-mix>
pub(crate) fn parse_color_mix(value: &str, current: CssColor) -> Option<CssColor> {
    let inner = crate::css::component_values::css_function_body(value, "color-mix")?;
    let arguments = split_top_level_commas(inner);
    if arguments.is_empty() {
        return None;
    }
    let (method, items) = match parse_color_mix_interpolation_method(arguments[0]) {
        Some(method) => (method, &arguments[1..]),
        None if !arguments[0].trim_start().starts_with("in ") => (
            crate::css::GradientInterpolationMethod::default(),
            arguments.as_slice(),
        ),
        None => return None,
    };
    let items = items
        .iter()
        .map(|item| parse_color_mix_item(item, current))
        .collect::<Option<Vec<_>>>()?;
    if items.is_empty() {
        return None;
    }
    mix_colors(items, method)
}

/// Parse the optional `in <color-space> [<hue-interpolation-method> hue]?`
/// prefix used by `color-mix()`. The equivalent gradient grammar is parsed in
/// the background cascade, but color values need the same semantic method.
fn parse_color_mix_interpolation_method(
    value: &str,
) -> Option<crate::css::GradientInterpolationMethod> {
    use crate::css::{GradientInterpolationSpace as Space, HueInterpolationMethod as Hue};

    let tokens = crate::css::component_values::split_css_component_values(value);
    let [in_keyword, space, tail @ ..] = tokens.as_slice() else {
        return None;
    };
    if !in_keyword.eq_ignore_ascii_case("in") {
        return None;
    }
    let space = match space.to_ascii_lowercase().as_str() {
        "srgb" => Space::Srgb,
        "srgb-linear" => Space::SrgbLinear,
        "display-p3" => Space::DisplayP3,
        "display-p3-linear" => Space::DisplayP3Linear,
        "a98-rgb" => Space::A98Rgb,
        "prophoto-rgb" => Space::ProphotoRgb,
        "rec2020" => Space::Rec2020,
        "xyz-d50" => Space::XyzD50,
        "xyz" | "xyz-d65" => Space::XyzD65,
        "lab" => Space::Lab,
        "oklab" => Space::Oklab,
        "hsl" => Space::Hsl,
        "hwb" => Space::Hwb,
        "lch" => Space::Lch,
        "oklch" => Space::Oklch,
        _ => return None,
    };
    let hue = match tail {
        [] => Hue::Shorter,
        [method, keyword] if space.is_polar() && keyword.eq_ignore_ascii_case("hue") => {
            match method.to_ascii_lowercase().as_str() {
                "shorter" => Hue::Shorter,
                "longer" => Hue::Longer,
                "increasing" => Hue::Increasing,
                "decreasing" => Hue::Decreasing,
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(crate::css::GradientInterpolationMethod { space, hue })
}

fn split_top_level_commas(value: &str) -> Vec<&str> {
    crate::css::component_values::split_css_top_level_delimiter(value, ',')
}

fn split_color_mix_component(value: &str) -> Option<(&str, Option<f32>)> {
    let components = crate::css::component_values::split_css_component_values(value);
    match components.as_slice() {
        [color] => Some((*color, None)),
        [color, percentage] => {
            let percentage = percentage.strip_suffix('%')?.trim().parse::<f32>().ok()?;
            (0.0..=100.0)
                .contains(&percentage)
                .then_some((*color, Some(percentage)))
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct ColorMixItem {
    color: CssColor,
    percentage: Option<f32>,
    missing: crate::css::GradientMissingComponents,
    missing_source: crate::css::GradientMissingComponentSpace,
}

fn parse_color_mix_item(value: &str, current: CssColor) -> Option<ColorMixItem> {
    let (value, percentage) = split_color_mix_component(value)?;
    let (color, missing, missing_source) = if value.eq_ignore_ascii_case("currentcolor") {
        (
            current,
            crate::css::GradientMissingComponents::default(),
            crate::css::GradientMissingComponentSpace::Rgb,
        )
    } else {
        let color = parse_color_mix(value, current).or_else(|| parse_color(value))?;
        let (missing, missing_source) = color_missing_components(value);
        (color, missing, missing_source)
    };
    Some(ColorMixItem {
        color,
        percentage,
        missing,
        missing_source,
    })
}

/// Normalize percentages, reduce the color list in source order, then apply
/// the alpha multiplier for an underspecified total.
/// <https://drafts.csswg.org/css-color-5/#color-mix>
fn mix_colors(
    mut items: Vec<ColorMixItem>,
    method: crate::css::GradientInterpolationMethod,
) -> Option<CssColor> {
    let specified = items.iter().filter_map(|item| item.percentage).sum::<f32>();
    let omitted = items
        .iter()
        .filter(|item| item.percentage.is_none())
        .count();
    let implied = (100.0 - specified) / omitted.max(1) as f32;
    if omitted > 0 && implied < 0.0 {
        return None;
    }
    for item in &mut items {
        item.percentage = Some(item.percentage.unwrap_or(implied));
    }
    let total = items
        .iter()
        .map(|item| item.percentage.unwrap())
        .sum::<f32>();
    if total == 0.0 {
        return Some(CssColor::TRANSPARENT);
    }
    let alpha_multiplier = (total / 100.0).min(1.0);
    let mut accumulated = items.remove(0);
    let mut accumulated_percentage = accumulated.percentage.unwrap();
    for item in items {
        let percentage = item.percentage.unwrap();
        let combined = accumulated_percentage + percentage;
        let progress = if combined == 0.0 {
            0.5
        } else {
            percentage / combined
        };
        accumulated.color = crate::color::interpolate_color_with_missing(
            accumulated.color,
            item.color,
            method,
            progress,
            missing_components_for_color_mix(accumulated, method),
            missing_components_for_color_mix(item, method),
        );
        accumulated.missing = crate::css::GradientMissingComponents::default();
        accumulated.missing_source = crate::css::GradientMissingComponentSpace::Rgb;
        accumulated_percentage = combined;
    }
    accumulated.color = accumulated
        .color
        .with_alpha(accumulated.color.alpha() * alpha_multiplier);
    // A computed CSS color retains its interpolation space. Gamut mapping and
    // quantization are output operations, not part of `color-mix()`.
    Some(accumulated.color)
}

/// Record `none` components before ordinary color parsing substitutes their
/// numeric fallback. CSS CssColor applies the analogous-component fixup only in
/// the selected interpolation space.
/// <https://www.w3.org/TR/css-color-4/#interpolation-missing>
fn missing_components_for_color_mix(
    item: ColorMixItem,
    method: crate::css::GradientInterpolationMethod,
) -> u8 {
    crate::css::GradientColor::ColorWithMissing {
        color: item.color,
        missing: item.missing,
        source: item.missing_source,
    }
    .missing_components_for(method)
    .bits()
}

fn color_missing_components(
    value: &str,
) -> (
    crate::css::GradientMissingComponents,
    crate::css::GradientMissingComponentSpace,
) {
    use crate::css::{GradientMissingComponentSpace as Space, GradientMissingComponents};

    let Some((name, inner)) = crate::css::component_values::css_single_function(value.trim())
    else {
        return (GradientMissingComponents::default(), Space::Rgb);
    };
    let name = name.to_ascii_lowercase();
    if !matches!(
        name.as_str(),
        "rgb" | "rgba" | "hsl" | "hsla" | "hwb" | "lab" | "lch" | "oklab" | "oklch" | "color"
    ) {
        return (GradientMissingComponents::default(), Space::Rgb);
    }
    let (components, slash_alpha) =
        crate::css::component_values::split_css_top_level_once(inner, '/')
            .map(|(components, alpha)| (components, Some(alpha.trim())))
            .unwrap_or((inner, None));
    let tokens = crate::css::component_values::split_css_top_level_delimiter(components, ',')
        .into_iter()
        .flat_map(crate::css::component_values::split_css_component_values)
        .collect::<Vec<_>>();
    let component_offset = usize::from(name == "color");
    if tokens.len() < component_offset + 3 {
        return (GradientMissingComponents::default(), Space::Rgb);
    }
    let source = match name.as_str() {
        "rgb" | "rgba" => Space::Rgb,
        "hsl" | "hsla" => Space::Hsl,
        "hwb" => Space::Hwb,
        "lab" => Space::Lab,
        "lch" => Space::Lch,
        "oklab" => Space::Oklab,
        "oklch" => Space::Oklch,
        "color" => match tokens.first().copied() {
            Some("xyz") | Some("xyz-d50") | Some("xyz-d65") => Space::Xyz,
            _ => Space::Rgb,
        },
        _ => unreachable!("validated color function"),
    };
    let mut bits = 0;
    for component in 0..3 {
        if tokens[component + component_offset].eq_ignore_ascii_case("none") {
            bits |= 1 << component;
        }
    }
    if slash_alpha.is_some_and(|alpha| alpha.eq_ignore_ascii_case("none"))
        || slash_alpha.is_none()
            && tokens
                .get(component_offset + 3)
                .is_some_and(|alpha| alpha.eq_ignore_ascii_case("none"))
    {
        bits |= 1 << 3;
    }
    (GradientMissingComponents::new(bits), source)
}

/// Normalize CSS comments to a single whitespace character without touching
/// comment-looking text inside quoted strings or escaped source text.
///
/// CSS Syntax consumes comments as whitespace between component values. The
/// returned string is deliberately not trimmed: callers decide whether the
/// consuming grammar permits leading or trailing whitespace.
/// <https://www.w3.org/TR/css-syntax-3/#comment-diagram>
pub(crate) fn normalize_css_comments(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut characters = value.chars().peekable();
    let mut quote = None;
    while let Some(character) = characters.next() {
        if let Some(active_quote) = quote {
            result.push(character);
            if character == '\\' {
                if let Some(escaped) = characters.next() {
                    result.push(escaped);
                }
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        match character {
            '\\' => {
                result.push(character);
                if let Some(escaped) = characters.next() {
                    result.push(escaped);
                }
            }
            '"' | '\'' => {
                quote = Some(character);
                result.push(character);
            }
            '/' if characters.peek() == Some(&'*') => {
                characters.next();
                let mut previous_was_star = false;
                for comment_character in characters.by_ref() {
                    if previous_was_star && comment_character == '/' {
                        break;
                    }
                    previous_was_star = comment_character == '*';
                }
                result.push(' ');
            }
            _ => result.push(character),
        }
    }
    result
}

/// Resolve CSS system colors to Quire's deterministic print palette.
///
/// CSS CssColor 4 leaves these colors dependent on the user agent and operating
/// system, while requiring deprecated system-color aliases to equal their
/// modern counterparts: <https://www.w3.org/TR/css-color-4/#css-system-colors>.
fn parse_system_color(value: &str) -> Option<CssColor> {
    use crate::css::{ForcedColorPalette, SystemColor};

    let system = match value {
        "canvas" | "buttonhighlight" | "buttonshadow" | "threedface" | "activecaption"
        | "inactivecaption" | "appworkspace" | "background" | "infobackground" | "menu"
        | "scrollbar" | "window" => SystemColor::Canvas,
        "canvastext"
        | "captiontext"
        | "inactivecaptiontext"
        | "infotext"
        | "menutext"
        | "windowtext" => SystemColor::CanvasText,
        "linktext" => SystemColor::LinkText,
        "visitedtext" => SystemColor::VisitedText,
        "activetext" => SystemColor::ActiveText,
        "buttonface" => SystemColor::ButtonFace,
        "buttontext" => SystemColor::ButtonText,
        "buttonborder" | "activeborder" | "inactiveborder" | "threeddarkshadow"
        | "threedhighlight" | "threedlightshadow" | "threedshadow" | "windowframe" => {
            SystemColor::ButtonBorder
        }
        "field" => SystemColor::Field,
        "fieldtext" => SystemColor::FieldText,
        "highlight" => SystemColor::Highlight,
        "highlighttext" => SystemColor::HighlightText,
        "mark" => SystemColor::Mark,
        "marktext" => SystemColor::MarkText,
        "graytext" => SystemColor::GrayText,
        "accentcolor" => SystemColor::AccentColor,
        "accentcolortext" => SystemColor::AccentColorText,
        "selecteditem" => SystemColor::SelectedItem,
        "selecteditemtext" => SystemColor::SelectedItemText,
        _ => return None,
    };
    Some(CssColor::system(
        system,
        ForcedColorPalette::LIGHT.color(system),
    ))
}

/// Parses the currently modeled sRGB subset of CSS CssColor syntax.
///
/// CSS CssColor Level 4 allows both legacy comma-separated and modern
/// whitespace-separated `rgb()`/`rgba()` forms:
/// <https://www.w3.org/TR/css-color-4/#rgb-functions>.
pub(crate) fn parse_rgb_function(value: &str) -> Option<CssColor> {
    let (name, inner) = crate::css::component_values::css_single_function(value)?;
    if !matches!(name.to_ascii_lowercase().as_str(), "rgb" | "rgba") {
        return None;
    }
    let (rgb, alpha) = split_rgb_alpha(inner);
    let comma_parts = split_top_level_commas(rgb);
    let channels = if comma_parts.len() > 1 {
        let parts = comma_parts;
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
        let parts = crate::css::component_values::split_css_component_values(rgb);
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
        [r, g, b] => Some(CssColor::rgba(*r, *g, *b, alpha)),
        _ => None,
    }
}

fn split_rgb_alpha(value: &str) -> (&str, Option<&str>) {
    crate::css::component_values::split_css_top_level_once(value, '/')
        .map(|(components, alpha)| (components, Some(alpha.trim())))
        .unwrap_or((value.trim(), None))
}

fn parse_rgb_channel(value: &str) -> Option<u8> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(0);
    }
    let channel = if let Some(percent) = value.strip_suffix('%') {
        percent.trim().parse::<f32>().ok()? * 255.0 / 100.0
    } else {
        value.parse::<f32>().ok()?
    };
    Some(channel.round().clamp(0.0, 255.0) as u8)
}

/// Parse `hsl()` and `hsla()` color functions.
///
/// CSS CssColor Level 4 allows both legacy comma-separated and modern
/// whitespace-separated HSL forms, with optional slash alpha:
/// <https://www.w3.org/TR/css-color-4/#the-hsl-notation>.
pub(crate) fn parse_hsl_function(value: &str) -> Option<CssColor> {
    let (name, inner) = crate::css::component_values::css_single_function(value)?;
    if !matches!(name.to_ascii_lowercase().as_str(), "hsl" | "hsla") {
        return None;
    }
    let (hsl, slash_alpha) = split_rgb_alpha(inner);
    let comma_parts = split_top_level_commas(hsl);
    let (hue, saturation, lightness, alpha) = if comma_parts.len() > 1 {
        let parts = comma_parts;
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
        let parts = crate::css::component_values::split_css_component_values(hsl);
        if parts.len() != 3 {
            return None;
        }
        (
            parse_hue_degrees(parts[0])?,
            parse_modern_hsl_hwb_component(parts[1])?,
            parse_modern_hsl_hwb_component(parts[2])?,
            slash_alpha,
        )
    };
    let alpha = if let Some(alpha) = alpha {
        parse_alpha_value(alpha)?
    } else {
        1.0
    };
    let (r, g, b) = hsl_to_rgb(hue, saturation, lightness);
    Some(CssColor::rgba(r, g, b, alpha))
}

/// Parse `hwb()` color functions into sRGB.
///
/// CSS CssColor Level 4 defines HWB as a cylindrical sRGB color notation whose
/// hue is mixed with whiteness and blackness; when whiteness plus blackness is
/// at least 100%, the result is the corresponding gray:
/// <https://www.w3.org/TR/css-color-4/#the-hwb-notation>.
pub(crate) fn parse_hwb_function(value: &str) -> Option<CssColor> {
    let inner = crate::css::component_values::css_function_body(value, "hwb")?;
    let (hwb, slash_alpha) = split_rgb_alpha(inner);
    let comma_parts = split_top_level_commas(hwb);
    let (hue, whiteness, blackness, alpha) = if comma_parts.len() > 1 {
        let parts = comma_parts;
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
        let parts = crate::css::component_values::split_css_component_values(hwb);
        if parts.len() != 3 {
            return None;
        }
        (
            parse_hue_degrees(parts[0])?,
            parse_modern_hsl_hwb_component(parts[1])?,
            parse_modern_hsl_hwb_component(parts[2])?,
            slash_alpha,
        )
    };
    let alpha = if let Some(alpha) = alpha {
        parse_alpha_value(alpha)?
    } else {
        1.0
    };
    let (r, g, b) = hwb_to_rgb(hue, whiteness, blackness);
    Some(CssColor::rgba(r, g, b, alpha))
}

/// Parse a modern HSL/HWB saturation, lightness, whiteness, or blackness
/// component into its unit reference range.
///
/// CSS CssColor Level 4 permits a percentage, a number in the 0--100 reference
/// range, or `none` in modern (space-separated) syntax. `none` has a numeric
/// fallback of zero; gradient parsing separately preserves its missingness
/// until interpolation.
/// <https://www.w3.org/TR/css-color-4/#the-hsl-notation>
/// <https://www.w3.org/TR/css-color-4/#the-hwb-notation>
fn parse_modern_hsl_hwb_component(value: &str) -> Option<f32> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    parse_percentage(value).or_else(|| value.parse::<f32>().ok().map(|value| value / 100.0))
}

/// Parse CSS CssColor 4's predefined `color()` spaces into Quire's sRGB paint
/// representation. CssColor conversion follows the conversion matrices and
/// transfer functions in <https://www.w3.org/TR/css-color-4/#color-conversion-code>.
pub(crate) fn parse_color_function(value: &str) -> Option<CssColor> {
    let inner = crate::css::component_values::css_function_body(value, "color")?;
    let (components, slash_alpha) = split_rgb_alpha(inner);
    let parts = crate::css::component_values::split_css_component_values(components);
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
        "srgb" => Some(CssColor::in_space(CssColorSpace::Srgb, r, g, b, alpha)),
        "srgb-linear" => {
            let encoded = linear_to_srgb(components);
            Some(CssColor::in_space(
                CssColorSpace::Srgb,
                encoded[0] as f32,
                encoded[1] as f32,
                encoded[2] as f32,
                alpha,
            ))
        }
        "display-p3" => Some(CssColor::in_space(CssColorSpace::DisplayP3, r, g, b, alpha)),
        "display-p3-linear" => {
            let encoded = linear_to_srgb(components);
            Some(CssColor::in_space(
                CssColorSpace::DisplayP3,
                encoded[0] as f32,
                encoded[1] as f32,
                encoded[2] as f32,
                alpha,
            ))
        }
        "a98-rgb" => Some(CssColor::in_space(CssColorSpace::A98Rgb, r, g, b, alpha)),
        "prophoto-rgb" => Some(CssColor::in_space(
            CssColorSpace::ProphotoRgb,
            r,
            g,
            b,
            alpha,
        )),
        "rec2020" => Some(CssColor::in_space(CssColorSpace::Rec2020, r, g, b, alpha)),
        "xyz" | "xyz-d65" => {
            let xyz = adapt_d65_to_d50(components);
            Some(CssColor::in_space(
                CssColorSpace::XyzD50,
                xyz[0] as f32,
                xyz[1] as f32,
                xyz[2] as f32,
                alpha,
            ))
        }
        "xyz-d50" => Some(CssColor::in_space(CssColorSpace::XyzD50, r, g, b, alpha)),
        _ => None,
    }
}

/// Parse `lab()` using the D50 CIE Lab space defined by CSS CssColor 4.
fn parse_lab_function(value: &str) -> Option<CssColor> {
    let ([lightness, a, b], alpha) = parse_four_component_function(value, "lab")?;
    let lightness = parse_lab_lightness(lightness)? as f64;
    let a = parse_lab_axis(a)? as f64;
    let b = parse_lab_axis(b)? as f64;
    let xyz = lab_to_xyz_d50(lightness, a, b);
    Some(CssColor::in_space(
        CssColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        alpha,
    ))
}

/// Parse `lch()` using CSS CssColor 4's D50 CIE LCH space.
fn parse_lch_function(value: &str) -> Option<CssColor> {
    let ([lightness, chroma, hue], alpha) = parse_four_component_function(value, "lch")?;
    let lightness = parse_lab_lightness(lightness)? as f64;
    // Chroma has no physical direction at either CIE LCH lightness endpoint.
    // CSS clamps L before conversion, so these polar endpoint values resolve
    // to the neutral black/white colors rather than manufacturing an
    // out-of-gamut hue from an undefined chroma direction.
    // <https://www.w3.org/TR/css-color-4/#specifying-lab-lch>
    if lightness == 0.0 {
        return Some(CssColor::srgb(0.0, 0.0, 0.0, alpha));
    }
    if lightness == 100.0 {
        return Some(CssColor::srgb(1.0, 1.0, 1.0, alpha));
    }
    let chroma = parse_lch_chroma(chroma)? as f64;
    let hue = parse_hue_degrees(hue)? as f64;
    let radians = hue.to_radians();
    let xyz = lab_to_xyz_d50(lightness, chroma * radians.cos(), chroma * radians.sin());
    Some(CssColor::in_space(
        CssColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        alpha,
    ))
}

/// Parse a relative `lch(from <color> l c h)` color at computed-value time.
///
/// CSS CssColor resolves the origin in the target color space before evaluating
/// its component expressions. This parser retains the existing `lch()` D50
/// conversion and supports numeric multiplication of a referenced component,
/// which is the arithmetic form used by palette overrides and other static
/// stylesheet colors.
/// <https://www.w3.org/TR/css-color-5/#relative-colors>
fn parse_relative_lch_function(value: &str) -> Option<CssColor> {
    let inner = value.strip_prefix("lch(from ")?.strip_suffix(')')?;
    let components = split_css_component_values(inner);
    let [origin, lightness, chroma, hue] = components.as_slice() else {
        return None;
    };
    let origin = parse_color(origin)?;
    let [origin_lightness, origin_chroma, origin_hue] = color_to_lch(origin);
    let component = |value: &str, name: &str, origin_value: f64| {
        if value == name {
            return Some(origin_value);
        }
        let expression = value
            .strip_prefix("calc(")
            .and_then(|expression| expression.strip_suffix(')'))?
            .trim();
        let (factor, referenced) = expression.split_once('*')?;
        (referenced.trim() == name).then(|| {
            factor
                .trim()
                .parse::<f64>()
                .ok()
                .map(|factor| factor * origin_value)
        })?
    };
    let lightness = component(lightness, "l", origin_lightness)?;
    let chroma = component(chroma, "c", origin_chroma)?;
    let hue = component(hue, "h", origin_hue)?;
    let radians = hue.to_radians();
    let xyz = lab_to_xyz_d50(lightness, chroma * radians.cos(), chroma * radians.sin());
    Some(CssColor::in_space(
        CssColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        origin.alpha(),
    ))
}

/// Parse `oklab()` according to CSS CssColor 4's D65 OKLab conversion.
fn parse_oklab_function(value: &str) -> Option<CssColor> {
    let ([lightness, a, b], alpha) = parse_four_component_function(value, "oklab")?;
    let xyz = oklab_to_xyz_d65(
        parse_oklab_lightness(lightness)? as f64,
        parse_oklab_axis(a)? as f64,
        parse_oklab_axis(b)? as f64,
    );
    let xyz = adapt_d65_to_d50(xyz);
    Some(CssColor::in_space(
        CssColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        alpha,
    ))
}

/// Parse `oklch()` according to CSS CssColor 4's D65 OKLCH conversion.
fn parse_oklch_function(value: &str) -> Option<CssColor> {
    let ([lightness, chroma, hue], alpha) = parse_four_component_function(value, "oklch")?;
    let lightness = parse_oklab_lightness(lightness)? as f64;
    // As in CIE LCH, chroma is powerless at the polar OKLCH lightness
    // endpoints. Normalize after CSS lightness clamping and before polar
    // conversion so the endpoint is a neutral color.
    // <https://www.w3.org/TR/css-color-4/#specifying-oklab-oklch>
    if lightness == 0.0 {
        return Some(CssColor::srgb(0.0, 0.0, 0.0, alpha));
    }
    if lightness == 1.0 {
        return Some(CssColor::srgb(1.0, 1.0, 1.0, alpha));
    }
    let chroma = parse_oklch_chroma(chroma)? as f64;
    let radians = (parse_hue_degrees(hue)? as f64).to_radians();
    let xyz = adapt_d65_to_d50(oklab_to_xyz_d65(
        lightness,
        chroma * radians.cos(),
        chroma * radians.sin(),
    ));
    Some(CssColor::in_space(
        CssColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        alpha,
    ))
}

/// Parse the modern, space-separated three-component color function grammar.
fn parse_four_component_function<'a>(value: &'a str, name: &str) -> Option<([&'a str; 3], f32)> {
    let inner = crate::css::component_values::css_function_body(value, name)?;
    let (components, slash_alpha) = split_rgb_alpha(inner);
    let components = crate::css::component_values::split_css_component_values(components);
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

/// Convert D50 PCS coordinates to one CSS predefined RGB component space
/// without gamut mapping or component clipping.
///
/// CSS CssColor 4 requires conversion between predefined spaces to preserve
/// extended-range values until the actual output gamut mapping boundary. In
/// particular, routing D50 XYZ through a clipped sRGB output encoding would irreversibly
/// lose a Display-P3 green outside sRGB.
/// <https://www.w3.org/TR/css-color-4/#color-conversion>
pub(crate) fn color_to_predefined_rgb(color: CssColor, target: CssColorSpace) -> Option<CssColor> {
    let target_space = match target {
        CssColorSpace::Srgb => RgbColorSpace::Srgb,
        CssColorSpace::DisplayP3 => RgbColorSpace::DisplayP3,
        CssColorSpace::A98Rgb => RgbColorSpace::A98Rgb,
        CssColorSpace::ProphotoRgb => RgbColorSpace::ProphotoRgb,
        CssColorSpace::Rec2020 => RgbColorSpace::Rec2020,
        CssColorSpace::XyzD50 => return None,
    };
    if color.space() == target {
        return Some(color);
    }

    let xyz = color.to_xyz_d50();
    let d50 = [xyz.x as f64, xyz.y as f64, xyz.z as f64];
    let encoded = match target {
        CssColorSpace::Srgb => xyz_d65_to_srgb(adapt_d50_to_d65(d50)),
        CssColorSpace::DisplayP3 => linear_to_srgb(multiply_matrix(
            [
                [
                    2.493_496_911_941_425,
                    -0.931_383_617_919_124,
                    -0.402_710_784_450_717,
                ],
                [
                    -0.829_488_969_561_575,
                    1.762_664_060_318_346,
                    0.023_624_685_841_944,
                ],
                [
                    0.035_845_830_243_784,
                    -0.076_172_389_268_042,
                    0.956_884_524_007_687,
                ],
            ],
            adapt_d50_to_d65(d50),
        )),
        CssColorSpace::A98Rgb => multiply_matrix(
            [
                [
                    2.041_587_903_810_746_5,
                    -0.565_006_974_278_859_6,
                    -0.344_731_350_778_329_56,
                ],
                [
                    -0.969_243_636_280_879_6,
                    1.875_967_501_507_720_2,
                    0.041_555_057_407_175_59,
                ],
                [
                    0.013_444_280_632_031_142,
                    -0.118_362_392_231_018_38,
                    1.015_174_994_391_205_4,
                ],
            ],
            adapt_d50_to_d65(d50),
        )
        .map(linear_to_a98),
        CssColorSpace::ProphotoRgb => multiply_matrix(
            [
                [
                    1.345_798_973_102_828_1,
                    -0.255_580_100_079_975_34,
                    -0.051_106_285_067_534_01,
                ],
                [
                    -0.544_622_493_902_834_7,
                    1.508_232_741_313_278_1,
                    0.020_536_032_391_479_73,
                ],
                [0.0, 0.0, 1.211_967_545_638_945_4],
            ],
            d50,
        )
        .map(linear_to_prophoto),
        CssColorSpace::Rec2020 => multiply_matrix(
            [
                [
                    1.716_651_187_971_267_4,
                    -0.355_670_783_776_392_33,
                    -0.253_366_281_373_659_74,
                ],
                [
                    -0.666_684_351_832_489,
                    1.616_481_236_634_939_5,
                    0.015_768_545_813_911_13,
                ],
                [
                    0.017_639_857_445_310_783,
                    -0.042_770_613_257_808_524,
                    0.942_103_121_235_473_8,
                ],
            ],
            adapt_d50_to_d65(d50),
        )
        .map(linear_to_rec2020),
        CssColorSpace::XyzD50 => unreachable!(),
    };
    Some(CssColor::rgb(
        target_space,
        encoded[0] as f32,
        encoded[1] as f32,
        encoded[2] as f32,
        color.alpha(),
    ))
}

fn linear_to_a98(value: f64) -> f64 {
    value.signum() * value.abs().powf(256.0 / 563.0)
}

fn linear_to_prophoto(value: f64) -> f64 {
    let sign = value.signum();
    let magnitude = value.abs();
    sign * if magnitude >= 1.0 / 512.0 {
        magnitude.powf(1.0 / 1.8)
    } else {
        16.0 * magnitude
    }
}

fn linear_to_rec2020(value: f64) -> f64 {
    let sign = value.signum();
    let magnitude = value.abs();
    sign * if magnitude > 0.018_1 {
        1.099_3 * magnitude.powf(0.45) - 0.099_3
    } else {
        4.5 * magnitude
    }
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
        values,
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

/// Convert a retained CSS color to the D50 XYZ profile-connection space.
///
/// CSS CssColor 4 defines the predefined RGB spaces through D50 or D65 XYZ.
/// Keeping this conversion here makes the PCS boundary explicit where an ICC
/// engine does not provide a native XYZ identity-profile transform.
pub(crate) fn color_to_xyz_d50(color: CssColor) -> CssColor {
    let xyz = if let Some(xyz) = color.xyz_d50_coordinates() {
        [xyz.x as f64, xyz.y as f64, xyz.z as f64]
    } else {
        let (space, coordinates) = color
            .rgb_coordinates()
            .expect("a CSS color must contain RGB or D50 XYZ coordinates");
        let source = [
            coordinates.red as f64,
            coordinates.green as f64,
            coordinates.blue as f64,
        ];
        match space {
            // `srgb_to_xyz_d65` performs the encoded sRGB transfer itself. The
            // other RGB-space branches call helpers that expect linear input, but
            // applying the transfer here as well would decode sRGB twice.
            RgbColorSpace::Srgb => adapt_d65_to_d50(srgb_to_xyz_d65(source)),
            RgbColorSpace::DisplayP3 => adapt_d65_to_d50(linear_display_p3_to_xyz_linear(
                source.map(srgb_component_to_linear),
            )),
            RgbColorSpace::A98Rgb => {
                adapt_d65_to_d50(linear_a98_rgb_to_xyz(source.map(a98_to_linear)))
            }
            RgbColorSpace::ProphotoRgb => {
                linear_prophoto_rgb_to_xyz(source.map(prophoto_to_linear))
            }
            RgbColorSpace::Rec2020 => {
                adapt_d65_to_d50(linear_rec2020_to_xyz(source.map(rec2020_to_linear)))
            }
        }
    };
    CssColor::in_space(
        CssColorSpace::XyzD50,
        xyz[0] as f32,
        xyz[1] as f32,
        xyz[2] as f32,
        color.alpha(),
    )
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

fn color_to_lch(color: CssColor) -> Triplet {
    let color = color_to_xyz_d50(color);
    let [lightness, a, b] = xyz_d50_to_lab([
        color.components()[0] as f64,
        color.components()[1] as f64,
        color.components()[2] as f64,
    ]);
    let chroma = a.hypot(b);
    [lightness, chroma, b.atan2(a).to_degrees().rem_euclid(360.0)]
}

fn xyz_d50_to_lab(xyz: Triplet) -> Triplet {
    let lab = Lab::from_color_unclamped(Xyz::<D50, f64>::new(xyz[0], xyz[1], xyz[2]));
    [lab.l, lab.a, lab.b]
}

fn lab_to_xyz_d50(lightness: f64, a: f64, b: f64) -> Triplet {
    let xyz: Xyz<D50, f64> = Xyz::from_color_unclamped(Lab::new(lightness, a, b));
    [xyz.x, xyz.y, xyz.z]
}

/// Convert CSS OKLab coordinates to D65 XYZ without gamut mapping.
///
/// CSS CssColor 4 defines OKLab relative to D65 XYZ, exactly the semantic model
/// represented by Palette's `Oklab` and `Xyz<D65>` types. Parsing and hue
/// grammar remain Quire-owned CSS behavior; Palette owns this standard math.
/// <https://www.w3.org/TR/css-color-4/#ok-lab>
fn oklab_to_xyz_d65(lightness: f64, a: f64, b: f64) -> Triplet {
    let xyz: Xyz<D65, f64> = Xyz::from_color_unclamped(Oklab::new(lightness, a, b));
    [xyz.x, xyz.y, xyz.z]
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
    } else if let Some(gradians) = value.strip_suffix("grad") {
        gradians
            .trim()
            .parse::<f32>()
            .ok()
            .map(|gradians| gradians * 0.9)
    } else if let Some(radians) = value.strip_suffix("rad") {
        radians
            .trim()
            .parse::<f32>()
            .ok()
            .map(|radians| radians.to_degrees())
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
    hsl_to_rgb_units_unclamped(
        hue_degrees,
        saturation.clamp(0.0, 1.0),
        lightness.clamp(0.0, 1.0),
    )
}

/// Relative HSL uses the CSS Color 5 extended-range reconstruction: its
/// component expressions are not clamped before the final output conversion.
fn hsl_to_rgb_units_unclamped(
    hue_degrees: f32,
    saturation: f32,
    lightness: f32,
) -> (f32, f32, f32) {
    let hue = hue_degrees.rem_euclid(360.0) / 360.0;
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

fn srgb_to_hsl(color: CssColor) -> (f32, f32, f32) {
    let color = color.to_rgb_space(RgbColorSpace::Srgb);
    let maximum = color.components()[0]
        .max(color.components()[1])
        .max(color.components()[2]);
    let minimum = color.components()[0]
        .min(color.components()[1])
        .min(color.components()[2]);
    let lightness = (maximum + minimum) / 2.0;
    let delta = maximum - minimum;
    if delta == 0.0 {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if maximum == color.components()[0] {
        60.0 * ((color.components()[1] - color.components()[2]) / delta).rem_euclid(6.0)
    } else if maximum == color.components()[1] {
        60.0 * ((color.components()[2] - color.components()[0]) / delta + 2.0)
    } else {
        60.0 * ((color.components()[0] - color.components()[1]) / delta + 4.0)
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
    if value.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
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
    fn modern_hsl_and_hwb_components_accept_numbers_and_none() {
        assert_eq!(
            parse_hsl_function("hsl(60deg 50 50)"),
            parse_hsl_function("hsl(60deg 50% 50%)")
        );
        assert_eq!(
            parse_hwb_function("hwb(60deg 20 30)"),
            parse_hwb_function("hwb(60deg 20% 30%)")
        );
        assert_eq!(
            parse_hsl_function("hsl(60deg none 50%)"),
            parse_hsl_function("hsl(60deg 0% 50%)")
        );
        assert_eq!(
            parse_hwb_function("hwb(60deg none none)"),
            parse_hwb_function("hwb(60deg 0% 0%)")
        );
        assert!(parse_hsl_function("hsl(60deg, 50, 50)").is_none());
    }

    #[test]
    fn predefined_rgb_spaces_retain_coordinates_until_output() {
        let cases = [
            ("color(srgb 1.2 -0.1 0.3)", CssColorSpace::Srgb),
            ("color(display-p3 1.2 -0.1 0.3)", CssColorSpace::DisplayP3),
            ("color(a98-rgb 1.1 0.2 0.3)", CssColorSpace::A98Rgb),
            (
                "color(prophoto-rgb 1.3 0.2 0.3)",
                CssColorSpace::ProphotoRgb,
            ),
            ("color(rec2020 1.1 0.2 0.3)", CssColorSpace::Rec2020),
        ];
        for (input, expected_space) in cases {
            let color = parse_color(input).unwrap();
            assert_eq!(color.space(), expected_space, "{input}");
            assert!(
                color.components()[0] > 1.0 || color.components()[1] < 0.0,
                "{input}"
            );
        }
    }

    #[test]
    fn xyz_d65_is_adapted_to_retained_d50_pcs() {
        // CSS CssColor 4's D65 reference white adapted to D50.
        let color = parse_color("color(xyz-d65 .950455927 1 1.089057751)").unwrap();
        assert_eq!(color.space(), CssColorSpace::XyzD50);
        assert!((color.components()[0] - 0.96422).abs() < 0.0001);
        assert!((color.components()[1] - 1.0).abs() < 0.0001);
        assert!((color.components()[2] - 0.82510).abs() < 0.0001);
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
            assert_eq!(color.space(), CssColorSpace::XyzD50, "{input}");
            assert!(color.alpha() == 1.0);
        }
    }

    #[test]
    fn polar_lightness_endpoints_are_neutral_after_clamping() {
        assert_eq!(parse_color("lch(100% 110 60)"), Some(CssColor::WHITE));
        assert_eq!(parse_color("lch(-10% 110 60)"), Some(CssColor::BLACK));
        assert_eq!(parse_color("oklch(100% 1.1 60)"), Some(CssColor::WHITE));
        assert_eq!(parse_color("oklch(-0.1 1.1 60)"), Some(CssColor::BLACK));
    }

    #[test]
    fn palette_oklab_conversion_matches_the_css_green_reference() {
        let color = parse_color("oklab(51.975% -0.1403 0.10768)").unwrap();
        let srgb = color_to_predefined_rgb(color, CssColorSpace::Srgb).unwrap();
        assert!(srgb.components()[0].abs() < 0.003, "{srgb:?}");
        assert!(
            (srgb.components()[1] - 0.501_960_8).abs() < 0.003,
            "{srgb:?}"
        );
        assert!(srgb.components()[2].abs() < 0.003, "{srgb:?}");
    }

    #[test]
    fn lab_reference_green_converts_to_equivalent_display_p3() {
        let color = parse_color("lab(46.2775% -47.5621 48.5837)").unwrap();
        let srgb = color_to_predefined_rgb(color, CssColorSpace::Srgb).unwrap();
        let display_p3 = color_to_predefined_rgb(color, CssColorSpace::DisplayP3).unwrap();
        assert!(srgb.components()[0].abs() < 0.003, "{srgb:?}");
        assert!(
            (srgb.components()[1] - 0.501_960_8).abs() < 0.003,
            "{srgb:?}"
        );
        assert!(srgb.components()[2].abs() < 0.003, "{srgb:?}");
        let round_trip = color_to_predefined_rgb(display_p3, CssColorSpace::Srgb).unwrap();
        assert!(
            round_trip.components()[0].abs() < 0.003,
            "{display_p3:?} -> {round_trip:?}"
        );
        assert!(
            (round_trip.components()[1] - 0.501_960_8).abs() < 0.003,
            "{display_p3:?} -> {round_trip:?}"
        );
        assert!(
            round_trip.components()[2].abs() < 0.003,
            "{display_p3:?} -> {round_trip:?}"
        );
    }

    #[test]
    fn relative_lch_resolves_origin_components_before_math() {
        let relative = parse_color("lch(from blue calc(0.5 * l) c h)").unwrap();
        let reference = parse_color("lch(14.7841 131.201 301.364)").unwrap();
        assert_eq!(relative.space(), CssColorSpace::XyzD50);
        assert!((relative.components()[0] - reference.components()[0]).abs() < 0.0001);
        assert!((relative.components()[1] - reference.components()[1]).abs() < 0.0001);
        assert!((relative.components()[2] - reference.components()[2]).abs() < 0.0001);
    }

    #[test]
    fn relative_colors_keep_unbounded_target_space_components() {
        let display_p3_green = CssColor::rgb(RgbColorSpace::DisplayP3, 0.0, 1.0, 0.0, 1.0);
        let relative =
            parse_color_from_currentcolor("rgb(from currentcolor r g b)", display_p3_green)
                .expect("relative RGB should resolve");
        assert_eq!(relative.space(), CssColorSpace::Srgb);
        assert!(
            relative.components()[0] < 0.0
                || relative.components()[1] > 1.0
                || relative.components()[2] < 0.0,
            "relative RGB must not clip its target-space components: {relative:?}"
        );

        let relative_hsl =
            parse_color_from_currentcolor("hsl(from currentcolor h s l)", display_p3_green)
                .expect("relative HSL should resolve");
        assert!(
            relative_hsl
                .components()
                .iter()
                .any(|component| !(0.0..=1.0).contains(component)),
            "relative HSL must use the unclamped reconstruction path: {relative_hsl:?}"
        );
    }

    #[test]
    fn lch_color_mix_retains_pcs_until_output_encoding() {
        let mixed = parse_color("color-mix(in lch longer hue, color(display-p3 0 1 0), blue)")
            .expect("LCH color mix should parse");
        assert_eq!(mixed.space(), CssColorSpace::XyzD50);
        assert!(mixed.xyz_d50_coordinates().is_some());
    }

    #[test]
    fn color_mix_hsl_longer_hue_matches_gradient_midpoint() {
        let mix = parse_color("color-mix(in hsl longer hue, red, blue)").unwrap();
        assert!(
            mix.components()[0] < 0.001,
            "red component: {}",
            mix.components()[0]
        );
        assert!(
            mix.components()[1] > 0.999,
            "green component: {}",
            mix.components()[1]
        );
        assert!(
            mix.components()[2] < 0.001,
            "blue component: {}",
            mix.components()[2]
        );
    }

    #[test]
    fn color_mix_accepts_every_gradient_interpolation_space() {
        for method in [
            "srgb",
            "srgb-linear",
            "display-p3",
            "display-p3-linear",
            "a98-rgb",
            "prophoto-rgb",
            "rec2020",
            "xyz-d50",
            "xyz-d65",
            "lab",
            "oklab",
            "hsl longer hue",
            "hwb increasing hue",
            "lch decreasing hue",
            "oklch shorter hue",
        ] {
            let value = format!("color-mix(in {method}, red, blue)");
            assert!(parse_color(&value).is_some(), "{value}");
        }
        for value in [
            "color-mix(in srgb longer hue, red, blue)",
            "color-mix(in hsl hue, red, blue)",
            "color-mix(in hsl sideways hue, red, blue)",
        ] {
            assert!(parse_color(value).is_none(), "{value}");
        }
    }

    #[test]
    fn color_mix_normalizes_ordered_lists_and_alpha() {
        let ordered = parse_color("color-mix(in hsl longer hue, red, blue, red)").unwrap();
        let reversed = parse_color("color-mix(in hsl longer hue, blue, red, blue)").unwrap();
        assert_ne!(ordered, reversed);

        let underspecified = parse_color("color-mix(in srgb, red 20%, blue 60%)").unwrap();
        assert!((underspecified.alpha() - 0.8).abs() < 0.0001);
        let lch = parse_color("color-mix(in lch, purple, plum)").unwrap();
        assert_eq!(lch.space(), CssColorSpace::XyzD50);
        let lch = color_to_predefined_rgb(lch, CssColorSpace::Srgb).unwrap();
        let expected_lch = CssColor::srgb(0.684_898, 0.360_15, 0.683_102, 1.0);
        assert!(
            (lch.components()[0] - expected_lch.components()[0]).abs() < 0.001,
            "actual={lch:?} expected={expected_lch:?}"
        );
        assert!(
            (lch.components()[1] - expected_lch.components()[1]).abs() < 0.001,
            "{lch:?}"
        );
        assert!(
            (lch.components()[2] - expected_lch.components()[2]).abs() < 0.001,
            "{lch:?}"
        );
        assert_eq!(
            parse_color("color-mix(in srgb, red 0%, blue 0%)"),
            Some(CssColor::TRANSPARENT)
        );
        for value in [
            "color-mix(in srgb, red -1%, blue)",
            "color-mix(in srgb, red 101%, blue)",
            "color-mix(in srgb, red 90%, blue 90%, green)",
        ] {
            assert!(parse_color(value).is_none(), "{value}");
        }
    }

    #[test]
    fn color_mix_resolves_currentcolor_and_analogous_missing_components() {
        let current = CssColor::new(0, 255, 0);
        let mixed =
            parse_color_mix("color-mix(in hsl longer hue, currentcolor, blue)", current).unwrap();
        assert!(
            mixed.components()[0] > 0.9
                && mixed.components()[1] < 0.1
                && mixed.components()[2] < 0.1,
            "{mixed:?}"
        );

        let missing = parse_color("color-mix(in srgb, rgb(none 255 none), yellow)").unwrap();
        assert!((missing.components()[0] - 1.0).abs() < 0.0001);
        assert!((missing.components()[1] - 1.0).abs() < 0.0001);
        assert!(missing.components()[2] < 0.0001);
    }

    #[test]
    fn color_functions_use_decoded_token_names_and_component_boundaries() {
        assert_eq!(
            parse_color("\\72 gb(255 /* red */ 0 0 / 50%)"),
            Some(CssColor::rgba(255, 0, 0, 0.5))
        );
        assert_eq!(parse_color("\\74 ransparent"), Some(CssColor::TRANSPARENT));
        assert!(parse_color("rgb(1 2 3 / calc(1 / 2))").is_none());
        assert_eq!(
            parse_color_from_currentcolor(
                "light-dark(rgb(1, 2, 3), rgb(4, 5, 6))",
                CssColor::BLACK,
            ),
            Some(CssColor::rgba(1, 2, 3, 1.0))
        );
    }

    #[test]
    fn currentcolor_provenance_walks_nested_color_component_values() {
        for value in [
            "currentcolor",
            "color-mix(in srgb, currentcolor 50%, red)",
            "contrast-color(currentcolor vs white, black)",
            "rgb(from currentcolor r g b)",
        ] {
            assert!(color_depends_on_currentcolor(value), "{value}");
        }
        for value in [
            "red",
            "rgb(1 2 3)",
            "\"currentcolor\"",
            "/* currentcolor */ blue",
        ] {
            assert!(!color_depends_on_currentcolor(value), "{value}");
        }
    }

    #[test]
    fn hue_angle_units_do_not_confuse_grad_with_rad() {
        // `grad` has `rad` as a suffix, so it must be tested before radians.
        // CSS Values defines 400grad as a complete turn:
        // <https://www.w3.org/TR/css-values-4/#angles>
        let grad = parse_hue_degrees("133.33333333grad").unwrap();
        let rad = parse_hue_degrees("2.0943951024rad").unwrap();
        assert!((grad - 120.0).abs() < 0.0001, "{grad}");
        assert!((rad - 120.0).abs() < 0.0001, "{rad}");
    }
}
