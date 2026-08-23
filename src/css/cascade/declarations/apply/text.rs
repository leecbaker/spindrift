use std::num::NonZeroU32;

use super::*;

pub(in crate::css) fn apply_cascaded_text_declaration(
    style: &mut ComputedStyle,
    name: &str,
    value: &str,
    declaration: &CascadedDeclaration<'_>,
    inheritance_source: &ComputedStyle,
    _parent_ch_advance: LayoutLength,
) -> bool {
    match name {
        "font-family" => {
            style.font_family =
                parse_font_family(value).unwrap_or_else(|| style.font_family.clone());
        }
        "font-language-override" => {
            if let Some(font_language_override) = parse_font_language_override(value) {
                style.font_language_override = font_language_override;
            }
        }
        "font-synthesis" => {
            if let Some(font_synthesis) = parse_font_synthesis(value) {
                style.font_synthesis = font_synthesis;
            }
        }
        "font-synthesis-weight" => {
            if let Some(value) = parse_font_synthesis_subproperty(value) {
                style.font_synthesis.weight = value;
            }
        }
        "font-synthesis-style" => {
            if let Some(value) = parse_font_synthesis_subproperty(value) {
                style.font_synthesis.style = value;
            }
        }
        "font-synthesis-small-caps" => {
            if let Some(value) = parse_font_synthesis_subproperty(value) {
                style.font_synthesis.small_caps = value;
            }
        }
        "font-synthesis-position" => {
            if let Some(value) = parse_font_synthesis_subproperty(value) {
                style.font_synthesis.position = value;
            }
        }
        "font-feature-settings" => {
            if let Some(font_feature_settings) = parse_font_feature_settings(value) {
                style.font_feature_settings = font_feature_settings;
            }
        }
        "font-variation-settings" => {
            if let Some(font_variation_settings) = parse_font_variation_settings(value) {
                style.font_variation_settings = font_variation_settings;
            }
        }
        "font-size-adjust" => {
            if let Some(font_size_adjust) = parse_font_size_adjust(value) {
                style.font_size_adjust = font_size_adjust;
            }
        }
        "font-kerning" => {
            if let Some(font_kerning) = parse_font_kerning(value) {
                style.font_kerning = font_kerning;
            }
        }
        "font-variant" => {
            if let Some(font_variant) = parse_font_variant(value) {
                style.font_variant_ligatures = font_variant.ligatures;
                style.font_variant_position = font_variant.position;
                style.font_variant_caps = font_variant.caps;
                style.font_variant_numeric = font_variant.numeric;
                style.font_variant_alternates = font_variant.alternates;
                style.font_variant_east_asian = font_variant.east_asian;
                style.font_variant_emoji = font_variant.emoji;
            }
        }
        "font-variant-ligatures" => {
            if let Some(font_variant_ligatures) = parse_font_variant_ligatures(value) {
                style.font_variant_ligatures = font_variant_ligatures;
            }
        }
        "font-variant-position" => {
            if let Some(font_variant_position) = parse_font_variant_position(value) {
                style.font_variant_position = font_variant_position;
            }
        }
        "font-variant-caps" => {
            if let Some(font_variant_caps) = parse_font_variant_caps(value) {
                style.font_variant_caps = font_variant_caps;
            }
        }
        "font-variant-numeric" => {
            if let Some(font_variant_numeric) = parse_font_variant_numeric(value) {
                style.font_variant_numeric = font_variant_numeric;
            }
        }
        "font-variant-alternates" => {
            if let Some(font_variant_alternates) = parse_font_variant_alternates(value) {
                style.font_variant_alternates = font_variant_alternates;
            }
        }
        "font-variant-east-asian" => {
            if let Some(font_variant_east_asian) = parse_font_variant_east_asian(value) {
                style.font_variant_east_asian = font_variant_east_asian;
            }
        }
        "font-variant-emoji" => {
            if let Some(font_variant_emoji) = parse_font_variant_emoji(value) {
                style.font_variant_emoji = font_variant_emoji;
            }
        }
        "font-palette" => {
            if let Some(font_palette) = parse_font_palette(value) {
                style.font_palette = font_palette;
            }
        }
        "bookmark-level" => {
            if let Some(level) = parse_bookmark_level(value) {
                style.bookmark_level = match level {
                    Some(level) => BookmarkLevel::Level(
                        NonZeroU32::new(level).expect("bookmark parser rejects zero"),
                    ),
                    None => BookmarkLevel::None,
                };
            }
        }
        "bookmark-label" => {
            if let Some(label) = parse_bookmark_label(value) {
                style.bookmark_label = label;
            }
        }
        "bookmark-state" => {
            if let Some(state) = parse_bookmark_state(value) {
                style.bookmark_state = state;
            }
        }
        "text-transform" => {
            if let Some(transform) = parse_text_transform(value) {
                style.text_transform = transform;
            }
        }
        "tab-size" => {
            if let Some(tab_size) = parse_tab_size(value, style.font_size) {
                style.tab_size = tab_size;
            }
        }
        "visibility" => {
            style.visibility = match value.trim().to_ascii_lowercase().as_str() {
                "hidden" => Visibility::Hidden,
                "collapse" => Visibility::Collapse,
                "visible" => Visibility::Visible,
                _ => style.visibility,
            };
        }
        "list-style" => {
            if let Some(components) = parse_list_style_shorthand(value) {
                if let Some(style_type) = parse_list_style_type(&components.style_type) {
                    style.list_style_type = style_type;
                }
                if let Some(position) = parse_list_style_position(&components.position) {
                    style.list_style_position = position;
                }
                if components.image.eq_ignore_ascii_case("none") {
                    style.list_style_image = ComputedImage::None;
                } else if let Some(image) = parse_list_style_image_component(
                    &components.image,
                    declaration.base_url,
                    declaration.root_url,
                ) {
                    style.list_style_image = image;
                }
            }
        }
        "list-style-type" => {
            style.list_style_type =
                parse_list_style_type(value).unwrap_or_else(|| style.list_style_type.clone());
        }
        "list-style-position" => {
            style.list_style_position =
                parse_list_style_position(value).unwrap_or(style.list_style_position);
        }
        "marker-side" => {
            if let Some(marker_side) = parse_marker_side(value) {
                style.marker_side = marker_side;
            }
        }
        "list-style-image" => {
            if let Some(image) =
                parse_list_style_image_component(value, declaration.base_url, declaration.root_url)
            {
                style.list_style_image = image;
            }
        }
        "counter-reset" => {
            if let Some(values) = parse_counter_resets(value) {
                style.counter_resets = values;
            }
        }
        "counter-increment" => {
            if let Some(values) = parse_counter_changes(value, 1, CounterDuplicatePolicy::KeepAll) {
                style.counter_increments = values;
            }
        }
        "counter-set" => {
            if let Some(values) = parse_counter_changes(value, 0, CounterDuplicatePolicy::KeepLast)
            {
                style.counter_sets = values;
            }
        }
        "string-set" => {
            // CSS Generated Content for Paged Media defines named strings
            // as generated content captured from document elements and
            // later referenced in page-margin boxes with `string()`.
            // https://www.w3.org/TR/css-gcpm-3/#named-strings
            if let Some(values) = parse_named_string_sets(value) {
                style.string_sets = values;
            }
        }
        "page" => {
            let value = value.trim();
            if value.eq_ignore_ascii_case("auto") {
                style.page = PageAssignment::Auto;
            } else if is_css_identifier(value) {
                style.page = PageAssignment::Named(PageName::new(value.to_string()));
            }
        }
        "break-before" => {
            style.break_before = parse_fragment_break(value);
        }
        "break-after" => {
            style.break_after = parse_fragment_break(value);
        }
        "page-break-before" => {
            style.break_before = parse_page_break(value);
        }
        "page-break-after" => {
            style.break_after = parse_page_break(value);
        }
        "break-inside" => {
            if let Some(avoidance) = BreakInsideAvoidance::parse_modern(value) {
                style.break_inside = avoidance;
            }
        }
        "page-break-inside" => {
            if let Some(avoidance) = BreakInsideAvoidance::parse_legacy_page(value) {
                style.break_inside = avoidance;
            }
        }
        "orphans" => {
            if let Some(value) = parse_positive_integer(value) {
                style.orphans = Orphans::try_new(value).expect("positive integer parser");
            }
        }
        "widows" => {
            if let Some(value) = parse_positive_integer(value) {
                style.widows = Widows::try_new(value).expect("positive integer parser");
            }
        }
        "text-decoration" => {
            if let Some(decoration) = parse_text_decoration_shorthand(value, style) {
                style.text_decoration = decoration;
            }
        }
        "text-decoration-line" => {
            if let Some(line) = parse_text_decoration_line(value) {
                apply_text_decoration_line(&mut style.text_decoration, line);
            }
        }
        "text-decoration-style" => {
            if let Some(decoration_style) = parse_text_decoration_style(value) {
                style.text_decoration.style = decoration_style;
            }
        }
        "text-decoration-color" => {
            if let Some(color) = parse_color(value) {
                style.text_decoration.color = CssColorOrCurrentColor::Color(color);
            }
        }
        "text-decoration-thickness" => {
            if let Some(thickness) = parse_text_decoration_thickness(value, style.font_size) {
                style.text_decoration.thickness = thickness;
            }
        }
        "text-decoration-inset" => {
            if let Some(inset) = parse_text_decoration_inset(value, style.font_size) {
                style.text_decoration.inset = inset;
            }
        }
        "text-decoration-skip" => {
            if let Some((skip_ink, skip_self, skip_box, skip_spaces)) =
                parse_text_decoration_skip(value)
            {
                style.text_decoration.skip_ink = skip_ink;
                style.text_decoration.skip_self = skip_self;
                style.text_decoration.skip_box = skip_box;
                style.text_decoration.skip_spaces = skip_spaces;
            }
        }
        "text-decoration-skip-ink" => {
            if let Some(skip_ink) = parse_text_decoration_skip_ink(value) {
                style.text_decoration.skip_ink = skip_ink;
            }
        }
        "text-decoration-skip-self" => {
            if let Some(skip_self) = parse_text_decoration_skip_self(value) {
                style.text_decoration.skip_self = skip_self;
            }
        }
        "text-decoration-skip-box" => {
            if let Some(skip_box) = parse_text_decoration_skip_box(value) {
                style.text_decoration.skip_box = skip_box;
            }
        }
        "text-decoration-skip-spaces" => {
            if let Some(skip_spaces) = parse_text_decoration_skip_spaces(value) {
                style.text_decoration.skip_spaces = skip_spaces;
            }
        }
        "text-underline-offset" => {
            if let Some(offset) = parse_text_underline_offset(value, style.font_size) {
                style.text_decoration.underline_offset = offset;
            }
        }
        "text-underline-position" => {
            if let Some(position) = parse_text_underline_position(value) {
                style.text_decoration.underline_position = position;
            }
        }
        "text-emphasis-style" => {
            if let Some(emphasis_style) = parse_text_emphasis_style(value) {
                style.text_emphasis_style = emphasis_style;
            }
        }
        "text-emphasis" => {
            if let Some((emphasis_style, emphasis_color)) = parse_text_emphasis(value) {
                style.text_emphasis_style = emphasis_style;
                style.text_emphasis_color = emphasis_color
                    .map(CssColorOrCurrentColor::Color)
                    .unwrap_or(CssColorOrCurrentColor::CurrentColor);
            }
        }
        "text-emphasis-color" => {
            if let Some(color) = parse_color(value) {
                style.text_emphasis_color = CssColorOrCurrentColor::Color(color);
            }
        }
        "text-emphasis-position" => {
            if let Some(position) = parse_text_emphasis_position(value) {
                style.text_emphasis_position = position;
            }
        }
        "text-emphasis-skip" => {
            if let Some(skip) = parse_text_emphasis_skip(value) {
                style.text_emphasis_skip = skip;
            }
        }
        "text-shadow" => {
            if let Some(shadows) = parse_text_shadow(value, style.font_size) {
                style.text_shadow = shadows;
            }
        }
        "box-shadow" => {
            if let Some(shadows) = parse_box_shadow(value, style.font_size) {
                style.box_shadow = shadows;
            }
        }
        "white-space" => {
            let parsed = match value.to_ascii_lowercase().as_str() {
                "normal" => Some((WhiteSpace::Normal, TextWrapMode::Wrap)),
                "nowrap" => Some((WhiteSpace::NoWrap, TextWrapMode::NoWrap)),
                "pre" => Some((WhiteSpace::Pre, TextWrapMode::NoWrap)),
                "pre-wrap" => Some((WhiteSpace::PreWrap, TextWrapMode::Wrap)),
                "pre-line" => Some((WhiteSpace::PreLine, TextWrapMode::Wrap)),
                "break-spaces" => Some((WhiteSpace::BreakSpaces, TextWrapMode::Wrap)),
                _ => None,
            };
            if let Some((white_space, text_wrap_mode)) = parsed {
                // `white-space` is a legacy shorthand that resets its
                // wrapping-mode component, but not text-wrap-style.
                // CSS Text 4: https://drafts.csswg.org/css-text-4/#white-space-property
                style.white_space = white_space;
                style.text_wrap_mode = text_wrap_mode;
            }
        }
        "text-wrap" => {
            let mut mode = TextWrapMode::Wrap;
            let mut wrap_style = TextWrapStyle::Auto;
            let mut saw_mode = false;
            let mut saw_style = false;
            let mut valid = true;
            for component in value.split_ascii_whitespace() {
                match component.to_ascii_lowercase().as_str() {
                    "wrap" if !saw_mode => {
                        mode = TextWrapMode::Wrap;
                        saw_mode = true;
                    }
                    "nowrap" if !saw_mode => {
                        mode = TextWrapMode::NoWrap;
                        saw_mode = true;
                    }
                    "auto" if !saw_style => {
                        wrap_style = TextWrapStyle::Auto;
                        saw_style = true;
                    }
                    "balance" if !saw_style => {
                        wrap_style = TextWrapStyle::Balance;
                        saw_style = true;
                    }
                    "stable" if !saw_style => {
                        wrap_style = TextWrapStyle::Stable;
                        saw_style = true;
                    }
                    _ => valid = false,
                }
            }
            if valid && (saw_mode || saw_style) {
                style.text_wrap_mode = mode;
                style.text_wrap_style = wrap_style;
            }
        }
        "text-wrap-mode" => match value.trim().to_ascii_lowercase().as_str() {
            "wrap" => style.text_wrap_mode = TextWrapMode::Wrap,
            "nowrap" => style.text_wrap_mode = TextWrapMode::NoWrap,
            _ => {}
        },
        "text-wrap-style" => match value.trim().to_ascii_lowercase().as_str() {
            "auto" => style.text_wrap_style = TextWrapStyle::Auto,
            "balance" => style.text_wrap_style = TextWrapStyle::Balance,
            "stable" => style.text_wrap_style = TextWrapStyle::Stable,
            _ => {}
        },
        "wrap-inside" => match value.trim().to_ascii_lowercase().as_str() {
            "auto" => style.wrap_inside = WrapInside::Auto,
            "avoid" => style.wrap_inside = WrapInside::Avoid,
            _ => {}
        },
        "max-lines" => {
            if let Some(max_lines) = parse_max_lines(value) {
                style.max_lines = max_lines;
                style.line_limit_traversal = None;
            }
        }
        "block-ellipsis" => {
            if let Some(ellipsis) = parse_block_ellipsis(value) {
                style.block_ellipsis = ellipsis;
                style.line_limit_traversal = None;
            }
        }
        "continue" => {
            if let Some(continuation) = parse_continue(value) {
                style.continue_ = continuation;
                style.line_limit_traversal = None;
            }
        }
        "overflow-clip-margin" => {
            if let Some(margin) = parse_overflow_clip_margin(value, style.font_size) {
                style.overflow_clip_margin = margin;
            }
        }
        "word-break" => {
            match value.trim().to_ascii_lowercase().as_str() {
                "normal" => style.word_break = WordBreak::Normal,
                "break-all" => style.word_break = WordBreak::BreakAll,
                "keep-all" => style.word_break = WordBreak::KeepAll,
                "auto-phrase" => style.word_break = WordBreak::AutoPhrase,
                "manual" => style.word_break = WordBreak::Manual,
                "break-word" => {
                    // This legacy value has `overflow-wrap:anywhere`
                    // behavior regardless of the authored `overflow-wrap`
                    // value. Preserve it on `word-break` so a later
                    // `overflow-wrap` declaration cannot erase its
                    // min-content effect.
                    // <https://drafts.csswg.org/css-text-3/#valdef-word-break-break-word>
                    style.word_break = WordBreak::BreakWord;
                }
                _ => {}
            }
        }
        "overflow" => {
            if let Some(overflow) = parse_overflow_value(value) {
                style.overflow = overflow;
                style.overflow_x = overflow;
                style.overflow_y = overflow;
            }
        }
        "overflow-x" => {
            if let Some(overflow) = parse_overflow_value(value) {
                style.overflow_x = overflow;
            }
        }
        "overflow-y" => {
            if let Some(overflow) = parse_overflow_value(value) {
                style.overflow_y = overflow;
            }
        }
        "scrollbar-gutter" => {
            if let Some(gutter) = parse_scrollbar_gutter(value) {
                style.scrollbar_gutter = gutter;
            }
        }
        "scrollbar-width" => match value.trim().to_ascii_lowercase().as_str() {
            "auto" => style.scrollbar_width = ScrollbarWidth::Auto,
            "thin" => style.scrollbar_width = ScrollbarWidth::Thin,
            "none" => style.scrollbar_width = ScrollbarWidth::None,
            _ => {}
        },
        "scroll-snap-type" => {
            if let Some(value) = parse_scroll_snap_type(value) {
                style.scroll_snap_type = value;
            }
        }
        "scroll-snap-align" => {
            if let Some(value) = parse_scroll_snap_align(value) {
                style.scroll_snap_align = value;
            }
        }
        "scroll-snap-stop" => match value.trim().to_ascii_lowercase().as_str() {
            "normal" => style.scroll_snap_stop = ScrollSnapStop::Normal,
            "always" => style.scroll_snap_stop = ScrollSnapStop::Always,
            _ => {}
        },
        "scroll-padding-top" => set_scroll_padding_side(value, style.font_size, |edge| {
            style.scroll_padding.top = edge;
        }),
        "scroll-padding-right" => set_scroll_padding_side(value, style.font_size, |edge| {
            style.scroll_padding.right = edge;
        }),
        "scroll-padding-bottom" => set_scroll_padding_side(value, style.font_size, |edge| {
            style.scroll_padding.bottom = edge;
        }),
        "scroll-padding-left" => set_scroll_padding_side(value, style.font_size, |edge| {
            style.scroll_padding.left = edge;
        }),
        "scroll-margin-top" => set_scroll_margin_side(value, style.font_size, |edge| {
            style.scroll_margin.top = edge;
        }),
        "scroll-margin-right" => set_scroll_margin_side(value, style.font_size, |edge| {
            style.scroll_margin.right = edge;
        }),
        "scroll-margin-bottom" => set_scroll_margin_side(value, style.font_size, |edge| {
            style.scroll_margin.bottom = edge;
        }),
        "scroll-margin-left" => set_scroll_margin_side(value, style.font_size, |edge| {
            style.scroll_margin.left = edge;
        }),
        "overflow-wrap" | "word-wrap" => {
            style.overflow_wrap = match value.trim().to_ascii_lowercase().as_str() {
                "normal" => OverflowWrap::Normal,
                "anywhere" => OverflowWrap::Anywhere,
                "break-word" => OverflowWrap::BreakWord,
                _ => style.overflow_wrap,
            };
        }
        "line-break" => {
            style.line_break = match value.trim().to_ascii_lowercase().as_str() {
                "auto" => LineBreak::Auto,
                "loose" => LineBreak::Loose,
                "normal" => LineBreak::Normal,
                "strict" => LineBreak::Strict,
                "anywhere" => LineBreak::Anywhere,
                _ => style.line_break,
            };
        }
        "hyphens" => {
            style.hyphens = match value.trim().to_ascii_lowercase().as_str() {
                "none" => Hyphens::None,
                "manual" => Hyphens::Manual,
                "auto" => Hyphens::Auto,
                _ => style.hyphens,
            };
        }
        "hyphenate-character" => {
            if let Some(character) = parse_hyphenate_character(value) {
                style.hyphenate_character = character;
            }
        }
        "hyphenate-limit-chars" => {
            if let Some(limit) = parse_hyphenate_limit_chars(value) {
                style.hyphenate_limit_chars = limit;
            }
        }
        "content" => {
            if let Some(content) =
                parse_content_property(value, declaration.base_url, declaration.root_url)
            {
                style.content = content;
            }
        }
        "quotes" => {
            if let Some(quotes) = parse_quotes(value, &inheritance_source.quotes) {
                style.quotes = quotes;
            }
        }
        _ => return false,
    }
    true
}

