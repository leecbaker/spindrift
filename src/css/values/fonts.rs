use super::*;

pub(crate) fn parse_list_style_type(value: &str) -> Option<ListStyleType> {
    let value = value.trim();
    if let Some(style) = parse_symbols_function(value) {
        return Some(ListStyleType::Anonymous(Box::new(style)));
    }
    if let Some((text, tail)) = parse_css_string_token(value)
        && tail.trim().is_empty()
    {
        return Some(ListStyleType::String(text));
    }
    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "disc" => Some(ListStyleType::Disc),
        "circle" => Some(ListStyleType::Circle),
        "square" => Some(ListStyleType::Square),
        "disclosure-open" => Some(ListStyleType::DisclosureOpen),
        "disclosure-closed" => Some(ListStyleType::DisclosureClosed),
        "decimal" => Some(ListStyleType::Decimal),
        "decimal-leading-zero" => Some(ListStyleType::DecimalLeadingZero),
        "arabic-indic" => Some(ListStyleType::Numeric(NumericCounterStyle::ArabicIndic)),
        "armenian" | "upper-armenian" => {
            Some(ListStyleType::Additive(AdditiveCounterStyle::Armenian))
        }
        "lower-armenian" => Some(ListStyleType::Additive(AdditiveCounterStyle::LowerArmenian)),
        "bengali" => Some(ListStyleType::Numeric(NumericCounterStyle::Bengali)),
        "cambodian" | "khmer" => Some(ListStyleType::Numeric(NumericCounterStyle::Cambodian)),
        "cjk-decimal" => Some(ListStyleType::Numeric(NumericCounterStyle::CjkDecimal)),
        "devanagari" => Some(ListStyleType::Numeric(NumericCounterStyle::Devanagari)),
        "georgian" => Some(ListStyleType::Additive(AdditiveCounterStyle::Georgian)),
        "gujarati" => Some(ListStyleType::Numeric(NumericCounterStyle::Gujarati)),
        "gurmukhi" => Some(ListStyleType::Numeric(NumericCounterStyle::Gurmukhi)),
        "hebrew" => Some(ListStyleType::Additive(AdditiveCounterStyle::Hebrew)),
        "kannada" => Some(ListStyleType::Numeric(NumericCounterStyle::Kannada)),
        "lao" => Some(ListStyleType::Numeric(NumericCounterStyle::Lao)),
        "malayalam" => Some(ListStyleType::Numeric(NumericCounterStyle::Malayalam)),
        "mongolian" => Some(ListStyleType::Numeric(NumericCounterStyle::Mongolian)),
        "myanmar" => Some(ListStyleType::Numeric(NumericCounterStyle::Myanmar)),
        "oriya" => Some(ListStyleType::Numeric(NumericCounterStyle::Oriya)),
        "persian" => Some(ListStyleType::Numeric(NumericCounterStyle::Persian)),
        "tamil" => Some(ListStyleType::Numeric(NumericCounterStyle::Tamil)),
        "telugu" => Some(ListStyleType::Numeric(NumericCounterStyle::Telugu)),
        "thai" => Some(ListStyleType::Numeric(NumericCounterStyle::Thai)),
        "tibetan" => Some(ListStyleType::Numeric(NumericCounterStyle::Tibetan)),
        "lower-alpha" | "lower-latin" => Some(ListStyleType::LowerAlpha),
        "upper-alpha" | "upper-latin" => Some(ListStyleType::UpperAlpha),
        "lower-greek" => Some(ListStyleType::LowerGreek),
        "hiragana" => Some(ListStyleType::Hiragana),
        "hiragana-iroha" => Some(ListStyleType::HiraganaIroha),
        "katakana" => Some(ListStyleType::Katakana),
        "katakana-iroha" => Some(ListStyleType::KatakanaIroha),
        "cjk-earthly-branch" => Some(ListStyleType::CjkEarthlyBranch),
        "cjk-heavenly-stem" => Some(ListStyleType::CjkHeavenlyStem),
        "lower-roman" => Some(ListStyleType::LowerRoman),
        "upper-roman" => Some(ListStyleType::UpperRoman),
        "none" => Some(ListStyleType::None),
        "inside" | "outside" => None,
        _ if is_counter_style_ident(value) => Some(ListStyleType::Named(lower)),
        _ => None,
    }
}

