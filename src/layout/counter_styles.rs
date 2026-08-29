use std::collections::HashSet;

use icu_segmenter::GraphemeClusterSegmenter;

use super::*;
use crate::css::CounterStyleRangeInterval;

/// The writing context needed by predefined styles whose representation
/// depends on the element's inline and block directions.
///
/// CSS Counter Styles defines the disclosure styles in terms of these
/// directions, so the same context must reach direct use, generated content,
/// and a custom style that `extends` a disclosure style.
/// <https://drafts.csswg.org/css-counter-styles-3/#disclosure-open>
/// <https://drafts.csswg.org/css-counter-styles-3/#disclosure-closed>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct CounterStyleRenderContext {
    direction: Direction,
    writing_mode: WritingMode,
}

impl CounterStyleRenderContext {
    pub(in crate::layout) fn for_style(style: &ComputedStyle) -> Self {
        Self {
            direction: style.direction,
            writing_mode: style.writing_mode,
        }
    }

    pub(super) fn default_context() -> Self {
        Self {
            direction: Direction::Ltr,
            writing_mode: WritingMode::HorizontalTb,
        }
    }
}

pub(in crate::layout) fn counter_text(
    list_style_type: ListStyleType,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<String> {
    counter_text_with_context(
        list_style_type,
        ordinal,
        counter_styles,
        CounterStyleRenderContext::default_context(),
    )
}

pub(in crate::layout) fn counter_text_with_context(
    list_style_type: ListStyleType,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
) -> Option<String> {
    match list_style_type {
        ListStyleType::Disc => Some("\u{2022}".to_string()),
        ListStyleType::Circle => Some("\u{25e6}".to_string()),
        ListStyleType::Square => Some("\u{25aa}".to_string()),
        ListStyleType::DisclosureOpen => Some(disclosure_symbol(true, render_context).to_string()),
        ListStyleType::DisclosureClosed => {
            Some(disclosure_symbol(false, render_context).to_string())
        }
        ListStyleType::Decimal => Some(ordinal.to_string()),
        ListStyleType::String(text) => Some(text),
        ListStyleType::Anonymous(rule) => {
            custom_counter_text_with_context(&rule, ordinal, counter_styles, render_context)
        }
        ListStyleType::Named(name) => counter_style_rule(&name, counter_styles)
            .and_then(|rule| {
                custom_counter_text_with_context(rule, ordinal, counter_styles, render_context)
            })
            .or_else(|| {
                complex_predefined_counter_style(&name).and_then(|effective| {
                    let mut fallback_context = CounterStyleFallbackContext::default();
                    fallback_context.visit(&name);
                    custom_counter_text_with_effective(
                        &effective,
                        ordinal,
                        counter_styles,
                        render_context,
                        &mut fallback_context,
                    )
                })
            })
            .or_else(|| {
                predefined_named_counter_text_with_context(&name, ordinal, render_context)
                    .map(|(text, _)| text)
            })
            .or_else(|| Some(ordinal.to_string())),
        ListStyleType::None => None,
    }
}

#[cfg(test)]
pub(in crate::layout) fn custom_counter_marker_text(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<(String, bool)> {
    custom_counter_marker_text_with_context(
        rule,
        ordinal,
        counter_styles,
        CounterStyleRenderContext::default_context(),
    )
}

pub(in crate::layout) fn custom_counter_marker_text_with_context(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
) -> Option<(String, bool)> {
    let effective = resolve_counter_style(rule, counter_styles, 0);
    let mut fallback_context = CounterStyleFallbackContext::for_rule(rule);
    custom_counter_marker_text_with_effective(
        &effective,
        ordinal,
        counter_styles,
        render_context,
        &mut fallback_context,
    )
}

pub(super) fn custom_counter_marker_text_with_effective(
    effective: &EffectiveCounterStyle,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
    fallback_context: &mut CounterStyleFallbackContext,
) -> Option<(String, bool)> {
    custom_counter_text_with_effective(
        effective,
        ordinal,
        counter_styles,
        render_context,
        fallback_context,
    )
    .map(|text| {
        (
            format!("{}{}{}", effective.prefix, text, effective.suffix),
            false,
        )
    })
}

#[cfg(test)]
pub(in crate::layout) fn custom_counter_text(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<String> {
    custom_counter_text_with_context(
        rule,
        ordinal,
        counter_styles,
        CounterStyleRenderContext::default_context(),
    )
}

pub(in crate::layout) fn custom_counter_text_with_context(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
) -> Option<String> {
    let effective = resolve_counter_style(rule, counter_styles, 0);
    let mut fallback_context = CounterStyleFallbackContext::for_rule(rule);
    custom_counter_text_with_effective(
        &effective,
        ordinal,
        counter_styles,
        render_context,
        &mut fallback_context,
    )
}

/// State held while producing one counter representation.
///
/// CSS Counter Styles falls back to decimal when a fallback chain repeats a
/// counter style. Tracking the normalized names makes that rule independent
/// of arbitrary nesting depth while preserving case-sensitive custom names.
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-fallback>
#[derive(Default)]
pub(super) struct CounterStyleFallbackContext {
    visited: HashSet<String>,
}

impl CounterStyleFallbackContext {
    pub(super) fn for_rule(rule: &CounterStyleRule) -> Self {
        let mut context = Self::default();
        if !rule.name.is_empty() {
            context.visit(&rule.name);
        }
        context
    }

    pub(super) fn visit(&mut self, name: &str) -> bool {
        let name = crate::css::canonical_predefined_counter_style_name(name).unwrap_or(name);
        self.visited.insert(name.to_string())
    }
}

fn custom_counter_text_with_effective(
    style: &EffectiveCounterStyle,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
    fallback_context: &mut CounterStyleFallbackContext,
) -> Option<String> {
    if !counter_style_range_contains(&style.range, &style.system, ordinal) {
        return Some(fallback_counter_text(
            &style.fallback,
            ordinal,
            counter_styles,
            render_context,
            fallback_context,
        ));
    }

    let absolute_ordinal = if ordinal < 0 {
        i32::try_from(i64::from(ordinal).abs()).ok()?
    } else {
        ordinal
    };
    let is_complex_predefined = style.predefined.is_some();
    // Fixed and cyclic systems select their symbols directly from the signed
    // ordinal. A negative ordinal is therefore not a magnitude requiring the
    // `negative` affix.
    // <https://drafts.csswg.org/css-counter-styles-3/#fixed-system>
    // <https://drafts.csswg.org/css-counter-styles-3/#cyclic-system>
    let uses_negative_affix = ordinal < 0
        && !is_complex_predefined
        && !matches!(
            style.system,
            CounterStyleSystem::Cyclic | CounterStyleSystem::Fixed(_)
        );
    let Some(mut text) = style
        .predefined
        .and_then(|name| {
            predefined_named_counter_text_with_context(name, ordinal, render_context)
                .map(|(text, _)| text)
        })
        .or_else(|| match style.system {
            CounterStyleSystem::Cyclic => cyclic_counter_text(ordinal, &style.symbols),
            CounterStyleSystem::Numeric => numeric_counter_text(absolute_ordinal, &style.symbols),
            CounterStyleSystem::Alphabetic => {
                alphabetic_counter_text(absolute_ordinal, &style.symbols)
            }
            CounterStyleSystem::Symbolic => symbolic_counter_text(absolute_ordinal, &style.symbols),
            CounterStyleSystem::Fixed(first) => fixed_counter_text(ordinal, first, &style.symbols),
            CounterStyleSystem::Additive => {
                additive_counter_text(absolute_ordinal, &style.additive_symbols)
            }
            CounterStyleSystem::Extends(_) => None,
        })
    else {
        // A fallback is a complete new representation. It does not inherit
        // the failed style's pad or negative descriptors; the originally
        // requested marker keeps its prefix and suffix at the call site.
        // <https://drafts.csswg.org/css-counter-styles-3/#counter-style-fallback>
        return Some(fallback_counter_text(
            &style.fallback,
            ordinal,
            counter_styles,
            render_context,
            fallback_context,
        ));
    };
    if let Some((width, symbol)) = &style.pad {
        // `pad` measures the representation after a negative affix has been
        // accounted for, in extended grapheme clusters rather than Unicode
        // scalar values. The affix is still appended after padding below.
        // <https://drafts.csswg.org/css-counter-styles-3/#counter-style-pad>
        let negative_length = if uses_negative_affix {
            counter_representation_grapheme_length(&style.negative.0)
                + counter_representation_grapheme_length(&style.negative.1)
        } else {
            0
        };
        let text_len = counter_representation_grapheme_length(&text) + negative_length;
        if text_len < *width {
            text = format!("{}{}", symbol.repeat(*width - text_len), text);
        }
    }
    if uses_negative_affix {
        text = format!("{}{}{}", style.negative.0, text, style.negative.1);
    }
    Some(text)
}

fn counter_representation_grapheme_length(text: &str) -> usize {
    GraphemeClusterSegmenter::new()
        .segment_str(text)
        .count()
        .saturating_sub(1)
}

fn fallback_counter_text(
    fallback: &str,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    render_context: CounterStyleRenderContext,
    fallback_context: &mut CounterStyleFallbackContext,
) -> String {
    if !fallback_context.visit(fallback) {
        return ordinal.to_string();
    }
    if let Some(rule) = counter_style_rule(fallback, counter_styles) {
        let effective = resolve_counter_style(rule, counter_styles, 0);
        return custom_counter_text_with_effective(
            &effective,
            ordinal,
            counter_styles,
            render_context,
            fallback_context,
        )
        .unwrap_or_else(|| ordinal.to_string());
    }
    let style = css::parse_list_style_type(fallback).unwrap_or(ListStyleType::Decimal);
    match style {
        ListStyleType::Named(name) if name == fallback => ordinal.to_string(),
        other => counter_text_with_context(other, ordinal, counter_styles, render_context)
            .unwrap_or_else(|| ordinal.to_string()),
    }
}

/// Resolve a counter-style reference without erasing the case distinction for
/// author-defined names.  Only the predefined names are ASCII-case-insensitive.
/// <https://drafts.csswg.org/css-counter-styles-3/#counter-style-name>
pub(in crate::layout) fn counter_style_rule<'a>(
    name: &str,
    counter_styles: &'a HashMap<String, CounterStyleRule>,
) -> Option<&'a CounterStyleRule> {
    counter_styles.get(name).or_else(|| {
        crate::css::canonical_predefined_counter_style_name(name)
            .and_then(|canonical| counter_styles.get(canonical))
    })
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct EffectiveCounterStyle {
    pub(in crate::layout) system: CounterStyleSystem,
    pub(in crate::layout) symbols: Vec<String>,
    pub(in crate::layout) additive_symbols: Vec<(i32, String)>,
    pub(in crate::layout) prefix: String,
    pub(in crate::layout) suffix: String,
    pub(in crate::layout) negative: (String, String),
    pub(in crate::layout) pad: Option<(usize, String)>,
    pub(in crate::layout) range: CounterStyleRange,
    pub(in crate::layout) fallback: String,
    /// Complex predefined styles have algorithms that cannot be expressed by
    /// the simple `system` descriptor, but remain valid `extends` targets.
    pub(in crate::layout) predefined: Option<&'static str>,
}

fn default_effective_counter_style() -> EffectiveCounterStyle {
    EffectiveCounterStyle {
        system: CounterStyleSystem::Numeric,
        symbols: decimal_counter_symbols(),
        additive_symbols: Vec::new(),
        prefix: String::new(),
        suffix: ". ".to_string(),
        negative: ("-".to_string(), String::new()),
        pad: None,
        range: CounterStyleRange::Auto,
        fallback: "decimal".to_string(),
        predefined: None,
    }
}

#[derive(Debug, Clone)]
struct CounterStyleResolution {
    effective: EffectiveCounterStyle,
    /// Names in the current `extends` cycle. A style in this set replaces its
    /// inherited base with decimal, while a non-participating caller may still
    /// inherit the repaired style and its descriptors.
    /// <https://drafts.csswg.org/css-counter-styles-3/#extends-system>
    cyclic_names: Vec<String>,
}

pub(in crate::layout) fn resolve_counter_style(
    rule: &CounterStyleRule,
    counter_styles: &HashMap<String, CounterStyleRule>,
    _depth: usize,
) -> EffectiveCounterStyle {
    let mut visiting = Vec::new();
    if !rule.name.is_empty() {
        visiting.push(rule.name.clone());
    }
    resolve_counter_style_inner(rule, counter_styles, &mut visiting).effective
}

fn resolve_counter_style_inner(
    rule: &CounterStyleRule,
    counter_styles: &HashMap<String, CounterStyleRule>,
    visiting: &mut Vec<String>,
) -> CounterStyleResolution {
    let inherited = if let CounterStyleSystem::Extends(name) = &rule.system {
        if let Some(cycle_start) = visiting.iter().position(|visited| visited == name) {
            CounterStyleResolution {
                effective: default_effective_counter_style(),
                cyclic_names: visiting[cycle_start..].to_vec(),
            }
        } else if let Some(effective) = complex_predefined_counter_style(name) {
            CounterStyleResolution {
                effective,
                cyclic_names: Vec::new(),
            }
        } else if let Some(target) = counter_style_rule(name, counter_styles) {
            visiting.push(name.clone());
            let resolved = resolve_counter_style_inner(target, counter_styles, visiting);
            visiting.pop();
            resolved
        } else {
            CounterStyleResolution {
                effective: default_effective_counter_style(),
                cyclic_names: Vec::new(),
            }
        }
    } else {
        CounterStyleResolution {
            effective: default_effective_counter_style(),
            cyclic_names: Vec::new(),
        }
    };
    let mut effective = if inherited.cyclic_names.iter().any(|name| name == &rule.name) {
        default_effective_counter_style()
    } else {
        inherited.effective
    };
    if !matches!(rule.system, CounterStyleSystem::Extends(_)) {
        effective.system = rule.system.clone();
        effective.symbols = rule.symbols.clone();
        effective.additive_symbols = rule.additive_symbols.clone();
        effective.predefined = None;
    }
    if let Some(prefix) = &rule.prefix {
        effective.prefix = prefix.clone();
    }
    if let Some(suffix) = &rule.suffix {
        effective.suffix = suffix.clone();
    }
    if let Some(negative) = &rule.negative {
        effective.negative = negative.clone();
    }
    if let Some(pad) = &rule.pad {
        effective.pad = Some(pad.clone());
    }
    if let Some(range) = &rule.range {
        effective.range = range.clone();
    }
    if let Some(fallback) = &rule.fallback {
        effective.fallback = fallback.clone();
    }
    CounterStyleResolution {
        effective,
        cyclic_names: inherited.cyclic_names,
    }
}

/// Construct the effective descriptor set for the complex styles which the
/// spec defines algorithmically rather than through the normative UA sheet.
/// They must nevertheless be valid `extends` targets.
/// <https://drafts.csswg.org/css-counter-styles-3/#complex-counters>
pub(super) fn complex_predefined_counter_style(name: &str) -> Option<EffectiveCounterStyle> {
    let canonical = crate::css::canonical_predefined_counter_style_name(name)?;
    let (suffix, range, fallback) = match canonical {
        "disclosure-open" | "disclosure-closed" => (" ", CounterStyleRange::Auto, "decimal"),
        "simp-chinese-informal"
        | "simp-chinese-formal"
        | "trad-chinese-informal"
        | "trad-chinese-formal"
        | "cjk-ideographic" => (
            "、",
            CounterStyleRange::Intervals(vec![CounterStyleRangeInterval {
                start: -9_999,
                end: 9_999,
            }]),
            "cjk-decimal",
        ),
        "ethiopic-numeric" => ("/ ", CounterStyleRange::Auto, "decimal"),
        _ => return None,
    };
    Some(EffectiveCounterStyle {
        suffix: suffix.to_string(),
        range,
        fallback: fallback.to_string(),
        predefined: Some(canonical),
        ..default_effective_counter_style()
    })
}

pub(in crate::layout) fn counter_style_range_contains(
    range: &CounterStyleRange,
    system: &CounterStyleSystem,
    ordinal: i32,
) -> bool {
    let value = i64::from(ordinal);
    match range {
        CounterStyleRange::Auto => match system {
            CounterStyleSystem::Alphabetic | CounterStyleSystem::Symbolic => ordinal >= 1,
            CounterStyleSystem::Additive => ordinal >= 0,
            _ => true,
        },
        CounterStyleRange::Intervals(intervals) => intervals
            .iter()
            .any(|interval| value >= interval.start && value <= interval.end),
    }
}

pub(in crate::layout) fn decimal_counter_symbols() -> Vec<String> {
    (0..=9).map(|digit| digit.to_string()).collect()
}

pub(in crate::layout) fn cyclic_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    let count = i32::try_from(symbols.len()).ok()?;
    if count == 0 {
        return None;
    }
    let position = (index - 1).rem_euclid(count);
    symbols.get(position as usize).cloned()
}

pub(in crate::layout) fn fixed_counter_text(
    index: i32,
    first: i32,
    symbols: &[String],
) -> Option<String> {
    let offset = index.checked_sub(first)?;
    let offset = usize::try_from(offset).ok()?;
    symbols.get(offset).cloned()
}

pub(in crate::layout) fn symbolic_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    if index <= 0 || symbols.is_empty() {
        return None;
    }
    let count = i32::try_from(symbols.len()).ok()?;
    let symbol = symbols.get(((index - 1) % count) as usize)?;
    let repetitions = ((index + count - 1) / count) as usize;
    Some(symbol.repeat(repetitions))
}