fn parse_scrollbar_gutter(value: &str) -> Option<ScrollbarGutter> {
    let values = value
        .split_ascii_whitespace()
        .map(|value| value.to_ascii_lowercase())
        .collect::<Vec<_>>();
    match values.as_slice() {
        [value] if value == "auto" => Some(ScrollbarGutter::Auto),
        [value] if value == "stable" => Some(ScrollbarGutter::Stable { both_edges: false }),
        [first, second]
            if (first == "stable" && second == "both-edges")
                || (first == "both-edges" && second == "stable") =>
        {
            Some(ScrollbarGutter::Stable { both_edges: true })
        }
        _ => None,
    }
}

fn parse_scroll_snap_type(value: &str) -> Option<ScrollSnapType> {
    let parts = split_css_component_values(value);
    if parts.as_slice() == ["none"] {
        return Some(ScrollSnapType::None);
    }
    let [axis, strictness] = match parts.as_slice() {
        [axis] => [*axis, "proximity"],
        [axis, strictness] => [*axis, *strictness],
        _ => return None,
    };
    let strictness = match strictness.to_ascii_lowercase().as_str() {
        "mandatory" => ScrollSnapStrictness::Mandatory,
        "proximity" => ScrollSnapStrictness::Proximity,
        _ => return None,
    };
    match axis.to_ascii_lowercase().as_str() {
        "x" => Some(ScrollSnapType::X(strictness)),
        "y" => Some(ScrollSnapType::Y(strictness)),
        "block" => Some(ScrollSnapType::Block(strictness)),
        "inline" => Some(ScrollSnapType::Inline(strictness)),
        "both" => Some(ScrollSnapType::Both(strictness)),
        _ => None,
    }
}

