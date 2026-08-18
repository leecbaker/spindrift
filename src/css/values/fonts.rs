use super::*;
use crate::css::component_values::{
    css_leading_function_matching, css_leading_ident, css_single_ident, split_css_component_values,
    try_split_css_component_values,
};
use cssparser::{Parser, ParserInput, Token};

/// Parse a CSS `<counter-style>` value.
///
/// Except for CSS Counter Styles' non-overridable built-ins, predefined names
/// remain named styles so the cascaded UA or author `@counter-style` rule
/// supplies every descriptor during formatting.
/// <https://drafts.csswg.org/css-counter-styles-3/#predefined-counters>
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
    let name = css_single_ident(value)?;
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "disc" => Some(ListStyleType::Disc),
        "circle" => Some(ListStyleType::Circle),
        "square" => Some(ListStyleType::Square),
        "disclosure-open" => Some(ListStyleType::DisclosureOpen),
        "disclosure-closed" => Some(ListStyleType::DisclosureClosed),
        "decimal" => Some(ListStyleType::Decimal),
        "none" => Some(ListStyleType::None),
        "inside" | "outside" => None,
        // Counter style names are case-sensitive except for the predefined
        // names defined by CSS Counter Styles. Preserve an author-defined
        // spelling so lookup cannot accidentally select `foo` for `Foo`.
        // <https://drafts.csswg.org/css-counter-styles-3/#counter-style-name>
        _ => parse_counter_style_reference_name(value).map(ListStyleType::Named),
    }
}

pub(crate) fn parse_list_style_position(value: &str) -> Option<ListStylePosition> {
    try_split_css_component_values(value)?
        .into_iter()
        .map(css_single_ident)
        .collect::<Option<Vec<_>>>()?
        .into_iter()
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
    let (argument, tail) = css_leading_function_matching(value, "symbols")?;
    if !tail.trim().is_empty() {
        return None;
    }
    let mut system = CounterStyleSystem::Symbolic;
    let mut rest = argument.trim();
    if let Some((token, tail)) = css_leading_ident(rest)
        && let Some(parsed_system) = parse_symbols_system_keyword(&token)
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
        } else {
            // CSS Counter Styles Level 3 only permits strings and images in
            // symbols(). Unlike @counter-style's symbols descriptor, bare
            // identifiers are not valid anonymous counter-style symbols.
            // <https://drafts.csswg.org/css-counter-styles-3/#symbols-function>
            return Vec::new();
        }
    }
    symbols
}

/// Parse a counter-style reference that is syntactically a CSS `<custom-ident>`.
///
/// The CSS tokenizer, rather than an ASCII-only character check, decodes
/// escapes and accepts non-ASCII identifiers. Predefined counter style names
/// are canonicalized at each reference site; author-defined names retain their
/// exact decoded spelling and are therefore case-sensitive.
/// <https://drafts.csswg.org/css-values-4/#custom-idents>
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-name>
pub(crate) fn parse_counter_style_reference_name(value: &str) -> Option<String> {
    let name = css_single_ident(value.trim())?;
    (is_counter_name(&name) && !name.eq_ignore_ascii_case("default")).then(|| {
        canonical_predefined_counter_style_name(&name)
            .map(str::to_string)
            .unwrap_or(name)
    })
}