pub(in crate::layout) fn alphabetic_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    if index <= 0 || symbols.len() < 2 {
        return None;
    }
    let base = symbols.len();
    let mut value = index as usize;
    let mut output = Vec::new();
    while value > 0 {
        value -= 1;
        output.push(symbols[value % base].as_str());
        value /= base;
    }
    Some(output.iter().rev().cloned().collect::<String>())
}

pub(in crate::layout) fn numeric_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    if symbols.len() < 2 {
        return None;
    }
    let base = i64::try_from(symbols.len()).ok()?;
    let sign = if index < 0 { "-" } else { "" };
    let mut value = i64::from(index).abs();
    if value == 0 {
        return symbols.first().map(|zero| format!("{sign}{zero}"));
    }
    let mut output = Vec::new();
    while value > 0 {
        let digit = usize::try_from(value % base).ok()?;
        output.push(symbols.get(digit)?.as_str());
        value /= base;
    }
    Some(format!(
        "{sign}{}",
        output.iter().rev().cloned().collect::<String>()
    ))
}

pub(in crate::layout) fn additive_counter_text(
    index: i32,
    symbols: &[(i32, String)],
) -> Option<String> {
    if index == 0 {
        return symbols
            .iter()
            .find_map(|(weight, symbol)| (*weight == 0).then(|| symbol.clone()));
    }
    if index < 0 {
        return None;
    }
    let mut value = index;
    let mut output = String::new();
    for (weight, symbol) in symbols {
        if *weight <= 0 {
            continue;
        }
        while value >= *weight {
            output.push_str(symbol);
            value -= *weight;
        }
    }
    (value == 0).then_some(output)
}