fn parse_scroll_snap_align(value: &str) -> Option<ScrollSnapAlign> {
    let values = split_css_component_values(value);
    let [block, inline] = match values.as_slice() {
        [both] => [*both, *both],
        [block, inline] => [*block, *inline],
        _ => return None,
    };
    let parse = |value: &str| match value.to_ascii_lowercase().as_str() {
        "none" => Some(ScrollSnapAlignment::None),
        "start" => Some(ScrollSnapAlignment::Start),
        "end" => Some(ScrollSnapAlignment::End),
        "center" => Some(ScrollSnapAlignment::Center),
        _ => None,
    };
    Some(ScrollSnapAlign {
        block: parse(block)?,
        inline: parse(inline)?,
    })
}

fn set_scroll_padding_side(value: &str, font_size: f32, set: impl FnOnce(ScrollPadding)) {
    let value = value.trim();
    if value.eq_ignore_ascii_case("auto") {
        set(ScrollPadding::Auto);
        return;
    }
    let Some(value) = parse_computed_length_percentage(value, font_size) else {
        return;
    };
    if !value.is_definitely_negative() {
        set(ScrollPadding::LengthPercentage(value));
    }
}

fn set_scroll_margin_side(value: &str, font_size: f32, set: impl FnOnce(ComputedLengthPercentage)) {
    let Some(value) = parse_computed_length_percentage(value, font_size) else {
        return;
    };
    if !value.contains_percentage() {
        set(value);
    }
}