/// Return the canonical spelling of a predefined counter-style name.
///
/// CSS Counter Styles parses every predefined name ASCII-case-insensitively,
/// while author-defined names retain their spelling and remain case-sensitive.
/// This deliberately lives next to list-style parsing so all counter-style
/// reference sites share one classification.
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-name>
pub(crate) fn canonical_predefined_counter_style_name(value: &str) -> Option<&'static str> {
    const NAMES: &[&str] = &[
        "decimal",
        "disc",
        "square",
        "circle",
        "disclosure-open",
        "disclosure-closed",
        "decimal-leading-zero",
        "arabic-indic",
        "armenian",
        "upper-armenian",
        "lower-armenian",
        "bengali",
        "cambodian",
        "khmer",
        "cjk-decimal",
        "devanagari",
        "georgian",
        "gujarati",
        "gurmukhi",
        "hebrew",
        "kannada",
        "lao",
        "malayalam",
        "mongolian",
        "myanmar",
        "oriya",
        "persian",
        "lower-roman",
        "upper-roman",
        "tamil",
        "telugu",
        "thai",
        "tibetan",
        "lower-alpha",
        "lower-latin",
        "upper-alpha",
        "upper-latin",
        "cjk-earthly-branch",
        "cjk-heavenly-stem",
        "lower-greek",
        "hiragana",
        "hiragana-iroha",
        "katakana",
        "katakana-iroha",
        "japanese-informal",
        "japanese-formal",
        "korean-hangul-formal",
        "korean-hanja-informal",
        "korean-hanja-formal",
        "simp-chinese-informal",
        "simp-chinese-formal",
        "trad-chinese-informal",
        "trad-chinese-formal",
        "cjk-ideographic",
        "ethiopic-numeric",
    ];
    let lower = value.to_ascii_lowercase();
    NAMES.iter().copied().find(|name| *name == lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_shorthand_slash_is_independent_of_comment_boundaries() {
        let attached = parse_font_shorthand("150px/1 Ahem", 12.0, FontWeight::NORMAL)
            .expect("attached slash form");
        let spaced = parse_font_shorthand("150px / 1 Ahem", 12.0, FontWeight::NORMAL)
            .expect("spaced slash form");
        let commented =
            parse_font_shorthand("150px/**/ / /**/1 Ahem/**/", 12.0, FontWeight::NORMAL)
                .expect("comment-separated slash form");
        assert_eq!(attached, spaced);
        assert_eq!(attached, commented);
    }

    #[test]
    fn counter_style_references_use_css_custom_identifier_tokenization() {
        assert_eq!(
            parse_list_style_type(r"\3BB \3B1 "),
            Some(ListStyleType::Named("λα".to_string()))
        );
        assert_eq!(
            parse_list_style_type("Hiragana"),
            Some(ListStyleType::Named("hiragana".to_string()))
        );
        for value in ["inherit", "initial", "unset", "revert", "default"] {
            assert_eq!(parse_list_style_type(value), None, "{value}");
        }
    }
}

pub(crate) fn parse_font_family_names(value: &str) -> Vec<String> {
    parse_font_family_components(value)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

pub(crate) fn parse_font_family(value: &str) -> Option<FontFamily> {
    let components = parse_font_family_components(value);
    if components.is_empty() {
        return None;
    }
    let families = components
        .into_iter()
        .map(|(name, quoted)| {
            (!quoted)
                .then(|| generic_font_family(&name))
                .flatten()
                .unwrap_or_else(|| FontFamily::named(name))
        })
        .collect::<Vec<_>>();
    if families.len() == 1 {
        Some(families.into_iter().next().unwrap())
    } else {
        Some(FontFamily::List(families))
    }
}

/// Parse the comma-separated `font-family` list while retaining whether each
/// name was quoted. CSS generic-family keywords are keywords only when
/// unquoted; quoted `"serif"` or `"ui-serif"` are ordinary family names.
/// <https://www.w3.org/TR/css-fonts-4/#generic-font-families>
fn parse_font_family_components(value: &str) -> Vec<(String, bool)> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut components = Vec::new();
    let mut current = Vec::new();

    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        match token.clone() {
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            Token::Comma => {
                if !push_font_family_component(&mut current, &mut components) {
                    return Vec::new();
                }
            }
            Token::Ident(name) => current.push((name.to_string(), false)),
            Token::QuotedString(name) => current.push((name.to_string(), true)),
            _ => return Vec::new(),
        }
    }

    if push_font_family_component(&mut current, &mut components) {
        components
    } else {
        Vec::new()
    }
}

fn push_font_family_component(
    current: &mut Vec<(String, bool)>,
    components: &mut Vec<(String, bool)>,
) -> bool {
    if current.is_empty() {
        return true;
    }

    let component = std::mem::take(current);
    if component.len() == 1 {
        components.push(
            component
                .into_iter()
                .next()
                .expect("component is non-empty"),
        );
        return true;
    }

    if component.iter().any(|(_, quoted)| *quoted) {
        return false;
    }

    let name = component
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    components.push((name, false));
    true
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
    pub(crate) deferred_size: DeferredFontSize,
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
        layout_pt(inherited_font_size * 0.5),
        inherited_font_weight,
    )
}