pub(in crate::layout) fn predefined_named_counter_text_with_context(
    name: &str,
    ordinal: i32,
    render_context: CounterStyleRenderContext,
) -> Option<(String, &'static str)> {
    match name {
        "disclosure-open" => Some((disclosure_symbol(true, render_context).to_string(), " ")),
        "disclosure-closed" => Some((disclosure_symbol(false, render_context).to_string(), " ")),
        "simp-chinese-informal" => {
            chinese_longhand_marker(ordinal, ChineseLonghandStyle::SimplifiedInformal)
                .map(|text| (text, "、"))
        }
        "simp-chinese-formal" => {
            chinese_longhand_marker(ordinal, ChineseLonghandStyle::SimplifiedFormal)
                .map(|text| (text, "、"))
        }
        "trad-chinese-informal" | "cjk-ideographic" => {
            chinese_longhand_marker(ordinal, ChineseLonghandStyle::TraditionalInformal)
                .map(|text| (text, "、"))
        }
        "trad-chinese-formal" => {
            chinese_longhand_marker(ordinal, ChineseLonghandStyle::TraditionalFormal)
                .map(|text| (text, "、"))
        }
        "ethiopic-numeric" => ethiopic_numeric_marker(ordinal).map(|text| (text, "/ ")),
        _ => None,
    }
}