pub(crate) fn parse_list_style_position(value: &str) -> Option<ListStylePosition> {
    value
        .split_whitespace()
        .find_map(|part| match part.to_ascii_lowercase().as_str() {
            "outside" => Some(ListStylePosition::Outside),
            "inside" => Some(ListStylePosition::Inside),
            _ => None,
        })
}

pub(crate) fn parse_marker_side(value: &str) -> Option<MarkerSide> {
    match value.trim().to_ascii_lowercase().as_str() {
        "match-self" => Some(MarkerSide::MatchSelf),
        "match-parent" => Some(MarkerSide::MatchParent),
        _ => None,
    }
}

pub(crate) fn parse_symbols_function(value: &str) -> Option<CounterStyleRule> {
    let body = strip_ascii_function(value, "symbols")?;
    let (argument, tail) = split_function_argument(body)?;
    if !tail.trim().is_empty() {
        return None;
    }
    let mut system = CounterStyleSystem::Symbolic;
    let mut rest = argument.trim();
    if let Some((token, tail)) = split_symbols_token(rest)
        && let Some(parsed_system) = parse_symbols_system_keyword(token)
    {
        system = parsed_system;
        rest = tail.trim_start();
    }
    let symbols = parse_symbols_function_symbols(rest);
    let valid = match system {
        CounterStyleSystem::Cyclic
        | CounterStyleSystem::Symbolic
        | CounterStyleSystem::Fixed(_) => !symbols.is_empty(),
        CounterStyleSystem::Numeric | CounterStyleSystem::Alphabetic => symbols.len() >= 2,
        CounterStyleSystem::Additive | CounterStyleSystem::Extends(_) => false,
    };
    valid.then_some(CounterStyleRule {
        name: String::new(),
        system,
        symbols,
        additive_symbols: Vec::new(),
        prefix: None,
        suffix: None,
        negative: None,
        pad: None,
        range: None,
        fallback: None,
        speak_as: None,
    })
}

pub(crate) fn parse_symbols_system_keyword(value: &str) -> Option<CounterStyleSystem> {
    match value.to_ascii_lowercase().as_str() {
        "cyclic" => Some(CounterStyleSystem::Cyclic),
        "numeric" => Some(CounterStyleSystem::Numeric),
        "alphabetic" => Some(CounterStyleSystem::Alphabetic),
        "symbolic" => Some(CounterStyleSystem::Symbolic),
        "fixed" => Some(CounterStyleSystem::Fixed(1)),
        _ => None,
    }
}

pub(crate) fn parse_symbols_function_symbols(mut value: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    value = value.trim();
    while !value.is_empty() {
        if let Some((string, tail)) = parse_css_string_token(value) {
            symbols.push(string);
            value = tail.trim_start();
        } else if let Some((token, tail)) = split_symbols_token(value) {
            symbols.push(unescape_symbols_token(token));
            value = tail.trim_start();
        } else {
            break;
        }
    }
    symbols
}

pub(crate) fn split_symbols_token(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start();
    let end = value
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(value.len());
    (end > 0).then_some((&value[..end], &value[end..]))
}

pub(crate) fn unescape_symbols_token(value: &str) -> String {
    if let Some(hex) = value.strip_prefix('\\')
        && let Ok(codepoint) = u32::from_str_radix(hex, 16)
        && let Some(character) = char::from_u32(codepoint)
    {
        return character.to_string();
    }
    value.to_string()
}

pub(crate) fn is_counter_style_ident(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '-' || first.is_ascii_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '-' || character.is_ascii_alphanumeric()
        })
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "inherit" | "initial" | "unset" | "revert" | "default" | "url"
        )
}

pub(crate) fn parse_font_family_names(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|part| part.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

pub(crate) fn parse_font_family(value: &str) -> Option<FontFamily> {
    let families = parse_font_family_names(value);
    if families.is_empty() {
        return None;
    }
    if families.len() == 1
        && let Some(generic) = generic_font_family(&families[0])
    {
        return Some(generic);
    }
    Some(FontFamily::Names(families))
}

/// Parsed CSS `font` shorthand components currently modeled by `ComputedStyle`.
///
/// CSS Fonts defines `font` as a reset shorthand around font style, weight,
/// stretch, size, optional line-height, and family. Values not represented by
/// `ComputedStyle`, such as `font-variant`, are accepted only when they are
/// the CSS-wide `normal` reset:
/// <https://www.w3.org/TR/css-fonts-4/#font-prop>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedFontShorthand {
    pub(crate) style: FontStyle,
    pub(crate) weight: FontWeight,
    pub(crate) width: FontWidth,
    pub(crate) variant_caps: FontVariantCaps,
    pub(crate) size: f32,
    pub(crate) line_height: Option<ComputedLineHeight>,
    pub(crate) family: FontFamily,
}