pub(crate) fn parse_font_shorthand_with_parent_ch_advance(
    value: &str,
    inherited_font_size: f32,
    inherited_ch_advance: LayoutLength,
    inherited_font_weight: FontWeight,
) -> Option<ParsedFontShorthand> {
    parse_font_shorthand_with_line_height_font_size(
        value,
        inherited_font_size,
        inherited_ch_advance,
        inherited_font_weight,
        None,
        layout_pt(inherited_font_size * 1.2),
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
    inherited_ch_advance: LayoutLength,
    inherited_font_weight: FontWeight,
    line_height_font_size: Option<f32>,
    inherited_line_height: LayoutLength,
) -> Option<ParsedFontShorthand> {
    let tokens = split_font_shorthand_components(value)?;
    let size_index = tokens.iter().position(|token| {
        matches!(token, FontShorthandComponent::Value(token)
            if split_font_size_and_line_height(token, inherited_font_size, inherited_ch_advance).is_some())
    })?;
    let mut style = FontStyle::Normal;
    let mut weight = FontWeight::NORMAL;
    let mut width = FontWidth::NORMAL;
    let mut variant_caps = FontVariantCaps::Normal;
    for token in &tokens[..size_index] {
        let FontShorthandComponent::Value(token) = token else {
            return None;
        };
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

    let FontShorthandComponent::Value(size_value) = tokens[size_index] else {
        return None;
    };
    let (size, mut line_height) = split_font_size_and_line_height_with_line_height_font_size(
        size_value,
        inherited_font_size,
        inherited_ch_advance,
        line_height_font_size,
    )?;
    let size_token = split_font_token_on_slash(size_value)
        .map(|(size, _)| size)
        .unwrap_or(size_value);
    let deferred_size = parse_deferred_font_size(size_token)?;
    let mut family_start = size_index + 1;
    if line_height.is_none()
        && matches!(
            tokens.get(family_start),
            Some(FontShorthandComponent::Slash)
        )
    {
        let line_height_font_size = line_height_font_size.unwrap_or(size);
        line_height = tokens.get(family_start + 1).and_then(|token| match token {
            FontShorthandComponent::Value(token) => {
                parse_computed_line_height(token, line_height_font_size)
            }
            FontShorthandComponent::Slash => None,
        });
        line_height.clone()?;
        family_start += 2;
    }
    let family = tokens
        .get(family_start..)?
        .iter()
        .map(|token| match token {
            FontShorthandComponent::Value(token) => Some(*token),
            FontShorthandComponent::Slash => None,
        })
        .collect::<Option<Vec<_>>>()?
        .join(" ");
    let family = parse_font_family(&family)?;

    if let Some(line_height) = &mut line_height {
        line_height.resolve_inherited_line_height_relative_lengths(inherited_line_height);
    }

    Some(ParsedFontShorthand {
        style,
        weight,
        width,
        variant_caps,
        size,
        deferred_size,
        line_height,
        family,
    })
}

/// Top-level tokens relevant to CSS Fonts' `font` shorthand.
///
/// The shorthand's slash is a delimiter token, not a character attached to
/// either neighboring component. In particular, `var()` substitution can put
/// a comment token between `/` and the line-height value.
/// <https://drafts.csswg.org/css-fonts-4/#font-prop>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FontShorthandComponent<'a> {
    Value(&'a str),
    Slash,
}

fn split_font_shorthand_components(value: &str) -> Option<Vec<FontShorthandComponent<'_>>> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut components = Vec::new();
    let mut start = None;
    while !parser.is_exhausted() {
        let token_start = parser.position();
        let token = parser
            .next_including_whitespace_and_comments()
            .ok()?
            .clone();
        if token.is_parse_error() {
            return None;
        }
        if matches!(token, Token::WhiteSpace(_) | Token::Comment(_)) {
            if let Some(start) = start.take() {
                let value = parser.slice(start..token_start).trim();
                if !value.is_empty() {
                    components.push(FontShorthandComponent::Value(value));
                }
            }
            continue;
        }
        if matches!(token, Token::Delim('/')) {
            if let Some(start) = start.take() {
                let value = parser.slice(start..token_start).trim();
                if !value.is_empty() {
                    components.push(FontShorthandComponent::Value(value));
                }
            }
            components.push(FontShorthandComponent::Slash);
            continue;
        }
        start.get_or_insert(token_start);
        if matches!(
            token,
            Token::Function(_)
                | Token::ParenthesisBlock
                | Token::SquareBracketBlock
                | Token::CurlyBracketBlock
        ) && parser
            .parse_nested_block(|nested| {
                crate::css::component_values::validate_component_value_list_from_parser(nested)
                    .then_some(())
                    .ok_or_else(|| nested.new_custom_error::<(), ()>(()))
            })
            .is_err()
        {
            return None;
        }
    }
    if let Some(start) = start {
        let value = parser.slice_from(start).trim();
        if !value.is_empty() {
            components.push(FontShorthandComponent::Value(value));
        }
    }
    Some(components)
}

fn split_font_size_and_line_height(
    token: &str,
    inherited_font_size: f32,
    inherited_ch_advance: LayoutLength,
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
    inherited_ch_advance: LayoutLength,
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

pub(crate) fn generic_font_family(value: &str) -> Option<FontFamily> {
    match value.trim().to_ascii_lowercase().as_str() {
        "serif" => Some(FontFamily::Serif),
        "monospace" => Some(FontFamily::Monospace),
        "sans-serif" | "sans serif" => Some(FontFamily::SansSerif),
        "system-ui" => Some(FontFamily::SystemUi),
        "ui-serif" => Some(FontFamily::UiSerif),
        "ui-sans-serif" => Some(FontFamily::UiSansSerif),
        "ui-monospace" => Some(FontFamily::UiMonospace),
        "ui-rounded" => Some(FontFamily::UiRounded),
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
            let value = parse_font_size_adjust_number(token)?;
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

/// Parse the dimensionless numeric grammar accepted by `font-size-adjust`.
///
/// CSS Values permits a `<number>` in this property to be a `calc()` expression
/// whose result is dimensionless. Keeping this separate from length math avoids
/// silently accepting dimensions or percentages in the descriptor grammar.
/// <https://www.w3.org/TR/css-fonts-5/#font-size-adjust-prop>
/// <https://www.w3.org/TR/css-values-4/#calc-syntax>
fn parse_font_size_adjust_number(value: &str) -> Option<f32> {
    let value = value.trim();
    if let Some(inner) = value
        .strip_prefix("calc(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let mut parser = FontSizeAdjustNumberParser::new(inner);
        let result = parser.sum()?;
        parser.skip_whitespace();
        return parser
            .input
            .get(parser.position..)
            .filter(|rest| rest.is_empty())
            .map(|_| result);
    }
    value.parse().ok()
}

struct FontSizeAdjustNumberParser<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> FontSizeAdjustNumberParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn sum(&mut self) -> Option<f32> {
        let mut result = self.product()?;
        loop {
            self.skip_whitespace();
            let operator = self.next_byte();
            match operator {
                Some(b'+') => result += self.product()?,
                Some(b'-') => result -= self.product()?,
                Some(_) => {
                    self.position = self.position.saturating_sub(1);
                    return Some(result);
                }
                None => return Some(result),
            }
        }
    }

    fn product(&mut self) -> Option<f32> {
        let mut result = self.factor()?;
        loop {
            self.skip_whitespace();
            let operator = self.next_byte();
            match operator {
                Some(b'*') => result *= self.factor()?,
                Some(b'/') => {
                    let divisor = self.factor()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    result /= divisor;
                }
                Some(_) => {
                    self.position = self.position.saturating_sub(1);
                    return Some(result);
                }
                None => return Some(result),
            }
        }
    }

    fn factor(&mut self) -> Option<f32> {
        self.skip_whitespace();
        let sign = match self.next_byte() {
            Some(b'+') => 1.0,
            Some(b'-') => -1.0,
            Some(_) => {
                self.position = self.position.saturating_sub(1);
                1.0
            }
            None => return None,
        };
        self.skip_whitespace();
        if self.peek_byte() == Some(b'(') {
            self.position += 1;
            let value = self.sum()?;
            self.skip_whitespace();
            if self.next_byte() != Some(b')') {
                return None;
            }
            return Some(sign * value);
        }
        let start = self.position;
        while self.peek_byte().is_some_and(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-')
        }) {
            self.position += 1;
        }
        (start != self.position)
            .then(|| self.input[start..self.position].parse::<f32>().ok())
            .flatten()
            .map(|value| sign * value)
    }

    fn skip_whitespace(&mut self) {
        while self
            .peek_byte()
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            self.position += 1;
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.position).cloned()
    }

    fn next_byte(&mut self) -> Option<u8> {
        let byte = self.peek_byte()?;
        self.position += 1;
        Some(byte)
    }
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
    let value = value.trim();
    match value.to_ascii_lowercase().as_str() {
        "normal" => Some(FontStyle::Normal),
        "italic" => Some(FontStyle::Italic),
        "oblique" => Some(FontStyle::DEFAULT_OBLIQUE),
        _ if value.len() > "oblique ".len()
            && value[.."oblique ".len()].eq_ignore_ascii_case("oblique ") =>
        {
            value["oblique ".len()..]
                .trim()
                .strip_suffix("deg")
                .and_then(|angle| angle.trim().parse::<f32>().ok())
                .filter(|angle| angle.is_finite() && (-90.0..=90.0).contains(angle))
                .map(|angle| FontStyle::Oblique(angle.to_bits()))
        }
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

pub(crate) fn parse_font_synthesis(value: &str) -> Option<FontSynthesis> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(FontSynthesis::NONE);
    }
    let mut synthesis = FontSynthesis::NONE;
    let mut seen = false;
    for token in value.split_ascii_whitespace() {
        let enabled = match token.to_ascii_lowercase().as_str() {
            "weight" => &mut synthesis.weight,
            "style" => &mut synthesis.style,
            "small-caps" => &mut synthesis.small_caps,
            "position" => &mut synthesis.position,
            _ => return None,
        };
        if *enabled {
            return None;
        }
        *enabled = true;
        seen = true;
    }
    seen.then_some(synthesis)
}

pub(crate) fn parse_font_synthesis_subproperty(value: &str) -> Option<bool> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(true),
        "none" => Some(false),
        _ => None,
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

/// Parse the inherited low-level variation-axis map. Later duplicate tags win
/// and `normal` clears all explicit coordinates.
/// <https://www.w3.org/TR/css-fonts-4/#font-variation-settings-def>
pub(crate) fn parse_font_variation_settings(value: &str) -> Option<FontVariationSettings> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(FontVariationSettings::NORMAL);
    }
    let mut settings = Vec::<FontVariationSetting>::new();
    for item in split_top_level_commas(value) {
        let (tag, tail) = parse_css_string_token(item.trim())?;
        let tag = parse_opentype_tag(&tag)?;
        let value = tail.trim().parse::<f32>().ok()?;
        if !value.is_finite() || tail.trim().split_ascii_whitespace().nth(1).is_some() {
            return None;
        }
        if let Some(existing) = settings.iter_mut().find(|setting| setting.tag == tag) {
            existing.value = value.to_bits();
        } else {
            settings.push(FontVariationSetting {
                tag,
                value: value.to_bits(),
            });
        }
    }
    (!settings.is_empty()).then(|| {
        settings.sort_by_key(|setting| setting.tag);
        FontVariationSettings(settings)
    })
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

