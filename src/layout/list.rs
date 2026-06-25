use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(super) fn push_list_context(&mut self, element: &Element, style: &ComputedStyle) -> bool {
        let Some(state) = list_context_for_element(element, style) else {
            return false;
        };
        self.list_stack.push(state);
        true
    }

    pub(super) fn marker_for_list_item(
        &mut self,
        _element: &Element,
        style: &ComputedStyle,
        parent_direction: Direction,
    ) -> Option<ListMarker> {
        if !style.display.is_list_item() {
            return None;
        }

        // CSS Lists 3 defines the marker box and `list-style-type` marker
        // string; HTML's `start`, `reversed`, and `value` attributes seed the
        // same ordinal counter for `ol`/`li`.
        // https://www.w3.org/TR/css-lists-3/#markers
        // https://html.spec.whatwg.org/multipage/grouping-content.html#the-ol-element
        let ordinal = self
            .counter_set
            .current(LIST_ITEM_COUNTER_NAME)
            .unwrap_or_default();

        let marker_style = style
            .marker_style
            .as_deref()
            .cloned()
            .unwrap_or_else(|| style.clone());
        // CSS Lists 3: for automatic markers, `list-style-image` is tried
        // before falling back to the textual `list-style-type`.
        // Explicit `::marker { content: ... }` bypasses automatic markers.
        let image = self
            .marker_image_for_style(style)
            .filter(|_| marker_style.marker_content == MarkerContent::Auto);
        let (text, suffix_space) = if image.is_some() {
            (String::new(), true)
        } else {
            marker_text(
                &marker_style,
                ordinal,
                &self.counter_styles,
                self.counter_set.stacks(),
            )?
        };
        Some(ListMarker {
            text,
            image,
            style: marker_style,
            position: style.list_style_position,
            positioning_direction: match style.marker_side {
                MarkerSide::MatchSelf => style.direction,
                MarkerSide::MatchParent => parent_direction,
            },
            suffix_space,
        })
    }

    pub(super) fn paint_outside_marker(
        &mut self,
        marker: &ListMarker,
        style: &ComputedStyle,
        content_inline_start: f32,
        content_inline_end: f32,
        row_top: f32,
    ) {
        if marker.position != ListStylePosition::Outside
            || style.visibility != Visibility::Visible
            || (marker.text.is_empty() && marker.image.is_none())
        {
            return;
        }
        if let Some(image) = &marker.image {
            let gap = self.marker_gap_width(&marker.style);
            let x = match marker.positioning_direction {
                Direction::Ltr => content_inline_start - image.width - gap,
                Direction::Rtl => content_inline_end + gap,
            };
            self.push_image(RenderedImage {
                background: false,
                x,
                y: row_top - image.height,
                width: image.width,
                height: image.height,
                pixel_width: image.decoded.pixel_width,
                pixel_height: image.decoded.pixel_height,
                source_rect: None,
                interpolate: false,
                rgb: image.decoded.rgb.clone(),
                alpha: image.decoded.alpha.clone(),
                alt_text: None,
            });
            return;
        }
        let marker_width = self.font_system.measure_text(&marker.text, &marker.style);
        let gap = self.marker_gap_width(&marker.style);
        let x = match marker.positioning_direction {
            Direction::Ltr => content_inline_start - marker_width - gap,
            Direction::Rtl => content_inline_end + gap,
        };
        self.paint_text_runs(
            &marker.text,
            x,
            row_top - marker.style.font_size,
            &marker.style,
        );
    }

    pub(super) fn marker_gap_width(&mut self, style: &ComputedStyle) -> f32 {
        self.inline_space_width(style).max(style.font_size * 0.5)
    }

    pub(super) fn push_inside_marker_items(
        &mut self,
        marker: &ListMarker,
        _block_style: &ComputedStyle,
        link_target: Option<String>,
        items: &mut Vec<InlineItem>,
    ) {
        let marker_scope_style = marker_inline_scope_style(&marker.style);
        self.push_bidi_scope_start(&marker_scope_style, link_target.clone(), 0.0, items);
        if let Some(image) = &marker.image {
            items.push(InlineItem::Atom(Box::new(InlineAtom {
                content: InlineAtomContent::Image(image.decoded.clone()),
                style: marker.style.clone(),
                width: image.width,
                height: image.height + inline_replaced_descent(&marker.style),
                baseline_offset: image.height,
                baseline_shift: 0.0,
                link_target: link_target.clone(),
                alt_text: None,
            })));
        } else if !marker.text.is_empty() {
            items.push(InlineItem::Word(Box::new(InlineWord {
                text: marker.text.clone(),
                style: marker.style.clone(),
                baseline_shift: 0.0,
                link_target: link_target.clone(),
                mergeable: false,
                hanging_edges: InlineHangingEdges::default(),
            })));
        }
        if marker.suffix_space {
            self.push_collapsed_inline_space(&marker.style, link_target.clone(), 0.0, items);
        }
        self.push_bidi_scope_end(&marker_scope_style, link_target, 0.0, items);
    }

    fn marker_image_for_style(&self, style: &ComputedStyle) -> Option<MarkerImage> {
        let src = style.list_style_image.as_ref()?;
        let decoded = load_image_source(
            src,
            style.list_style_image_base_url.as_deref().or(self.base_url),
            style.list_style_image_root_url.as_deref(),
            self.resource_cache,
        )?;
        let intrinsic_width = decoded.pixel_width as f32;
        let intrinsic_height = decoded.pixel_height as f32;
        if intrinsic_width <= 0.0 || intrinsic_height <= 0.0 {
            return None;
        }
        Some(MarkerImage {
            decoded,
            width: intrinsic_width,
            height: intrinsic_height,
        })
    }
}