/// Parsed CSS `font-variant` shorthand components.
///
/// CSS Fonts defines `font-variant` as a shorthand over the OpenType feature
/// longhands, resetting omitted subproperties to their initial values:
/// <https://www.w3.org/TR/css-fonts-4/#font-variant-prop>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedFontVariant {
    pub(crate) ligatures: FontVariantLigatures,
    pub(crate) position: FontVariantPosition,
    pub(crate) caps: FontVariantCaps,
    pub(crate) numeric: FontVariantNumeric,
    pub(crate) alternates: FontVariantAlternates,
    pub(crate) east_asian: FontVariantEastAsian,
    pub(crate) emoji: FontVariantEmoji,
}

impl ParsedFontVariant {
    pub(crate) fn normal() -> Self {
        Self {
            ligatures: FontVariantLigatures::Normal,
            position: FontVariantPosition::Normal,
            caps: FontVariantCaps::Normal,
            numeric: FontVariantNumeric::Normal,
            alternates: FontVariantAlternates::Normal,
            east_asian: FontVariantEastAsian::Normal,
            emoji: FontVariantEmoji::Normal,
        }
    }
}

/// Parses the CSS `font` shorthand into computed font longhand values.
///
/// The parser handles the common author grammar used by WeasyPrint/WPT:
/// optional `font-style`, `font-weight`, and `font-stretch` tokens before the
/// required `font-size[/line-height]` and `font-family` list. CSS system font
/// keywords remain unsupported until platform font metrics are modeled:
/// <https://www.w3.org/TR/css-fonts-4/#font-prop>.
pub(crate) fn parse_font_shorthand(
    value: &str,
    inherited_font_size: f32,
    inherited_font_weight: FontWeight,
) -> Option<ParsedFontShorthand> {
    parse_font_shorthand_with_parent_ch_advance(
        value,
        inherited_font_size,
        inherited_font_size * 0.5,
        inherited_font_weight,
    )
}

pub(crate) fn parse_font_shorthand_with_parent_ch_advance(
    value: &str,
    inherited_font_size: f32,
    inherited_ch_advance: f32,
    inherited_font_weight: FontWeight,
) -> Option<ParsedFontShorthand> {
    parse_font_shorthand_with_line_height_font_size(
        value,
        inherited_font_size,
        inherited_ch_advance,
        inherited_font_weight,
        None,
    )
}

/// Parses the CSS `font` shorthand with an explicit line-height font-size basis.
///
/// CSS Fonts expands `font` into separate longhands, and CSS Values resolves
/// `em` units in `line-height` against the element's computed `font-size`.
/// During cascade, that final font size can come from a stronger `font-size`
/// declaration than the `font-size` component inside this shorthand:
/// <https://www.w3.org/TR/css-fonts-4/#font-prop>,
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-line-height>, and
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
pub(crate) fn parse_font_shorthand_with_line_height_font_size(
    value: &str,
    inherited_font_size: f32,
    inherited_ch_advance: f32,
    inherited_font_weight: FontWeight,
    line_height_font_size: Option<f32>,
) -> Option<ParsedFontShorthand> {
    let tokens = split_css_component_values(value);
    let size_index = tokens.iter().position(|token| {
        split_font_size_and_line_height(token, inherited_font_size, inherited_ch_advance).is_some()
    })?;
    let mut style = FontStyle::Normal;
    let mut weight = FontWeight::NORMAL;
    let mut width = FontWidth::NORMAL;
    let mut variant_caps = FontVariantCaps::Normal;
    for token in &tokens[..size_index] {
        if token.eq_ignore_ascii_case("normal") {
            continue;
        }
        if let Some(parsed) = parse_font_style(token) {
            style = parsed;
        } else if let Some(parsed) = parse_font_weight(token, inherited_font_weight) {
            weight = parsed;
        } else if let Some(parsed) = parse_font_width(token) {
            width = parsed;
        } else if token.eq_ignore_ascii_case("small-caps")
            && variant_caps == FontVariantCaps::Normal
        {
            variant_caps = FontVariantCaps::SmallCaps;
        } else {
            return None;
        }
    }

    let (size, mut line_height) = split_font_size_and_line_height_with_line_height_font_size(
        tokens[size_index],
        inherited_font_size,
        inherited_ch_advance,
        line_height_font_size,
    )?;
    let mut family_start = size_index + 1;
    if line_height.is_none() && tokens.get(family_start).is_some_and(|token| *token == "/") {
        let line_height_font_size = line_height_font_size.unwrap_or(size);
        line_height = tokens
            .get(family_start + 1)
            .and_then(|token| parse_computed_line_height(token, line_height_font_size));
        line_height?;
        family_start += 2;
    }
    let family = tokens.get(family_start..)?.join(" ");
    let family = parse_font_family(&family)?;

    Some(ParsedFontShorthand {
        style,
        weight,
        width,
        variant_caps,
        size,
        line_height,
        family,
    })
}