/// Parse CSS Overflow's independently cascaded `max-lines` longhand.
/// <https://drafts.csswg.org/css-overflow-4/#max-lines>
fn parse_max_lines(value: &str) -> Option<MaxLines> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(MaxLines::None);
    }
    std::num::NonZeroUsize::new(value.parse::<usize>().ok()?).map(MaxLines::Lines)
}

/// Parse the inherited block overflow marker and collapse forced line breaks
/// in authored strings as required by CSS Overflow.
/// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
fn parse_block_ellipsis(value: &str) -> Option<BlockEllipsis> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("auto") {
        return Some(BlockEllipsis::Auto);
    }
    if value.eq_ignore_ascii_case("no-ellipsis") {
        return Some(BlockEllipsis::NoEllipsis);
    }
    let (string, rest) = parse_css_string_token(value)?;
    if !rest.trim().is_empty() {
        return None;
    }
    let normalized = string
        .split(['\r', '\n'])
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Some(BlockEllipsis::String(std::sync::Arc::from(normalized)))
}

/// Parse the continuation policy. `-webkit-legacy` is accepted because the
/// line-clamp shorthands expand to this real computed value.
/// <https://drafts.csswg.org/css-overflow-4/#continue>
fn parse_continue(value: &str) -> Option<Continue> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(Continue::Auto),
        "collapse" => Some(Continue::Collapse),
        "discard" => Some(Continue::Discard),
        "-webkit-legacy" => Some(Continue::WebkitLegacy),
        _ => None,
    }
}

/// Parse the Level 3 `overflow-clip-margin` shorthand.
///
/// The visual-box keyword and signed length may appear in either order;
/// percentages are invalid for this Level 3 shorthand.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-margin>
fn parse_overflow_clip_margin(value: &str, font_size: f32) -> Option<OverflowClipMargin> {
    let mut reference_box = None;
    let mut length = None;
    for component in try_split_css_component_values(value)? {
        let component = component.to_ascii_lowercase();
        match component.as_str() {
            "border-box" if reference_box.is_none() => {
                reference_box = Some(OverflowClipMarginBox::Border)
            }
            "padding-box" if reference_box.is_none() => {
                reference_box = Some(OverflowClipMarginBox::Padding)
            }
            "content-box" if reference_box.is_none() => {
                reference_box = Some(OverflowClipMarginBox::Content)
            }
            _ if length.is_none() => {
                length = Some(parse_length_with_font_size(&component, font_size)?);
            }
            _ => return None,
        }
    }
    (reference_box.is_some() || length.is_some()).then_some(OverflowClipMargin {
        reference_box: reference_box.unwrap_or(OverflowClipMarginBox::Padding),
        offset: layout_pt(length.unwrap_or(0.0)),
    })
}