/// Return the disclosure triangle selected by the element's writing context.
///
/// The closed marker points toward the block's inline-end side and the open
/// marker toward its block-end side, except that vertical writing swaps the
/// role expected by the disclosure widget's expansion axis.
/// <https://drafts.csswg.org/css-counter-styles-3/#disclosure-open>
/// <https://drafts.csswg.org/css-counter-styles-3/#disclosure-closed>
fn disclosure_symbol(open: bool, context: CounterStyleRenderContext) -> char {
    match (context.writing_mode, context.direction, open) {
        (WritingMode::HorizontalTb, _, true) => '\u{25be}',
        (WritingMode::HorizontalTb, Direction::Ltr, false) => '\u{25b8}',
        (WritingMode::HorizontalTb, Direction::Rtl, false) => '\u{25c2}',
        (WritingMode::VerticalLr, Direction::Ltr, true) => '\u{25b8}',
        (WritingMode::VerticalLr, Direction::Rtl, true) => '\u{25b8}',
        (WritingMode::VerticalLr, Direction::Ltr, false) => '\u{25be}',
        (WritingMode::VerticalLr, Direction::Rtl, false) => '\u{25b4}',
        (WritingMode::VerticalRl, Direction::Ltr, true) => '\u{25c2}',
        (WritingMode::VerticalRl, Direction::Rtl, true) => '\u{25c2}',
        (WritingMode::VerticalRl, Direction::Ltr, false) => '\u{25be}',
        (WritingMode::VerticalRl, Direction::Rtl, false) => '\u{25b4}',
        // Sideways modes have horizontal typographic orientation but a
        // vertical block flow. They use the matching vertical geometry.
        (WritingMode::SidewaysLr, Direction::Ltr, true) => '\u{25b8}',
        (WritingMode::SidewaysLr, Direction::Rtl, true) => '\u{25b8}',
        (WritingMode::SidewaysLr, Direction::Ltr, false) => '\u{25be}',
        (WritingMode::SidewaysLr, Direction::Rtl, false) => '\u{25b4}',
        (WritingMode::SidewaysRl, Direction::Ltr, true) => '\u{25c2}',
        (WritingMode::SidewaysRl, Direction::Rtl, true) => '\u{25c2}',
        (WritingMode::SidewaysRl, Direction::Ltr, false) => '\u{25be}',
        (WritingMode::SidewaysRl, Direction::Rtl, false) => '\u{25b4}',
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum ChineseLonghandStyle {
    SimplifiedInformal,
    SimplifiedFormal,
    TraditionalInformal,
    TraditionalFormal,
}

impl ChineseLonghandStyle {
    pub(in crate::layout) fn digits(self) -> &'static [&'static str; 10] {
        match self {
            Self::SimplifiedInformal | Self::TraditionalInformal => {
                &["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"]
            }
            Self::SimplifiedFormal => &["零", "壹", "贰", "叁", "肆", "伍", "陆", "柒", "捌", "玖"],
            Self::TraditionalFormal => {
                &["零", "壹", "貳", "參", "肆", "伍", "陸", "柒", "捌", "玖"]
            }
        }
    }

    pub(in crate::layout) fn markers(self) -> &'static [&'static str; 4] {
        match self {
            Self::SimplifiedInformal | Self::TraditionalInformal => &["", "十", "百", "千"],
            Self::SimplifiedFormal => &["", "拾", "佰", "仟"],
            Self::TraditionalFormal => &["", "拾", "佰", "仟"],
        }
    }

    pub(in crate::layout) fn negative(self) -> &'static str {
        match self {
            Self::SimplifiedInformal | Self::SimplifiedFormal => "负",
            Self::TraditionalInformal | Self::TraditionalFormal => "負",
        }
    }

    pub(in crate::layout) fn is_informal(self) -> bool {
        matches!(self, Self::SimplifiedInformal | Self::TraditionalInformal)
    }
}