/// Parse the CSS `font-language-override` longhand.
///
/// The serialized CSS form uses three-letter tags such as `"TRK"`; OpenType
/// represents those as a space-padded four-byte tag. Tags are case-sensitive,
/// so four printable ASCII bytes are retained exactly while an invalid string
/// invalidates the declaration.
/// <https://drafts.csswg.org/css-fonts-4/#font-language-override-prop>
pub(crate) fn parse_font_language_override(value: &str) -> Option<FontLanguageOverride> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(FontLanguageOverride::Normal);
    }
    let (tag, tail) = parse_css_string_token(value)?;
    if !tail.trim().is_empty() || !(3..=4).contains(&tag.len()) || !tag.is_ascii() {
        return None;
    }
    let mut padded = [b' '; 4];
    for (slot, byte) in padded.iter_mut().zip(tag.bytes()) {
        if !byte.is_ascii_graphic() {
            return None;
        }
        *slot = byte;
    }
    Some(FontLanguageOverride::OpenType(padded))
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
        } else {
            let name = parse_font_feature_value_function(token, "annotation")?;
            push_unique_alternate_name(&mut annotation, name)?;
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
    let (argument, tail) = css_leading_function_matching(value, name)?;
    if !tail.trim().is_empty() {
        return None;
    }
    // CSS Fonts' `<custom-ident>#` alias lists accept comma separation as
    // well as whitespace between component values. Split commas first so a
    // compact `styleset(foo,bar)` does not become one invalid identifier.
    // <https://www.w3.org/TR/css-fonts-4/#font-variant-alternates-prop>
    let names = split_top_level_commas(argument)
        .into_iter()
        .flat_map(split_css_component_values)
        .map(css_single_ident)
        .collect::<Option<Vec<_>>>()?
        .into_iter()
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

/// Parse CSS Fonts' `font-palette` property excluding the named palette
/// definition lookup, which happens after the stylesheet has been loaded.
/// <https://www.w3.org/TR/css-fonts-4/#font-palette-prop>
pub(crate) fn parse_font_palette(value: &str) -> Option<FontPalette> {
    let value = trim_css_value(value);
    match value.to_ascii_lowercase().as_str() {
        "normal" => Some(FontPalette::Normal),
        "light" => Some(FontPalette::Light),
        "dark" => Some(FontPalette::Dark),
        _ => value
            .strip_prefix("palette ")
            .and_then(|index| index.trim().parse::<u16>().ok())
            .map(FontPalette::Index)
            .or_else(|| {
                value
                    .strip_prefix("--")
                    .filter(|name| !name.is_empty())
                    .map(|_| FontPalette::Named(value.to_string()))
            }),
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