fn split_font_size_and_line_height(
    token: &str,
    inherited_font_size: f32,
    inherited_ch_advance: f32,
) -> Option<(f32, Option<ComputedLineHeight>)> {
    split_font_size_and_line_height_with_line_height_font_size(
        token,
        inherited_font_size,
        inherited_ch_advance,
        None,
    )
}

fn split_font_size_and_line_height_with_line_height_font_size(
    token: &str,
    inherited_font_size: f32,
    inherited_ch_advance: f32,
    line_height_font_size: Option<f32>,
) -> Option<(f32, Option<ComputedLineHeight>)> {
    let Some((size, line_height)) = split_font_token_on_slash(token) else {
        if is_unitless_nonzero_number(token) {
            return None;
        }
        return parse_font_size_with_parent_ch_advance(
            token,
            inherited_font_size,
            inherited_ch_advance,
        )
        .map(|size| (size, None));
    };
    if is_unitless_nonzero_number(size) {
        return None;
    }
    let size =
        parse_font_size_with_parent_ch_advance(size, inherited_font_size, inherited_ch_advance)?;
    let line_height =
        parse_computed_line_height(line_height, line_height_font_size.unwrap_or(size))?;
    Some((size, Some(line_height)))
}

fn is_unitless_nonzero_number(token: &str) -> bool {
    token.trim().parse::<f32>().is_ok_and(|value| value != 0.0)
}

fn split_font_token_on_slash(token: &str) -> Option<(&str, &str)> {
    let slash = token.find('/')?;
    let (size, line_height) = token.split_at(slash);
    let line_height = &line_height[1..];
    (!size.trim().is_empty() && !line_height.trim().is_empty())
        .then_some((size.trim(), line_height.trim()))
}

pub(crate) fn known_font_family(value: &str) -> Option<FontFamily> {
    generic_font_family(value)
}

pub(crate) fn generic_font_family(value: &str) -> Option<FontFamily> {
    match value.trim().to_ascii_lowercase().as_str() {
        "serif" => Some(FontFamily::Serif),
        "monospace" => Some(FontFamily::Monospace),
        "sans-serif" | "sans serif" => Some(FontFamily::SansSerif),
        _ => None,
    }
}

pub(crate) fn parse_font_size_adjust(value: &str) -> Option<FontSizeAdjust> {
    let tokens = split_css_component_values(value);
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("none") {
        return Some(FontSizeAdjust::None);
    }
    let mut metric = None;
    let mut adjust_value = None;
    for token in tokens {
        if let Some(parsed) = parse_font_size_adjust_metric(token) {
            if metric.replace(parsed).is_some() {
                return None;
            }
        } else if token.eq_ignore_ascii_case("from-font") {
            if adjust_value
                .replace(FontSizeAdjustValue::FromFont)
                .is_some()
            {
                return None;
            }
        } else {
            let value = token.parse::<f32>().ok()?;
            if !value.is_finite() || value < 0.0 {
                return None;
            }
            if adjust_value
                .replace(FontSizeAdjustValue::Number(value))
                .is_some()
            {
                return None;
            }
        }
    }
    Some(FontSizeAdjust::Value {
        metric: metric.unwrap_or(FontSizeAdjustMetric::ExHeight),
        value: adjust_value?,
    })
}