fn marker_inline_scope_style(style: &ComputedStyle) -> ComputedStyle {
    let mut style = style.clone();
    style.display = Display::INLINE;
    style
}

fn list_context_for_element(element: &Element, _style: &ComputedStyle) -> Option<ListState> {
    match list_container_kind(element) {
        ListContainerKind::Ordered => {
            let reversed = element.attrs.contains_key("reversed");
            let step = if reversed { -1 } else { 1 };
            Some(ListState { step })
        }
        ListContainerKind::Unordered | ListContainerKind::Other => Some(ListState { step: 1 }),
    }
}

fn ordered_list_start(element: &Element) -> Option<i32> {
    element
        .attrs
        .get("start")
        .and_then(|value| value.trim().parse::<i32>().ok())
}

pub(super) fn list_item_value(element: &Element) -> Option<i32> {
    if !is_list_item_element(element) {
        return None;
    }
    element
        .attrs
        .get("value")
        .and_then(|value| value.trim().parse::<i32>().ok())
}

fn direct_li_child_count(element: &Element) -> i32 {
    element
        .children
        .iter()
        .filter(|child| {
            matches!(
                &child.kind,
                NodeKind::Element(child_element) if is_list_item_element(child_element)
            )
        })
        .count()
        .try_into()
        .unwrap_or(i32::MAX)
}

pub(super) fn list_container_counter_reset(element: &Element) -> Option<(i32, bool)> {
    match list_container_kind(element) {
        ListContainerKind::Ordered => {
            let reversed = element.attrs.contains_key("reversed");
            let step = if reversed { -1 } else { 1 };
            let start = ordered_list_start(element).unwrap_or_else(|| {
                if reversed {
                    direct_li_child_count(element).max(1)
                } else {
                    1
                }
            });
            Some((
                start - step,
                reversed || element.attrs.contains_key("start"),
            ))
        }
        ListContainerKind::Unordered => Some((0, false)),
        ListContainerKind::Other => None,
    }
}

fn marker_text(
    style: &ComputedStyle,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    counter_stack: &HashMap<String, Vec<i32>>,
) -> Option<(String, bool)> {
    match &style.marker_content {
        MarkerContent::Auto => {
            automatic_marker_text(style.list_style_type.clone(), ordinal, counter_styles)
        }
        MarkerContent::None => None,
        MarkerContent::Parts(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    MarkerContentPart::Text(part) => text.push_str(part),
                    MarkerContentPart::Counter {
                        name,
                        style: counter_style,
                    } => {
                        let value = if name.as_str() == LIST_ITEM_COUNTER_NAME {
                            ordinal
                        } else {
                            counter_stack
                                .get(name)
                                .and_then(|values| values.last().copied())
                                .unwrap_or(0)
                        };
                        if let Some(counter) = counter_text(
                            counter_style.clone().unwrap_or(ListStyleType::Decimal),
                            value,
                            counter_styles,
                        ) {
                            text.push_str(&counter);
                        }
                    }
                    MarkerContentPart::Counters {
                        name,
                        separator,
                        style: counter_style,
                    } => {
                        let values = counter_stack.get(name).cloned().unwrap_or_else(|| vec![0]);
                        let style = counter_style.clone().unwrap_or(ListStyleType::Decimal);
                        let counters = values
                            .into_iter()
                            .filter_map(|value| counter_text(style.clone(), value, counter_styles))
                            .collect::<Vec<_>>();
                        if !counters.is_empty() {
                            text.push_str(&counters.join(separator));
                        }
                    }
                }
            }
            (!text.is_empty()).then_some((text, false))
        }
    }
}

