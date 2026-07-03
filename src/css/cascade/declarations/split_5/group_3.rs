use super::*;

pub(in crate::css) fn apply_cascaded_declaration_group_3(
    style: &mut ComputedStyle,
    name: &str,
    value: &str,
    declaration: &CascadedDeclaration<'_>,
    inheritance_source: &ComputedStyle,
    _parent_ch_advance: f32,
) -> bool {
    match name {
        "font-family" => {
            style.font_family =
                parse_font_family(value).unwrap_or_else(|| style.font_family.clone());
        }
        "font-feature-settings" => {
            if let Some(font_feature_settings) = parse_font_feature_settings(value) {
                style.font_feature_settings = font_feature_settings;
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
        "bookmark-level" => {
            if let Some(level) = parse_bookmark_level(value) {
                style.bookmark_level = level;
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
                    style.list_style_image = None;
                    style.list_style_image_base_url = None;
                    style.list_style_image_root_url = None;
                } else if let Some(Some(image)) =
                    parse_list_style_image_component(&components.image)
                {
                    style.list_style_image = Some(image);
                    style.list_style_image_base_url =
                        declaration.base_url.map(std::path::Path::to_path_buf);
                    style.list_style_image_root_url =
                        declaration.root_url.map(std::path::Path::to_path_buf);
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
            if value.trim().eq_ignore_ascii_case("none") {
                style.list_style_image = None;
                style.list_style_image_base_url = None;
                style.list_style_image_root_url = None;
            } else if let Some(image) = extract_css_url(value) {
                style.list_style_image = Some(image);
                style.list_style_image_base_url =
                    declaration.base_url.map(std::path::Path::to_path_buf);
                style.list_style_image_root_url =
                    declaration.root_url.map(std::path::Path::to_path_buf);
            }
        }
        "counter-reset" => {
            if let Some(values) = parse_counter_pairs(value, 0, CounterDuplicatePolicy::KeepLast) {
                style.counter_resets = values;
            }
        }
        "counter-increment" => {
            if let Some(values) = parse_counter_pairs(value, 1, CounterDuplicatePolicy::KeepAll) {
                style.counter_increments = values;
            }
        }
        "counter-set" => {
            if let Some(values) = parse_counter_pairs(value, 0, CounterDuplicatePolicy::KeepLast) {
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
                style.page_name = None;
                style.page_name_specified = true;
            } else if is_css_identifier(value) {
                style.page_name = Some(value.to_string());
                style.page_name_specified = true;
            }
        }
        "break-before" | "page-break-before" => {
            style.break_before = parse_page_break(value);
        }
        "break-after" | "page-break-after" => {
            style.break_after = parse_page_break(value);
        }
        "break-inside" | "page-break-inside" => {
            style.break_inside_avoid = matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "avoid" | "avoid-page"
            );
        }
        "orphans" => {
            if let Some(value) = parse_positive_integer(value) {
                style.orphans = value;
            }
        }
        "widows" => {
            if let Some(value) = parse_positive_integer(value) {
                style.widows = value;
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
                style.text_decoration.color = Some(color);
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
                style.text_emphasis_color = emphasis_color;
            }
        }
        "text-emphasis-color" => {
            if let Some(color) = parse_color(value) {
                style.text_emphasis_color = Some(color);
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
            style.white_space = match value.to_ascii_lowercase().as_str() {
                "normal" => WhiteSpace::Normal,
                "nowrap" => WhiteSpace::NoWrap,
                "pre" => WhiteSpace::Pre,
                "pre-wrap" => WhiteSpace::PreWrap,
                "pre-line" => WhiteSpace::PreLine,
                "break-spaces" => WhiteSpace::BreakSpaces,
                _ => style.white_space,
            };
        }
        "word-break" => {
            match value.trim().to_ascii_lowercase().as_str() {
                "normal" => style.word_break = WordBreak::Normal,
                "break-all" => style.word_break = WordBreak::BreakAll,
                "keep-all" => style.word_break = WordBreak::KeepAll,
                "break-word" => {
                    // CSS Text defines legacy `word-break: break-word` as
                    // normal word breaking plus emergency wrapping when no
                    // earlier soft wrap opportunity can fit the line:
                    // https://www.w3.org/TR/css-text-3/#word-break-property
                    style.word_break = WordBreak::Normal;
                    style.overflow_wrap = OverflowWrap::BreakWord;
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