fn parse_font_size_adjust_metric(value: &str) -> Option<FontSizeAdjustMetric> {
    match value.to_ascii_lowercase().as_str() {
        "ex-height" => Some(FontSizeAdjustMetric::ExHeight),
        "cap-height" => Some(FontSizeAdjustMetric::CapHeight),
        "ch-width" => Some(FontSizeAdjustMetric::ChWidth),
        "ic-width" => Some(FontSizeAdjustMetric::IcWidth),
        "ic-height" => Some(FontSizeAdjustMetric::IcHeight),
        _ => None,
    }
}

pub(crate) fn parse_font_weight(value: &str, inherited: FontWeight) -> Option<FontWeight> {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(FontWeight::NORMAL),
        "bold" => Some(FontWeight::BOLD),
        "bolder" => Some(inherited.bolder()),
        "lighter" => Some(inherited.lighter()),
        value => value.parse::<f32>().ok().and_then(FontWeight::from_number),
    }
}

pub(crate) fn parse_font_style(value: &str) -> Option<FontStyle> {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(FontStyle::Normal),
        "italic" => Some(FontStyle::Italic),
        value if value == "oblique" || value.starts_with("oblique ") => Some(FontStyle::Oblique),
        _ => None,
    }
}

pub(crate) fn parse_font_width(value: &str) -> Option<FontWidth> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ultra-condensed" => Some(FontWidth::ULTRA_CONDENSED),
        "extra-condensed" => Some(FontWidth::EXTRA_CONDENSED),
        "condensed" => Some(FontWidth::CONDENSED),
        "semi-condensed" => Some(FontWidth::SEMI_CONDENSED),
        "normal" => Some(FontWidth::NORMAL),
        "semi-expanded" => Some(FontWidth::SEMI_EXPANDED),
        "expanded" => Some(FontWidth::EXPANDED),
        "extra-expanded" => Some(FontWidth::EXTRA_EXPANDED),
        "ultra-expanded" => Some(FontWidth::ULTRA_EXPANDED),
        value => {
            parse_percentage(value).and_then(|percent| FontWidth::from_percent(percent * 100.0))
        }
    }
}

pub(crate) fn parse_font_kerning(value: &str) -> Option<FontKerning> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(FontKerning::Auto),
        "normal" => Some(FontKerning::Normal),
        "none" => Some(FontKerning::None),
        _ => None,
    }
}

/// Parse `font-feature-settings` into the computed OpenType feature map.
///
/// CSS Fonts requires four-character printable ASCII tags, optional
/// non-negative integer/on/off values, and duplicate tags to be resolved by
/// the last specified value:
/// <https://www.w3.org/TR/css-fonts-4/#font-feature-settings-prop>.
pub(crate) fn parse_font_feature_settings(value: &str) -> Option<FontFeatureSettings> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(FontFeatureSettings::NORMAL);
    }
    let mut settings = Vec::<FontFeatureSetting>::new();
    for item in split_top_level_commas(value) {
        let item = item.trim();
        if item.is_empty() {
            return None;
        }
        let (tag, tail) = parse_css_string_token(item)?;
        let tag = parse_opentype_tag(&tag)?;
        let tail = tail.trim();
        let value = if tail.is_empty() {
            1
        } else {
            match tail.to_ascii_lowercase().as_str() {
                "on" => 1,
                "off" => 0,
                _ => tail.parse::<u16>().ok()?,
            }
        };
        if let Some(existing) = settings.iter_mut().find(|setting| setting.tag == tag) {
            existing.value = value;
        } else {
            settings.push(FontFeatureSetting::new(tag, value));
        }
    }
    if settings.is_empty() {
        return None;
    }
    settings.sort_by_key(|setting| setting.tag);
    Some(FontFeatureSettings(settings))
}