fn automatic_marker_text(
    list_style_type: ListStyleType,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<(String, bool)> {
    if let ListStyleType::Named(name) = &list_style_type
        && let Some(rule) = counter_styles.get(name)
    {
        return custom_counter_marker_text(rule, ordinal, counter_styles);
    }
    if let ListStyleType::Named(name) = &list_style_type
        && let Some((representation, suffix)) = predefined_named_counter_text(name, ordinal)
    {
        return Some((format!("{representation}{suffix}"), suffix == " "));
    }
    if let ListStyleType::Anonymous(rule) = &list_style_type {
        return custom_counter_marker_text(rule, ordinal, counter_styles);
    }
    let representation = counter_text(list_style_type.clone(), ordinal, counter_styles)?;
    match list_style_type {
        ListStyleType::Disc
        | ListStyleType::Circle
        | ListStyleType::Square
        | ListStyleType::DisclosureOpen
        | ListStyleType::DisclosureClosed
        | ListStyleType::Anonymous(_) => Some((representation, true)),
        ListStyleType::String(_) => Some((representation, false)),
        ListStyleType::Numeric(NumericCounterStyle::CjkDecimal)
        | ListStyleType::Hiragana
        | ListStyleType::HiraganaIroha
        | ListStyleType::Katakana
        | ListStyleType::KatakanaIroha
        | ListStyleType::CjkEarthlyBranch
        | ListStyleType::CjkHeavenlyStem => Some((format!("{representation}、"), false)),
        ListStyleType::Decimal
        | ListStyleType::DecimalLeadingZero
        | ListStyleType::Numeric(_)
        | ListStyleType::Additive(_)
        | ListStyleType::LowerAlpha
        | ListStyleType::UpperAlpha
        | ListStyleType::LowerGreek
        | ListStyleType::LowerRoman
        | ListStyleType::UpperRoman
        | ListStyleType::Named(_) => Some((format!("{representation}."), true)),
        ListStyleType::None => None,
    }
}

pub(super) fn counter_text(
    list_style_type: ListStyleType,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<String> {
    match list_style_type {
        ListStyleType::Disc => Some("\u{2022}".to_string()),
        ListStyleType::Circle => Some("\u{25e6}".to_string()),
        ListStyleType::Square => Some("\u{25aa}".to_string()),
        ListStyleType::DisclosureOpen => Some("\u{25be}".to_string()),
        ListStyleType::DisclosureClosed => Some("\u{25b8}".to_string()),
        ListStyleType::Decimal => Some(ordinal.to_string()),
        ListStyleType::DecimalLeadingZero => Some(decimal_leading_zero_marker(ordinal)),
        ListStyleType::Numeric(style) => Some(numeric_marker_i32(ordinal, numeric_digits(style))),
        ListStyleType::Additive(style) => Some(additive_marker_i32(
            ordinal,
            additive_symbols(style),
            additive_range(style),
        )),
        ListStyleType::LowerAlpha => Some(alpha_marker_i32(ordinal, false)),
        ListStyleType::UpperAlpha => Some(alpha_marker_i32(ordinal, true)),
        ListStyleType::LowerGreek => Some(alphabetic_marker_i32(ordinal, LOWER_GREEK_SYMBOLS)),
        ListStyleType::Hiragana => Some(alphabetic_marker_i32(ordinal, HIRAGANA_SYMBOLS)),
        ListStyleType::HiraganaIroha => {
            Some(alphabetic_marker_i32(ordinal, HIRAGANA_IROHA_SYMBOLS))
        }
        ListStyleType::Katakana => Some(alphabetic_marker_i32(ordinal, KATAKANA_SYMBOLS)),
        ListStyleType::KatakanaIroha => {
            Some(alphabetic_marker_i32(ordinal, KATAKANA_IROHA_SYMBOLS))
        }
        ListStyleType::CjkEarthlyBranch => Some(fixed_marker_i32(ordinal, CJK_EARTHLY_BRANCH)),
        ListStyleType::CjkHeavenlyStem => Some(fixed_marker_i32(ordinal, CJK_HEAVENLY_STEM)),
        ListStyleType::LowerRoman => Some(roman_marker_i32(ordinal, false)),
        ListStyleType::UpperRoman => Some(roman_marker_i32(ordinal, true)),
        ListStyleType::String(text) => Some(text),
        ListStyleType::Anonymous(rule) => custom_counter_text(&rule, ordinal, counter_styles),
        ListStyleType::Named(name) => counter_styles
            .get(&name)
            .and_then(|rule| custom_counter_text(rule, ordinal, counter_styles))
            .or_else(|| predefined_named_counter_text(&name, ordinal).map(|(text, _)| text))
            .or_else(|| Some(ordinal.to_string())),
        ListStyleType::None => None,
    }
}

fn custom_counter_marker_text(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<(String, bool)> {
    let effective = resolve_counter_style(rule, counter_styles, 0);
    custom_counter_text_with_effective(&effective, ordinal, counter_styles, 0).map(|text| {
        (
            format!("{}{}{}", effective.prefix, text, effective.suffix),
            false,
        )
    })
}

fn custom_counter_text(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<String> {
    let effective = resolve_counter_style(rule, counter_styles, 0);
    custom_counter_text_with_effective(&effective, ordinal, counter_styles, 0)
}

fn custom_counter_text_with_effective(
    style: &EffectiveCounterStyle,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    depth: usize,
) -> Option<String> {
    if depth > 8 {
        return Some(ordinal.to_string());
    }
    if !counter_style_range_contains(&style.range, &style.system, ordinal) {
        return fallback_counter_text(&style.fallback, ordinal, counter_styles, depth + 1);
    }

    let absolute_ordinal = if ordinal < 0 {
        i32::try_from(i64::from(ordinal).abs()).ok()?
    } else {
        ordinal
    };
    let mut text = match style.system {
        CounterStyleSystem::Cyclic => cyclic_counter_text(absolute_ordinal, &style.symbols),
        CounterStyleSystem::Numeric => numeric_counter_text(absolute_ordinal, &style.symbols),
        CounterStyleSystem::Alphabetic => alphabetic_counter_text(absolute_ordinal, &style.symbols),
        CounterStyleSystem::Symbolic => symbolic_counter_text(absolute_ordinal, &style.symbols),
        CounterStyleSystem::Fixed(first) => fixed_counter_text(ordinal, first, &style.symbols),
        CounterStyleSystem::Additive => {
            additive_counter_text(absolute_ordinal, &style.additive_symbols)
        }
        CounterStyleSystem::Extends(_) => None,
    }
    .or_else(|| fallback_counter_text(&style.fallback, ordinal, counter_styles, depth + 1))?;
    if let Some((width, symbol)) = &style.pad {
        let text_len = text.chars().count();
        if text_len < *width {
            text = format!("{}{}", symbol.repeat(*width - text_len), text);
        }
    }
    if ordinal < 0 {
        text = format!("{}{}{}", style.negative.0, text, style.negative.1);
    }
    Some(text)
}

fn fallback_counter_text(
    fallback: &str,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    depth: usize,
) -> Option<String> {
    if let Some(rule) = counter_styles.get(fallback) {
        let effective = resolve_counter_style(rule, counter_styles, depth);
        return custom_counter_text_with_effective(&effective, ordinal, counter_styles, depth);
    }
    let style = css::parse_list_style_type(fallback).unwrap_or(ListStyleType::Decimal);
    match style {
        ListStyleType::Named(name) if name == fallback => Some(ordinal.to_string()),
        other => counter_text(other, ordinal, counter_styles),
    }
}

#[derive(Debug, Clone)]
struct EffectiveCounterStyle {
    system: CounterStyleSystem,
    symbols: Vec<String>,
    additive_symbols: Vec<(i32, String)>,
    prefix: String,
    suffix: String,
    negative: (String, String),
    pad: Option<(usize, String)>,
    range: CounterStyleRange,
    fallback: String,
}

fn resolve_counter_style(
    rule: &CounterStyleRule,
    counter_styles: &HashMap<String, CounterStyleRule>,
    depth: usize,
) -> EffectiveCounterStyle {
    let inherited = if let CounterStyleSystem::Extends(name) = &rule.system
        && depth <= 8
    {
        counter_styles
            .get(name)
            .map(|rule| resolve_counter_style(rule, counter_styles, depth + 1))
    } else {
        None
    };
    let default = || EffectiveCounterStyle {
        system: CounterStyleSystem::Numeric,
        symbols: decimal_counter_symbols(),
        additive_symbols: Vec::new(),
        prefix: String::new(),
        suffix: ". ".to_string(),
        negative: ("-".to_string(), String::new()),
        pad: None,
        range: CounterStyleRange::Auto,
        fallback: "decimal".to_string(),
    };
    let mut effective = inherited.unwrap_or_else(default);
    if !matches!(rule.system, CounterStyleSystem::Extends(_)) {
        effective.system = rule.system.clone();
        effective.symbols = rule.symbols.clone();
        effective.additive_symbols = rule.additive_symbols.clone();
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
    effective
}

fn counter_style_range_contains(
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

fn decimal_counter_symbols() -> Vec<String> {
    (0..=9).map(|digit| digit.to_string()).collect()
}

fn cyclic_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    let count = i32::try_from(symbols.len()).ok()?;
    if count == 0 {
        return None;
    }
    let position = (index - 1).rem_euclid(count);
    symbols.get(position as usize).cloned()
}

fn fixed_counter_text(index: i32, first: i32, symbols: &[String]) -> Option<String> {
    let offset = index.checked_sub(first)?;
    let offset = usize::try_from(offset).ok()?;
    symbols.get(offset).cloned()
}

fn symbolic_counter_text(index: i32, symbols: &[String]) -> Option<String> {
    if index <= 0 || symbols.is_empty() {
        return None;
    }
    let count = i32::try_from(symbols.len()).ok()?;
    let symbol = symbols.get(((index - 1) % count) as usize)?;
    let repetitions = ((index + count - 1) / count) as usize;
    Some(symbol.repeat(repetitions))
}

fn alphabetic_counter_text(index: i32, symbols: &[String]) -> Option<String> {
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
    Some(output.iter().rev().copied().collect::<String>())
}

fn numeric_counter_text(index: i32, symbols: &[String]) -> Option<String> {
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
        output.iter().rev().copied().collect::<String>()
    ))
}

fn additive_counter_text(index: i32, symbols: &[(i32, String)]) -> Option<String> {
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

fn predefined_named_counter_text(name: &str, ordinal: i32) -> Option<(String, &'static str)> {
    match name {
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

#[derive(Debug, Clone, Copy)]
enum ChineseLonghandStyle {
    SimplifiedInformal,
    SimplifiedFormal,
    TraditionalInformal,
    TraditionalFormal,
}

impl ChineseLonghandStyle {
    fn digits(self) -> &'static [&'static str; 10] {
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

    fn markers(self) -> &'static [&'static str; 4] {
        match self {
            Self::SimplifiedInformal | Self::TraditionalInformal => &["", "十", "百", "千"],
            Self::SimplifiedFormal => &["", "拾", "佰", "仟"],
            Self::TraditionalFormal => &["", "拾", "佰", "仟"],
        }
    }

    fn negative(self) -> &'static str {
        match self {
            Self::SimplifiedInformal | Self::SimplifiedFormal => "负",
            Self::TraditionalInformal | Self::TraditionalFormal => "負",
        }
    }

    fn is_informal(self) -> bool {
        matches!(self, Self::SimplifiedInformal | Self::TraditionalInformal)
    }
}

/// Render CSS Counter Styles Level 3 Chinese longhand predefined styles.
///
/// The spec defines these styles as special algorithms rather than ordinary
/// `@counter-style` rules:
/// <https://www.w3.org/TR/css-counter-styles-3/#limited-chinese>.
fn chinese_longhand_marker(ordinal: i32, style: ChineseLonghandStyle) -> Option<String> {
    if !(-9999..=9999).contains(&ordinal) {
        return Some(numeric_marker_i32(
            ordinal,
            numeric_digits(NumericCounterStyle::CjkDecimal),
        ));
    }
    if ordinal == 0 {
        return Some(style.digits()[0].to_string());
    }

    let mut value = ordinal.abs();
    let mut places = Vec::new();
    for place in 0..4 {
        places.push((value % 10, place));
        value /= 10;
    }
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
fn ethiopic_numeric_marker(ordinal: i32) -> Option<String> {
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
            output.push_str(ethiopic_group_text(group));
        }
        if odd_index && group != 0 {
            output.push('፻');
        } else if index != 0 && !odd_index {
            output.push('፼');
        }
    }
    Some(output)
}

fn ethiopic_group_text(group: i32) -> &'static str {
    const TENS: [&str; 10] = ["", "", "፳", "፴", "፵", "፶", "፷", "፸", "፹", "፺"];
    const UNITS: [&str; 10] = ["", "፩", "፪", "፫", "፬", "፭", "፮", "፯", "፰", "፱"];
    match (group / 10, group % 10) {
        (0, unit) => UNITS[unit as usize],
        (1, unit) => match unit {
            0 => "፲",
            1 => "፲፩",
            2 => "፲፪",
            3 => "፲፫",
            4 => "፲፬",
            5 => "፲፭",
            6 => "፲፮",
            7 => "፲፯",
            8 => "፲፰",
            9 => "፲፱",
            _ => "",
        },
        (ten, 0) => TENS[ten as usize],
        (ten, unit) => match (TENS[ten as usize], UNITS[unit as usize]) {
            ("፳", "፩") => "፳፩",
            ("፳", "፪") => "፳፪",
            ("፳", "፫") => "፳፫",
            ("፳", "፬") => "፳፬",
            ("፳", "፭") => "፳፭",
            ("፳", "፮") => "፳፮",
            ("፳", "፯") => "፳፯",
            ("፳", "፰") => "፳፰",
            ("፳", "፱") => "፳፱",
            ("፴", "፩") => "፴፩",
            ("፴", "፪") => "፴፪",
            ("፴", "፫") => "፴፫",
            ("፴", "፬") => "፴፬",
            ("፴", "፭") => "፴፭",
            ("፴", "፮") => "፴፮",
            ("፴", "፯") => "፴፯",
            ("፴", "፰") => "፴፰",
            ("፴", "፱") => "፴፱",
            ("፵", "፩") => "፵፩",
            ("፵", "፪") => "፵፪",
            ("፵", "፫") => "፵፫",
            ("፵", "፬") => "፵፬",
            ("፵", "፭") => "፵፭",
            ("፵", "፮") => "፵፮",
            ("፵", "፯") => "፵፯",
            ("፵", "፰") => "፵፰",
            ("፵", "፱") => "፵፱",
            ("፶", "፩") => "፶፩",
            ("፶", "፪") => "፶፪",
            ("፶", "፫") => "፶፫",
            ("፶", "፬") => "፶፬",
            ("፶", "፭") => "፶፭",
            ("፶", "፮") => "፶፮",
            ("፶", "፯") => "፶፯",
            ("፶", "፰") => "፶፰",
            ("፶", "፱") => "፶፱",
            ("፷", "፩") => "፷፩",
            ("፷", "፪") => "፷፪",
            ("፷", "፫") => "፷፫",
            ("፷", "፬") => "፷፬",
            ("፷", "፭") => "፷፭",
            ("፷", "፮") => "፷፮",
            ("፷", "፯") => "፷፯",
            ("፷", "፰") => "፷፰",
            ("፷", "፱") => "፷፱",
            ("፸", "፩") => "፸፩",
            ("፸", "፪") => "፸፪",
            ("፸", "፫") => "፸፫",
            ("፸", "፬") => "፸፬",
            ("፸", "፭") => "፸፭",
            ("፸", "፮") => "፸፮",
            ("፸", "፯") => "፸፯",
            ("፸", "፰") => "፸፰",
            ("፸", "፱") => "፸፱",
            ("፹", "፩") => "፹፩",
            ("፹", "፪") => "፹፪",
            ("፹", "፫") => "፹፫",
            ("፹", "፬") => "፹፬",
            ("፹", "፭") => "፹፭",
            ("፹", "፮") => "፹፮",
            ("፹", "፯") => "፹፯",
            ("፹", "፰") => "፹፰",
            ("፹", "፱") => "፹፱",
            ("፺", "፩") => "፺፩",
            ("፺", "፪") => "፺፪",
            ("፺", "፫") => "፺፫",
            ("፺", "፬") => "፺፬",
            ("፺", "፭") => "፺፭",
            ("፺", "፮") => "፺፮",
            ("፺", "፯") => "፺፯",
            ("፺", "፰") => "፺፰",
            ("፺", "፱") => "፺፱",
            _ => "",
        },
    }
}

fn decimal_leading_zero_marker(index: i32) -> String {
    let sign = if index < 0 { "-" } else { "" };
    let value = i64::from(index).abs().to_string();
    if value.chars().count() >= 2 {
        format!("{sign}{value}")
    } else {
        format!("{sign}0{value}")
    }
}

fn numeric_marker_i32(index: i32, digits: &[&str; 10]) -> String {
    let sign = if index < 0 { "-" } else { "" };
    let value = i64::from(index).abs().to_string();
    let mut output = String::new();
    output.push_str(sign);
    for digit in value.bytes() {
        let index = (digit - b'0') as usize;
        output.push_str(digits[index]);
    }
    output
}

fn alpha_marker_i32(index: i32, uppercase: bool) -> String {
    if index <= 0 {
        return index.to_string();
    }
    alpha_marker(index as usize, uppercase)
}

fn alphabetic_marker_i32(index: i32, symbols: &[&str]) -> String {
    if index <= 0 {
        return index.to_string();
    }
    let base = symbols.len();
    let mut value = index as usize;
    let mut output = Vec::new();
    while value > 0 {
        value -= 1;
        output.push(symbols[value % base]);
        value /= base;
    }
    output.iter().rev().copied().collect::<String>()
}

fn fixed_marker_i32(index: i32, symbols: &[&str]) -> String {
    if index <= 0 {
        return index.to_string();
    }
    let Ok(index) = usize::try_from(index) else {
        return index.to_string();
    };
    symbols
        .get(index - 1)
        .map(|symbol| (*symbol).to_string())
        .unwrap_or_else(|| index.to_string())
}

fn additive_marker_i32(index: i32, symbols: &[(i32, &str)], range: (i32, i32)) -> String {
    if index < range.0 || index > range.1 {
        return index.to_string();
    }
    let mut value = index;
    let mut output = String::new();
    for (weight, symbol) in symbols {
        while value >= *weight {
            output.push_str(symbol);
            value -= *weight;
        }
    }
    if value == 0 {
        output
    } else {
        index.to_string()
    }
}

fn roman_marker_i32(index: i32, uppercase: bool) -> String {
    if !(1..=3999).contains(&index) {
        return index.to_string();
    }
    let mut value = index;
    let mut output = String::new();
    for (number, numeral) in [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ] {
        while value >= number {
            output.push_str(numeral);
            value -= number;
        }
    }
    if uppercase {
        output.to_uppercase()
    } else {
        output
    }
}

fn numeric_digits(style: NumericCounterStyle) -> &'static [&'static str; 10] {
    match style {
        NumericCounterStyle::ArabicIndic => &["٠", "١", "٢", "٣", "٤", "٥", "٦", "٧", "٨", "٩"],
        NumericCounterStyle::Bengali => &["০", "১", "২", "৩", "৪", "৫", "৬", "৭", "৮", "৯"],
        NumericCounterStyle::Cambodian => &["០", "១", "២", "៣", "៤", "៥", "៦", "៧", "៨", "៩"],
        NumericCounterStyle::CjkDecimal => {
            &["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"]
        }
        NumericCounterStyle::Devanagari => &["०", "१", "२", "३", "४", "५", "६", "७", "८", "९"],
        NumericCounterStyle::Gujarati => &["૦", "૧", "૨", "૩", "૪", "૫", "૬", "૭", "૮", "૯"],
        NumericCounterStyle::Gurmukhi => &["੦", "੧", "੨", "੩", "੪", "੫", "੬", "੭", "੮", "੯"],
        NumericCounterStyle::Kannada => &["೦", "೧", "೨", "೩", "೪", "೫", "೬", "೭", "೮", "೯"],
        NumericCounterStyle::Lao => &["໐", "໑", "໒", "໓", "໔", "໕", "໖", "໗", "໘", "໙"],
        NumericCounterStyle::Malayalam => &["൦", "൧", "൨", "൩", "൪", "൫", "൬", "൭", "൮", "൯"],
        NumericCounterStyle::Mongolian => &["᠐", "᠑", "᠒", "᠓", "᠔", "᠕", "᠖", "᠗", "᠘", "᠙"],
        NumericCounterStyle::Myanmar => &["၀", "၁", "၂", "၃", "၄", "၅", "၆", "၇", "၈", "၉"],
        NumericCounterStyle::Oriya => &["୦", "୧", "୨", "୩", "୪", "୫", "୬", "୭", "୮", "୯"],
        NumericCounterStyle::Persian => &["۰", "۱", "۲", "۳", "۴", "۵", "۶", "۷", "۸", "۹"],
        NumericCounterStyle::Tamil => &["௦", "௧", "௨", "௩", "௪", "௫", "௬", "௭", "௮", "௯"],
        NumericCounterStyle::Telugu => &["౦", "౧", "౨", "౩", "౪", "౫", "౬", "౭", "౮", "౯"],
        NumericCounterStyle::Thai => &["๐", "๑", "๒", "๓", "๔", "๕", "๖", "๗", "๘", "๙"],
        NumericCounterStyle::Tibetan => &["༠", "༡", "༢", "༣", "༤", "༥", "༦", "༧", "༨", "༩"],
    }
}

fn additive_symbols(style: AdditiveCounterStyle) -> &'static [(i32, &'static str)] {
    match style {
        AdditiveCounterStyle::Armenian => ARMENIAN_ADDITIVE,
        AdditiveCounterStyle::LowerArmenian => LOWER_ARMENIAN_ADDITIVE,
        AdditiveCounterStyle::Georgian => GEORGIAN_ADDITIVE,
        AdditiveCounterStyle::Hebrew => HEBREW_ADDITIVE,
    }
}