/// Render CSS Counter Styles Level 3 Chinese longhand predefined styles.
///
/// The spec defines these styles as special algorithms rather than ordinary
/// `@counter-style` rules:
/// <https://www.w3.org/TR/css-counter-styles-3/#limited-chinese>.
pub(in crate::layout) fn chinese_longhand_marker(
    ordinal: i32,
    style: ChineseLonghandStyle,
) -> Option<String> {
    if !(-9999..=9999).contains(&ordinal) {
        return Some(numeric_marker_i32(ordinal, CJK_DECIMAL_DIGITS));
    }
    if ordinal == 0 {
        return Some(style.digits()[0].to_string());
    }

    let mut places = std::iter::successors(Some(ordinal.abs()), |value| Some(value / 10))
        .take(4)
        .enumerate()
        .map(|(place, value)| (value % 10, place))
        .collect::<Vec<_>>();
    while matches!(places.last(), Some((0, _))) {
        places.pop();
    }

    let digits = style.digits();
    let markers = style.markers();
    let mut output = String::new();
    let mut pending_zero = false;
    for &(digit, place) in places.iter().rev() {
        if digit == 0 {
            pending_zero = true;
            continue;
        }
        if pending_zero && !output.is_empty() {
            output.push_str(digits[0]);
        }
        pending_zero = false;
        if !(style.is_informal() && ordinal.abs() < 20 && place == 1 && digit == 1) {
            output.push_str(digits[digit as usize]);
        }
        output.push_str(markers[place]);
    }

    if ordinal < 0 {
        output = format!("{}{output}", style.negative());
    }
    Some(output)
}