fn parse_opentype_tag(value: &str) -> Option<[u8; 4]> {
    let bytes = value.as_bytes();
    (bytes.len() == 4 && bytes.iter().all(|byte| (0x20..=0x7e).contains(byte)))
        .then(|| [bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub(crate) fn parse_font_variant_ligatures(value: &str) -> Option<FontVariantLigatures> {
    let tokens = split_css_component_values(value);
    if tokens.len() == 1 {
        match tokens[0].to_ascii_lowercase().as_str() {
            "normal" => return Some(FontVariantLigatures::Normal),
            "none" => return Some(FontVariantLigatures::None),
            _ => {}
        }
    }
    let mut common = None;
    let mut discretionary = None;
    let mut historical = None;
    let mut contextual = None;
    for token in tokens {
        match token.to_ascii_lowercase().as_str() {
            "common-ligatures" => set_exclusive_flag(&mut common, true)?,
            "no-common-ligatures" => set_exclusive_flag(&mut common, false)?,
            "discretionary-ligatures" => set_exclusive_flag(&mut discretionary, true)?,
            "no-discretionary-ligatures" => set_exclusive_flag(&mut discretionary, false)?,
            "historical-ligatures" => set_exclusive_flag(&mut historical, true)?,
            "no-historical-ligatures" => set_exclusive_flag(&mut historical, false)?,
            "contextual" => set_exclusive_flag(&mut contextual, true)?,
            "no-contextual" => set_exclusive_flag(&mut contextual, false)?,
            _ => return None,
        }
    }
    (common.is_some() || discretionary.is_some() || historical.is_some() || contextual.is_some())
        .then_some(FontVariantLigatures::Values {
            common,
            discretionary,
            historical,
            contextual,
        })
}

fn set_exclusive_flag(target: &mut Option<bool>, value: bool) -> Option<()> {
    if target.is_some() {
        return None;
    }
    *target = Some(value);
    Some(())
}

pub(crate) fn parse_font_variant_position(value: &str) -> Option<FontVariantPosition> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "normal" => Some(FontVariantPosition::Normal),
        "sub" => Some(FontVariantPosition::Sub),
        "super" => Some(FontVariantPosition::Super),
        _ => None,
    }
}

pub(crate) fn parse_font_variant_caps(value: &str) -> Option<FontVariantCaps> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "normal" => Some(FontVariantCaps::Normal),
        "small-caps" => Some(FontVariantCaps::SmallCaps),
        "all-small-caps" => Some(FontVariantCaps::AllSmallCaps),
        "petite-caps" => Some(FontVariantCaps::PetiteCaps),
        "all-petite-caps" => Some(FontVariantCaps::AllPetiteCaps),
        "unicase" => Some(FontVariantCaps::Unicase),
        "titling-caps" => Some(FontVariantCaps::TitlingCaps),
        _ => None,
    }
}

pub(crate) fn parse_font_variant_numeric(value: &str) -> Option<FontVariantNumeric> {
    let tokens = split_css_component_values(value);
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("normal") {
        return Some(FontVariantNumeric::Normal);
    }
    let mut figure = None;
    let mut spacing = None;
    let mut fraction = None;
    let mut ordinal = false;
    let mut slashed_zero = false;
    let mut values = Vec::new();
    for token in tokens {
        let value = match token.to_ascii_lowercase().as_str() {
            "lining-nums" => {
                set_exclusive_flag(&mut figure, true)?;
                FontVariantNumericValue::LiningNums
            }
            "oldstyle-nums" => {
                set_exclusive_flag(&mut figure, false)?;
                FontVariantNumericValue::OldstyleNums
            }
            "proportional-nums" => {
                set_exclusive_flag(&mut spacing, true)?;
                FontVariantNumericValue::ProportionalNums
            }
            "tabular-nums" => {
                set_exclusive_flag(&mut spacing, false)?;
                FontVariantNumericValue::TabularNums
            }
            "diagonal-fractions" => {
                set_exclusive_flag(&mut fraction, true)?;
                FontVariantNumericValue::DiagonalFractions
            }
            "stacked-fractions" => {
                set_exclusive_flag(&mut fraction, false)?;
                FontVariantNumericValue::StackedFractions
            }
            "ordinal" if !ordinal => {
                ordinal = true;
                FontVariantNumericValue::Ordinal
            }
            "slashed-zero" if !slashed_zero => {
                slashed_zero = true;
                FontVariantNumericValue::SlashedZero
            }
            _ => return None,
        };
        values.push(value);
    }
    (!values.is_empty()).then_some(FontVariantNumeric::Values(values))
}