fn additive_range(style: AdditiveCounterStyle) -> (i32, i32) {
    match style {
        AdditiveCounterStyle::Armenian | AdditiveCounterStyle::LowerArmenian => (1, 9999),
        AdditiveCounterStyle::Georgian => (1, 19999),
        AdditiveCounterStyle::Hebrew => (1, 10999),
    }
}

const ARMENIAN_ADDITIVE: &[(i32, &str)] = &[
    (9000, "Ք"),
    (8000, "Փ"),
    (7000, "Ւ"),
    (6000, "Ց"),
    (5000, "Ր"),
    (4000, "Տ"),
    (3000, "Վ"),
    (2000, "Ս"),
    (1000, "Ռ"),
    (900, "Ջ"),
    (800, "Պ"),
    (700, "Չ"),
    (600, "Ո"),
    (500, "Շ"),
    (400, "Ն"),
    (300, "Յ"),
    (200, "Մ"),
    (100, "Ճ"),
    (90, "Ղ"),
    (80, "Ձ"),
    (70, "Հ"),
    (60, "Կ"),
    (50, "Ծ"),
    (40, "Խ"),
    (30, "Լ"),
    (20, "Ի"),
    (10, "Ժ"),
    (9, "Թ"),
    (8, "Ը"),
    (7, "Է"),
    (6, "Զ"),
    (5, "Ե"),
    (4, "Դ"),
    (3, "Գ"),
    (2, "Բ"),
    (1, "Ա"),
];
const LOWER_ARMENIAN_ADDITIVE: &[(i32, &str)] = &[
    (9000, "ք"),
    (8000, "փ"),
    (7000, "ւ"),
    (6000, "ց"),
    (5000, "ր"),
    (4000, "տ"),
    (3000, "վ"),
    (2000, "ս"),
    (1000, "ռ"),
    (900, "ջ"),
    (800, "պ"),
    (700, "չ"),
    (600, "ո"),
    (500, "շ"),
    (400, "ն"),
    (300, "յ"),
    (200, "մ"),
    (100, "ճ"),
    (90, "ղ"),
    (80, "ձ"),
    (70, "հ"),
    (60, "կ"),
    (50, "ծ"),
    (40, "խ"),
    (30, "լ"),
    (20, "ի"),
    (10, "ժ"),
    (9, "թ"),
    (8, "ը"),
    (7, "է"),
    (6, "զ"),
    (5, "ե"),
    (4, "դ"),
    (3, "գ"),
    (2, "բ"),
    (1, "ա"),
];
const GEORGIAN_ADDITIVE: &[(i32, &str)] = &[
    (10000, "ჵ"),
    (9000, "ჰ"),
    (8000, "ჯ"),
    (7000, "ჴ"),
    (6000, "ხ"),
    (5000, "ჭ"),
    (4000, "წ"),
    (3000, "ძ"),
    (2000, "ც"),
    (1000, "ჩ"),
    (900, "შ"),
    (800, "ყ"),
    (700, "ღ"),
    (600, "ქ"),
    (500, "ფ"),
    (400, "ჳ"),
    (300, "ტ"),
    (200, "ს"),
    (100, "რ"),
    (90, "ჟ"),
    (80, "პ"),
    (70, "ო"),
    (60, "ჲ"),
    (50, "ნ"),
    (40, "მ"),
    (30, "ლ"),
    (20, "კ"),
    (10, "ი"),
    (9, "თ"),
    (8, "ჱ"),
    (7, "ზ"),
    (6, "ვ"),
    (5, "ე"),
    (4, "დ"),
    (3, "გ"),
    (2, "ბ"),
    (1, "ა"),
];
const HEBREW_ADDITIVE: &[(i32, &str)] = &[
    (10000, "י׳"),
    (9000, "ט׳"),
    (8000, "ח׳"),
    (7000, "ז׳"),
    (6000, "ו׳"),
    (5000, "ה׳"),
    (4000, "ד׳"),
    (3000, "ג׳"),
    (2000, "ב׳"),
    (1000, "א׳"),
    (400, "ת"),
    (300, "ש"),
    (200, "ר"),
    (100, "ק"),
    (90, "צ"),
    (80, "פ"),
    (70, "ע"),
    (60, "ס"),
    (50, "נ"),
    (40, "מ"),
    (30, "ל"),
    (20, "כ"),
    (19, "יט"),
    (18, "יח"),
    (17, "יז"),
    (16, "טז"),
    (15, "טו"),
    (10, "י"),
    (9, "ט"),
    (8, "ח"),
    (7, "ז"),
    (6, "ו"),
    (5, "ה"),
    (4, "ד"),
    (3, "ג"),
    (2, "ב"),
    (1, "א"),
];

