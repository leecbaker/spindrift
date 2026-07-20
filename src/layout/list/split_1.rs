use super::*;
use crate::text::is_css_preserved_document_space;
use std::rc::Rc;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn marker_for_list_item(
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
        let marker_style = style
            .marker_style
            .as_deref()
            .cloned()
            .unwrap_or_else(|| style.clone());
        let planned_stacks = self
            .counter_plan
            .values_at_origin
            .get(&CounterOriginKey::new(
                _element,
                box_tree::CounterEventSource::Marker,
            ))
            .or_else(|| {
                self.counter_plan
                    .values_at_origin
                    .get(&CounterOriginKey::new(
                        _element,
                        box_tree::CounterEventSource::Principal,
                    ))
            });
        let ordinal = planned_stacks
            .and_then(|stacks| stacks.get(LIST_ITEM_COUNTER_NAME))
            .and_then(|values| values.last())
            .cloned()
            .or_else(|| self.counter_set.current(LIST_ITEM_COUNTER_NAME))
            .unwrap_or_default();
        // CSS Lists 3: for automatic markers, `list-style-image` is tried
        // before falling back to the textual `list-style-type`.
        // Explicit `::marker { content: ... }` bypasses automatic markers.
        let image = self
            .marker_image_for_style(style)
            .filter(|_| marker_style.marker_content == MarkerContent::Auto);
        let runtime_stacks;
        let counter_stacks = if let Some(planned_stacks) = planned_stacks {
            planned_stacks
        } else {
            runtime_stacks = self.counter_set.stacks();
            &runtime_stacks
        };
        let marker = if image.is_some() {
            // `list-style-image` replaces the counter representation, not
            // the marker's generated separator. An inside marker therefore
            // still contributes the normal following U+0020 to the inline
            // stream, where it remains available for extraction.
            // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
            Some((String::new(), true))
        } else {
            marker_text(
                &marker_style,
                ordinal,
                &self.counter_styles,
                counter_stacks,
                &mut self.quote_depth,
            )
        };
        let (text, suffix_space) = marker?;
        Some(ListMarker {
            text,
            image,
            style: marker_style,
            // A non-atomic inline list item has no block marker gutter, so an
            // `outside` marker participates as `inside`. An inline flow-root
            // is atomic and retains its own outside marker box.
            // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
            position: if style.display.is_inline_level() && !style.display.is_atomic_inline() {
                ListStylePosition::Inside
            } else {
                style.list_style_position
            },
            positioning_direction: match style.marker_side {
                MarkerSide::MatchSelf => style.direction,
                MarkerSide::MatchParent => parent_direction,
            },
            suffix_space,
        })
    }

    pub(in crate::layout) fn paint_outside_marker(
        &mut self,
        marker: &ListMarker,
        style: &ComputedStyle,
        content_inline_start: f32,
        content_inline_end: f32,
        row_top: f32,
    ) {
        if !marker.paints_outside()
            || style.visibility != Visibility::Visible
            || (marker.text.is_empty() && marker.image.is_none())
        {
            return;
        }
        if let Some(image) = &marker.image {
            let gap = if marker.suffix_space {
                self.marker_gap_width(&marker.style).points()
            } else {
                0.0
            };
            let x = match marker.positioning_direction {
                Direction::Ltr => content_inline_start - image.width - gap,
                Direction::Rtl => content_inline_end + gap,
            };
            let rect = PageTopRect::new(x, row_top, image.width, image.height).paint_rect();
            if let Some(asset) = &image.svg {
                for path in asset.paint_paths(rect) {
                    self.push_path_in_band(PaintBand::Inline, path);
                }
            } else {
                self.push_image(
                    RenderedImage::from_paint_rect(
                        rect,
                        false,
                        image.decoded.pixel_width,
                        image.decoded.pixel_height,
                        None,
                        false,
                        image.decoded.rgb.shared(),
                        image.decoded.alpha.clone(),
                        None,
                    )
                    .with_raster_color_space(image.decoded.color_space.clone())
                    .with_image_id(image.decoded.image_id),
                );
            }
            return;
        }
        let mut items = Vec::new();
        self.push_inside_marker_items(marker, style, None, &mut items);
        let measurement =
            self.intrinsic_inline_measurement_for_items(items.clone(), &marker.style, f32::MAX);
        let marker_width = measurement.contribution.max_content.points();
        let sequence = if marker
            .text
            .chars()
            .last()
            .is_some_and(is_css_preserved_document_space)
        {
            measurement.sequence
        } else {
            self.collect_inline_line_sequence_with_text_box_trim(
                items,
                &marker.style,
                marker_width,
                0.0,
                0.0,
            )
        };
        let marker_left = match marker.positioning_direction {
            Direction::Ltr => content_inline_start - marker_width,
            Direction::Rtl => content_inline_end,
        };
        self.paint_inline_box_sequence(
            &sequence,
            &marker.style,
            marker_left,
            marker_width,
            row_top,
        );
    }

    pub(in crate::layout) fn marker_gap_width(&mut self, style: &ComputedStyle) -> LayoutLength {
        // The automatic textual marker suffix ends in U+0020.  Its advance is
        // therefore the selected font's space advance, not a synthesized
        // half-em gutter.
        // <https://drafts.csswg.org/css-counter-styles-3/#generate-a-counter>
        self.inline_space_width(style)
    }

    pub(in crate::layout) fn push_inside_marker_items(
        &mut self,
        marker: &ListMarker,
        _block_style: &ComputedStyle,
        link_target: Option<String>,
        items: &mut Vec<InlineItem>,
    ) {
        let marker_scope_style = marker_inline_scope_style(&marker.style);
        self.push_bidi_scope_start_with_source(
            &marker_scope_style,
            link_target.clone(),
            0.0,
            InlineVisualOffset::zero(),
            InlineTextSource::Marker,
            items,
        );
        if let Some(image) = &marker.image {
            items.push(InlineItem::Atom(Box::new(InlineAtom::new(
                image
                    .svg
                    .clone()
                    .map(|asset| InlineAtomContent::Svg { asset: Some(asset) })
                    .unwrap_or_else(|| InlineAtomContent::Image(image.decoded.clone())),
                marker.style.clone(),
                None,
                // The atom's content box is exactly the marker image. Line
                // layout accounts for baseline descent separately; including
                // it here would stretch the image's painted height.
                InlineSize::new(image.width, image.height),
                image.height,
                0.0,
                link_target.clone(),
                None,
            ))));
        } else if !marker.text.is_empty() {
            items.push(InlineItem::Word(Box::new(InlineWord {
                text: marker.text.clone(),
                style: inline_style(&marker.style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: link_target.clone().map(Rc::from),
                mergeable: true,
                source: InlineTextSource::Marker,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new().into(),
            })));
        }
        if marker.suffix_space {
            items.push(InlineItem::Word(Box::new(InlineWord {
                text: " ".to_string(),
                style: inline_style(&marker.style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: link_target.clone().map(Rc::from),
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new().into(),
            })));
        }
        self.push_bidi_scope_end_with_source(
            &marker_scope_style,
            link_target,
            0.0,
            InlineVisualOffset::zero(),
            InlineTextSource::Marker,
            items,
        );
    }

    pub(in crate::layout) fn marker_image_for_style(
        &self,
        style: &ComputedStyle,
    ) -> Option<MarkerImage> {
        let image = style.list_style_image.as_image()?;
        let intrinsic_resolution = image.intrinsic_resolution().max(f32::MIN_POSITIVE);
        let css::BackgroundImage::Url {
            src,
            base_url,
            root_url,
            request_modifiers,
        } = image.selected_image()
        else {
            // CSS generated images are not yet marker paint sources. They
            // remain an invalid marker image here, allowing list-style-type
            // fallback while that rendering path is implemented.
            return None;
        };
        let asset = load_resolved_image_source_with_request(
            src,
            base_url.as_ref().or(self.base_url),
            root_url.as_ref(),
            self.resource_cache,
            style.image_orientation == css::ImageOrientation::FromImage,
            request_modifiers,
        )?;
        let intrinsic_size = asset.intrinsic_size();
        let width = intrinsic_size.width / intrinsic_resolution;
        let height = intrinsic_size.height / intrinsic_resolution;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let (decoded, svg) = match asset {
            ResolvedImageAsset::Raster(decoded) => (decoded, None),
            ResolvedImageAsset::Svg(svg) => (
                DecodedPngImage::new(1, 1, vec![0, 0, 0], Some(vec![0])),
                Some(svg),
            ),
        };
        Some(MarkerImage {
            decoded,
            svg,
            width,
            height,
        })
    }
}

pub(in crate::layout) fn marker_inline_scope_style(style: &ComputedStyle) -> ComputedStyle {
    let mut style = style.clone();
    style.display = Display::INLINE;
    style
}

pub(in crate::layout) fn marker_text(
    style: &ComputedStyle,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
    counter_stack: &HashMap<String, Vec<i32>>,
    quote_depth: &mut usize,
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
                    MarkerContentPart::Quote(quote) => match quote {
                        GeneratedQuote::Open => {
                            text.push_str(
                                &crate::layout::inline_collect::quote_pair(style, *quote_depth).0,
                            );
                            *quote_depth += 1;
                        }
                        GeneratedQuote::Close => {
                            *quote_depth = quote_depth.saturating_sub(1);
                            text.push_str(
                                &crate::layout::inline_collect::quote_pair(style, *quote_depth).1,
                            );
                        }
                        GeneratedQuote::NoOpen => *quote_depth += 1,
                        GeneratedQuote::NoClose => {
                            *quote_depth = quote_depth.saturating_sub(1);
                        }
                    },
                    MarkerContentPart::Counter {
                        name,
                        style: counter_style,
                    } => {
                        let value = if name.as_str() == LIST_ITEM_COUNTER_NAME {
                            ordinal
                        } else {
                            counter_stack
                                .get(name)
                                .and_then(|values| values.last().cloned())
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

pub(in crate::layout) fn automatic_marker_text(
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
        ListStyleType::Decimal | ListStyleType::Named(_) => {
            Some((format!("{representation}."), true))
        }
        ListStyleType::None => None,
    }
}

pub(in crate::layout) fn counter_text(
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

pub(in crate::layout) fn custom_counter_marker_text(
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

pub(in crate::layout) fn custom_counter_text(
    rule: &CounterStyleRule,
    ordinal: i32,
    counter_styles: &HashMap<String, CounterStyleRule>,
) -> Option<String> {
    let effective = resolve_counter_style(rule, counter_styles, 0);
    custom_counter_text_with_effective(&effective, ordinal, counter_styles, 0)
}

pub(in crate::layout) fn custom_counter_text_with_effective(
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

pub(in crate::layout) fn fallback_counter_text(
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
}

pub(in crate::layout) fn resolve_counter_style(
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

pub(in crate::layout) fn predefined_named_counter_text(
    name: &str,
    ordinal: i32,
) -> Option<(String, &'static str)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ua_counter_styles() -> HashMap<String, CounterStyleRule> {
        crate::css::html5_user_agent_stylesheet()
            .counter_styles
            .into_iter()
            .map(|style| (style.name.clone(), style))
            .collect()
    }

    #[test]
    fn cjk_decimal_honors_its_ua_range_and_fallback() {
        let counter_styles = ua_counter_styles();
        let style = crate::css::parse_list_style_type("cjk-decimal").expect("valid style");

        assert_eq!(style, ListStyleType::Named("cjk-decimal".to_string()));
        assert_eq!(
            counter_text(style.clone(), 12_345, &counter_styles),
            Some("一二三四五".to_string())
        );
        assert_eq!(
            counter_text(style, -1, &counter_styles),
            Some("-1".to_string())
        );
    }
}