pub(crate) fn parse_font_variant_alternates(value: &str) -> Option<FontVariantAlternates> {
    let tokens = split_css_component_values(value);
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("normal") {
        return Some(FontVariantAlternates::Normal);
    }
    let mut historical_forms = false;
    let mut stylistic = Vec::new();
    let mut styleset = Vec::new();
    let mut character_variant = Vec::new();
    let mut swash = Vec::new();
    let mut ornaments = Vec::new();
    let mut annotation = Vec::new();
    for token in tokens {
        if token.eq_ignore_ascii_case("historical-forms") {
            if historical_forms {
                return None;
            }
            historical_forms = true;
            continue;
        }
        if let Some(name) = parse_font_feature_value_function(token, "stylistic") {
            push_unique_alternate_name(&mut stylistic, name)?;
        } else if let Some(names) = parse_font_feature_value_function_list(token, "styleset") {
            for name in names {
                push_unique_alternate_name(&mut styleset, name)?;
            }
        } else if let Some(names) =
            parse_font_feature_value_function_list(token, "character-variant")
        {
            for name in names {
                push_unique_alternate_name(&mut character_variant, name)?;
            }
        } else if let Some(name) = parse_font_feature_value_function(token, "swash") {
            push_unique_alternate_name(&mut swash, name)?;
        } else if let Some(name) = parse_font_feature_value_function(token, "ornaments") {
            push_unique_alternate_name(&mut ornaments, name)?;
        } else if let Some(name) = parse_font_feature_value_function(token, "annotation") {
            push_unique_alternate_name(&mut annotation, name)?;
        } else {
            return None;
        }
    }
    let has_values = historical_forms
        || !stylistic.is_empty()
        || !styleset.is_empty()
        || !character_variant.is_empty()
        || !swash.is_empty()
        || !ornaments.is_empty()
        || !annotation.is_empty();
    has_values.then_some(FontVariantAlternates::Values {
        historical_forms,
        stylistic,
        styleset,
        character_variant,
        swash,
        ornaments,
        annotation,
    })
}

fn parse_font_feature_value_function(value: &str, name: &str) -> Option<String> {
    let names = parse_font_feature_value_function_list(value, name)?;
    (names.len() == 1).then(|| names[0].clone())
}

fn parse_font_feature_value_function_list(value: &str, name: &str) -> Option<Vec<String>> {
    let body = strip_ascii_function(value, name)?;
    let (argument, tail) = split_function_argument(body)?;
    if !tail.trim().is_empty() {
        return None;
    }
    let names = split_css_component_values(argument)
        .into_iter()
        .map(|name| name.to_ascii_lowercase())
        .filter(|name| font_feature_value_name_is_valid(name))
        .collect::<Vec<_>>();
    (!names.is_empty()).then_some(names)
}

fn font_feature_value_name_is_valid(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '-' || first.is_ascii_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '-' || character.is_ascii_alphanumeric()
        })
}

fn push_unique_alternate_name(names: &mut Vec<String>, name: String) -> Option<()> {
    if names.iter().any(|existing| existing == &name) {
        return None;
    }
    names.push(name);
    Some(())
}

pub(crate) fn parse_font_variant_emoji(value: &str) -> Option<FontVariantEmoji> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "normal" => Some(FontVariantEmoji::Normal),
        "text" => Some(FontVariantEmoji::Text),
        "emoji" => Some(FontVariantEmoji::Emoji),
        "unicode" => Some(FontVariantEmoji::Unicode),
        _ => None,
    }
}

pub(crate) fn parse_font_variant_east_asian(value: &str) -> Option<FontVariantEastAsian> {
    let tokens = split_css_component_values(value);
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("normal") {
        return Some(FontVariantEastAsian::Normal);
    }
    let mut variant = None;
    let mut width = None;
    let mut ruby = false;
    let mut values = Vec::new();
    for token in tokens {
        let value = match token.to_ascii_lowercase().as_str() {
            "jis78" => {
                set_exclusive_flag(&mut variant, true)?;
                FontVariantEastAsianValue::Jis78
            }
            "jis83" => {
                set_exclusive_flag(&mut variant, true)?;
                FontVariantEastAsianValue::Jis83
            }
            "jis90" => {
                set_exclusive_flag(&mut variant, true)?;
                FontVariantEastAsianValue::Jis90
            }
            "jis04" => {
                set_exclusive_flag(&mut variant, true)?;
                FontVariantEastAsianValue::Jis04
            }
            "simplified" => {
                set_exclusive_flag(&mut variant, true)?;
                FontVariantEastAsianValue::Simplified
            }
            "traditional" => {
                set_exclusive_flag(&mut variant, true)?;
                FontVariantEastAsianValue::Traditional
            }
            "full-width" => {
                set_exclusive_flag(&mut width, true)?;
                FontVariantEastAsianValue::FullWidth
            }
            "proportional-width" => {
                set_exclusive_flag(&mut width, false)?;
                FontVariantEastAsianValue::ProportionalWidth
            }
            "ruby" if !ruby => {
                ruby = true;
                FontVariantEastAsianValue::Ruby
            }
            _ => return None,
        };
        values.push(value);
    }
    (!values.is_empty()).then_some(FontVariantEastAsian::Values(values))
}