const LOWER_GREEK_SYMBOLS: &[&str] = &[
    "α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ", "ν", "ξ", "ο", "π", "ρ", "σ", "τ",
    "υ", "φ", "χ", "ψ", "ω",
];
const HIRAGANA_SYMBOLS: &[&str] = &[
    "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た",
    "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み",
    "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん",
];
const HIRAGANA_IROHA_SYMBOLS: &[&str] = &[
    "い", "ろ", "は", "に", "ほ", "へ", "と", "ち", "り", "ぬ", "る", "を", "わ", "か", "よ", "た",
    "れ", "そ", "つ", "ね", "な", "ら", "む", "う", "ゐ", "の", "お", "く", "や", "ま", "け", "ふ",
    "こ", "え", "て", "あ", "さ", "き", "ゆ", "め", "み", "し", "ゑ", "ひ", "も", "せ", "す",
];
const KATAKANA_SYMBOLS: &[&str] = &[
    "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ",
    "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ",
    "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヰ", "ヱ", "ヲ", "ン",
];
const KATAKANA_IROHA_SYMBOLS: &[&str] = &[
    "イ", "ロ", "ハ", "ニ", "ホ", "ヘ", "ト", "チ", "リ", "ヌ", "ル", "ヲ", "ワ", "カ", "ヨ", "タ",
    "レ", "ソ", "ツ", "ネ", "ナ", "ラ", "ム", "ウ", "ヰ", "ノ", "オ", "ク", "ヤ", "マ", "ケ", "フ",
    "コ", "エ", "テ", "ア", "サ", "キ", "ユ", "メ", "ミ", "シ", "ヱ", "ヒ", "モ", "セ", "ス",
];
const CJK_EARTHLY_BRANCH: &[&str] = &[
    "子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥",
];
const CJK_HEAVENLY_STEM: &[&str] = &["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