/// Render CSS Counter Styles Level 3 `ethiopic-numeric`.
///
/// <https://www.w3.org/TR/css-counter-styles-3/#ethiopic-numeric-counter-style>
pub(in crate::layout) fn ethiopic_numeric_marker(ordinal: i32) -> Option<String> {
    if ordinal <= 0 {
        return Some(ordinal.to_string());
    }
    if ordinal == 1 {
        return Some("፩".to_string());
    }

    let mut groups = Vec::new();
    let mut value = ordinal;
    while value > 0 {
        groups.push(value % 100);
        value /= 100;
    }

    let mut output = String::new();
    for index in (0..groups.len()).rev() {
        let group = groups[index];
        let odd_index = index % 2 == 1;
        let most_significant = index + 1 == groups.len();
        if group != 0 && !(most_significant && group == 1) && !(odd_index && group == 1) {
            output.push_str(&ethiopic_group_text(group));
        }
        if odd_index && group != 0 {
            output.push('፻');
        } else if index != 0 && !odd_index {
            output.push('፼');
        }
    }
    Some(output)
}

pub(in crate::layout) const CJK_DECIMAL_DIGITS: &[&str; 10] =
    &["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

pub(in crate::layout) fn numeric_marker_i32(index: i32, digits: &[&str; 10]) -> String {
    let sign = if index < 0 { "-" } else { "" };
    let value = i64::from(index).abs().to_string();
    let mut output = String::from(sign);
    for digit in value.bytes() {
        output.push_str(digits[(digit - b'0') as usize]);
    }
    output
}

pub(in crate::layout) fn ethiopic_group_text(group: i32) -> String {
    const TENS: [&str; 10] = ["", "፲", "፳", "፴", "፵", "፶", "፷", "፸", "፹", "፺"];
    const UNITS: [&str; 10] = ["", "፩", "፪", "፫", "፬", "፭", "፮", "፯", "፰", "፱"];

    let tens = (group / 10) as usize;
    let units = (group % 10) as usize;
    format!("{}{}", TENS[tens], UNITS[units])
}

#[cfg(test)]
mod tests;