pub(crate) fn parse_font_variant(value: &str) -> Option<ParsedFontVariant> {
    let tokens = split_css_component_values(value);
    if tokens.len() == 1 {
        match tokens[0].to_ascii_lowercase().as_str() {
            "normal" => return Some(ParsedFontVariant::normal()),
            "none" => {
                return Some(ParsedFontVariant {
                    ligatures: FontVariantLigatures::None,
                    ..ParsedFontVariant::normal()
                });
            }
            _ => {}
        }
    }
    let mut ligature_tokens = Vec::new();
    let mut position_tokens = Vec::new();
    let mut caps_tokens = Vec::new();
    let mut numeric_tokens = Vec::new();
    let mut alternates_tokens = Vec::new();
    let mut east_asian_tokens = Vec::new();
    let mut emoji_tokens = Vec::new();
    for token in tokens {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "normal" | "none" => return None,
            "common-ligatures"
            | "no-common-ligatures"
            | "discretionary-ligatures"
            | "no-discretionary-ligatures"
            | "historical-ligatures"
            | "no-historical-ligatures"
            | "contextual"
            | "no-contextual" => ligature_tokens.push(token),
            "sub" | "super" => position_tokens.push(token),
            "small-caps" | "all-small-caps" | "petite-caps" | "all-petite-caps" | "unicase"
            | "titling-caps" => caps_tokens.push(token),
            "lining-nums" | "oldstyle-nums" | "proportional-nums" | "tabular-nums"
            | "diagonal-fractions" | "stacked-fractions" | "ordinal" | "slashed-zero" => {
                numeric_tokens.push(token)
            }
            "historical-forms" => alternates_tokens.push(token),
            "jis78" | "jis83" | "jis90" | "jis04" | "simplified" | "traditional" | "full-width"
            | "proportional-width" | "ruby" => east_asian_tokens.push(token),
            "text" | "emoji" | "unicode" => emoji_tokens.push(token),
            _ if parse_font_variant_alternates(token).is_some() => alternates_tokens.push(token),
            _ => return None,
        }
    }
    if tokens_are_empty([
        &ligature_tokens,
        &position_tokens,
        &caps_tokens,
        &numeric_tokens,
        &alternates_tokens,
        &east_asian_tokens,
        &emoji_tokens,
    ]) {
        return None;
    }
    let ligatures = if ligature_tokens.is_empty() {
        FontVariantLigatures::Normal
    } else {
        parse_font_variant_ligatures(&ligature_tokens.join(" "))?
    };
    let position = if position_tokens.is_empty() {
        FontVariantPosition::Normal
    } else {
        parse_font_variant_position(&position_tokens.join(" "))?
    };
    let caps = if caps_tokens.is_empty() {
        FontVariantCaps::Normal
    } else {
        parse_font_variant_caps(&caps_tokens.join(" "))?
    };
    let numeric = if numeric_tokens.is_empty() {
        FontVariantNumeric::Normal
    } else {
        parse_font_variant_numeric(&numeric_tokens.join(" "))?
    };
    let alternates = if alternates_tokens.is_empty() {
        FontVariantAlternates::Normal
    } else {
        parse_font_variant_alternates(&alternates_tokens.join(" "))?
    };
    let east_asian = if east_asian_tokens.is_empty() {
        FontVariantEastAsian::Normal
    } else {
        parse_font_variant_east_asian(&east_asian_tokens.join(" "))?
    };
    let emoji = if emoji_tokens.is_empty() {
        FontVariantEmoji::Normal
    } else if emoji_tokens.len() == 1 {
        parse_font_variant_emoji(emoji_tokens[0])?
    } else {
        return None;
    };
    Some(ParsedFontVariant {
        ligatures,
        position,
        caps,
        numeric,
        alternates,
        east_asian,
        emoji,
    })
}

fn tokens_are_empty<const N: usize>(groups: [&Vec<&str>; N]) -> bool {
    groups.iter().all(|group| group.is_empty())
}
