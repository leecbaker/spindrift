use super::*;
use crate::css::html5_user_agent_stylesheet;

pub(crate) fn default_style_for_tag(tag: &str) -> ComputedStyle {
    // HTML's suggested rendering is expressed as a user-agent stylesheet, not
    // as renderer-side tag switches. Synthetic styles use the same cascade path
    // as DOM elements so defaults stay aligned with `css/ua/html5_ua.css`.
    // https://html.spec.whatwg.org/multipage/rendering.html#rendering
    let ua = html5_user_agent_stylesheet();
    style_for_element_with_signature(
        ElementSignature::new(tag, HashMap::new()),
        None,
        std::slice::from_ref(&ua),
        None,
        &[],
    )
}

pub(crate) fn style_for_element_with_signature(
    current: ElementSignature,
    inline_style: Option<&str>,
    stylesheets: &[Stylesheet],
    parent: Option<&ComputedStyle>,
    ancestors: &[ElementSignature],
) -> ComputedStyle {
    let inheritance_source = parent.cloned().unwrap_or_else(ComputedStyle::initial);
    let parent_ch_advance = fallback_ch_advance_for_style(&inheritance_source);
    style_for_element_with_signature_inner(
        current,
        inline_style,
        stylesheets,
        parent,
        ancestors,
        parent_ch_advance,
        true,
    )
}

pub(crate) fn style_for_element_with_signature_and_parent_ch_advance(
    current: ElementSignature,
    inline_style: Option<&str>,
    stylesheets: &[Stylesheet],
    parent: Option<&ComputedStyle>,
    ancestors: &[ElementSignature],
    parent_ch_advance: f32,
) -> ComputedStyle {
    style_for_element_with_signature_inner(
        current,
        inline_style,
        stylesheets,
        parent,
        ancestors,
        parent_ch_advance,
        false,
    )
}

fn style_for_element_with_signature_inner(
    mut current: ElementSignature,
    inline_style: Option<&str>,
    stylesheets: &[Stylesheet],
    parent: Option<&ComputedStyle>,
    ancestors: &[ElementSignature],
    parent_ch_advance: f32,
    apply_pseudos: bool,
) -> ComputedStyle {
    let inheritance_source = parent.cloned().unwrap_or_else(ComputedStyle::initial);
    let mut style = parent
        .map(inherited_base_style)
        .unwrap_or_else(ComputedStyle::initial);
    let resolved_language = current
        .attrs
        .get("lang")
        .or_else(|| current.attrs.get("xml:lang"))
        .map(|language| ResolvedLanguage::from_html_attribute(language))
        .unwrap_or_else(|| match &current.resolved_language {
            ResolvedLanguage::Unresolved => {
                ResolvedLanguage::from_computed(style.language.as_deref())
            }
            language => language.clone(),
        });
    style.language = resolved_language.as_computed_language();
    current = current.with_resolved_language(resolved_language);

    let mut matching_rules = Vec::new();
    let layer_order = global_layer_order(stylesheets);
    let mut presentational_hints_stylesheet_index = None;
    for (stylesheet_index, stylesheet) in stylesheets.iter().enumerate() {
        if stylesheet.html_presentational_hints {
            presentational_hints_stylesheet_index = Some(stylesheet_index);
        }
        for rule in &stylesheet.rules {
            if let Some(scope_proximity) = selector_matches_with_scope_proximity(
                &rule.selector,
                &rule.scopes,
                &current,
                ancestors,
            ) {
                matching_rules.push(MatchedRule {
                    origin: stylesheet.origin,
                    specificity: stylesheet.specificity_override.unwrap_or(rule.specificity),
                    stylesheet_index,
                    rule,
                    scope_proximity,
                });
            }
        }
    }
    matching_rules.sort_by_key(MatchedRule::cascade_key);

    let inline_declarations = inline_style.map(parse_declarations);
    let mut cascaded_declarations = Vec::new();
    for matched in &matching_rules {
        push_cascaded_rule_declarations(
            &mut cascaded_declarations,
            &matched.rule.declarations,
            RuleCascadeMeta::from_matched(matched, &layer_order),
        );
    }
    if let Some(stylesheet_index) = presentational_hints_stylesheet_index {
        push_dynamic_html_presentational_hint_declarations(
            &mut cascaded_declarations,
            &current,
            stylesheet_index,
        );
    }
    if let Some(inline_declarations) = &inline_declarations {
        push_cascaded_rule_declarations(
            &mut cascaded_declarations,
            inline_declarations,
            RuleCascadeMeta::inline_author(),
        );
    }
    if let Some(direction) = current.html_direction {
        push_html_direction_declaration(&mut cascaded_declarations, direction);
    }
    sort_cascaded_declarations(&mut cascaded_declarations);
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
        &mut style,
        &cascaded_declarations,
        &inheritance_source,
        parent_ch_advance,
    );
    let quotes_auto_language = match parent {
        Some(parent) => parent.language.as_deref(),
        None => style.language.as_deref(),
    };
    style.quotes.resolve_auto_language(quotes_auto_language);
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        style.abspos_static_source_was_inline_level = style.display.is_inline_level();
    }
    resolve_ua_relative_margins(&mut style);
    if apply_pseudos {
        let pseudo_parent_ch_advance = fallback_ch_advance_for_style(&style);
        apply_pseudo_rules_with_parent_ch_advance(
            &mut style,
            &current,
            stylesheets,
            ancestors,
            pseudo_parent_ch_advance,
        );
    }
    finalize_text_decoration_layers(&mut style);
    style
}

/// Add value-dependent HTML presentational hints for the current element.
///
/// Static presentational hints live in `html5_ph.css`. Legacy attributes whose
/// declarations depend on parsed attribute values are injected here with the
/// same author-origin, zero-specificity cascade priority:
/// <https://html.spec.whatwg.org/multipage/rendering.html#presentational-hints>.
fn push_dynamic_html_presentational_hint_declarations(
    output: &mut Vec<CascadedDeclaration<'_>>,
    element: &ElementSignature,
    stylesheet_index: usize,
) {
    if element.tag != "hr" {
        return;
    }
    let mut declaration_order = 0usize;
    if let Some(width) = element
        .attrs
        .get("width")
        .and_then(|value| html_dimension_property_value(value))
    {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            &mut declaration_order,
            "width",
            width,
        );
    }

    let size = element
        .attrs
        .get("size")
        .and_then(|value| parse_html_non_negative_integer(value));
    if let Some(size) = size {
        let shaded_solid =
            element.attrs.contains_key("color") || element.attrs.contains_key("noshade");
        if shaded_solid {
            if size >= 1 {
                push_dynamic_html_presentational_hint_declaration(
                    output,
                    stylesheet_index,
                    &mut declaration_order,
                    "border-width",
                    format!("{}px", format_html_number(size as f32 / 2.0)),
                );
            }
        } else if size == 1 {
            push_dynamic_html_presentational_hint_declaration(
                output,
                stylesheet_index,
                &mut declaration_order,
                "border-bottom-width",
                "0".to_string(),
            );
        } else if size > 1 {
            push_dynamic_html_presentational_hint_declaration(
                output,
                stylesheet_index,
                &mut declaration_order,
                "height",
                format!("{}px", size - 2),
            );
        }
    }

    if let Some(color) = element
        .attrs
        .get("color")
        .and_then(|value| html_legacy_color_hint_value(value))
    {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            &mut declaration_order,
            "border-color",
            color.clone(),
        );
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            &mut declaration_order,
            "color",
            color,
        );
    }
}

fn push_dynamic_html_presentational_hint_declaration(
    output: &mut Vec<CascadedDeclaration<'_>>,
    stylesheet_index: usize,
    declaration_order: &mut usize,
    name: &'static str,
    value: String,
) {
    output.push(CascadedDeclaration {
        name: std::borrow::Cow::Borrowed(name),
        value: std::borrow::Cow::Owned(value),
        origin: StylesheetOrigin::Author,
        base_url: None,
        root_url: None,
        important: false,
        layer_order: None,
        specificity: 0,
        scope_proximity: usize::MAX,
        stylesheet_index,
        rule_order: usize::MAX,
        declaration_order: *declaration_order,
    });
    *declaration_order += 1;
}

fn html_dimension_property_value(value: &str) -> Option<String> {
    let value = value.trim_start_matches(is_html_space);
    let mut end = 0usize;
    let mut saw_digit = false;
    let mut saw_dot = false;
    for (index, character) in value.char_indices() {
        if character.is_ascii_digit() {
            saw_digit = true;
            end = index + character.len_utf8();
        } else if character == '.' && !saw_dot {
            saw_dot = true;
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    if !saw_digit {
        return None;
    }
    if value[..end].parse::<f32>().ok()? < 0.0 {
        return None;
    }
    if value[end..].starts_with('%') {
        Some(format!("{}%", &value[..end]))
    } else {
        Some(format!("{}px", &value[..end]))
    }
}

fn parse_html_non_negative_integer(value: &str) -> Option<i32> {
    let value = value.trim_start_matches(is_html_space);
    let (sign, rest) = match value.as_bytes().first().copied() {
        Some(b'+') => (1, &value[1..]),
        Some(b'-') => (-1, &value[1..]),
        _ => (1, value),
    };
    let digit_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digit_len == 0 {
        return None;
    }
    let integer = rest[..digit_len].parse::<i32>().ok()? * sign;
    (integer >= 0).then_some(integer)
}

fn html_legacy_color_hint_value(value: &str) -> Option<String> {
    let trimmed = value.trim_matches(is_html_space);
    if trimmed.eq_ignore_ascii_case("transparent") || trimmed.eq_ignore_ascii_case("currentcolor") {
        return None;
    }
    let color = parse_color(trimmed)?;
    if color.a < 0.999 {
        return None;
    }
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        css_color_channel(color.r),
        css_color_channel(color.g),
        css_color_channel(color.b)
    ))
}

fn css_color_channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn format_html_number(value: f32) -> String {
    if value.fract().abs() < 0.000001 {
        format!("{}", value as i32)
    } else {
        format!("{value}")
    }
}

fn is_html_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\u{0c}' | '\r')
}

/// Derive the active decoration chain for this element.
///
/// CSS Text Decoration propagates line decorations through descendants, but
/// the non-inherited line/style/color/thickness/inset values remain those of
/// the decorating box. Keeping a derived layer list avoids mutating descendant
/// computed longhands while preserving the decorating box's used values:
/// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>.
pub(super) fn finalize_text_decoration_layers(style: &mut ComputedStyle) {
    if style.text_emphasis_color.is_none() {
        style.text_emphasis_color = Some(style.color);
    }
    if style.text_decoration.has_visible_line() {
        let mut decoration = style.text_decoration;
        decoration.color.get_or_insert(style.color);
        style.text_decoration_layers.push(decoration);
    }
}

/// Add HTML `dir=auto`/`bdi` directionality as a UA cascade declaration.
///
/// HTML Rendering expresses element directionality through UA-level
/// `direction` rules using `:dir()`, so author and user declarations must
/// continue to override it in the normal CSS Cascade order:
/// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering> and
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
fn push_html_direction_declaration<'a>(
    output: &mut Vec<CascadedDeclaration<'a>>,
    direction: Direction,
) {
    output.push(CascadedDeclaration {
        name: std::borrow::Cow::Borrowed("direction"),
        value: std::borrow::Cow::Borrowed(match direction {
            Direction::Ltr => "ltr",
            Direction::Rtl => "rtl",
        }),
        origin: StylesheetOrigin::UserAgent,
        base_url: None,
        root_url: None,
        important: false,
        layer_order: None,
        specificity: 0,
        scope_proximity: usize::MAX,
        stylesheet_index: 0,
        rule_order: usize::MAX,
        declaration_order: usize::MAX,
    });
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MatchedRule<'a> {
    origin: StylesheetOrigin,
    specificity: u32,
    stylesheet_index: usize,
    rule: &'a StyleRule,
    scope_proximity: usize,
}

impl MatchedRule<'_> {
    fn cascade_key(&self) -> (StylesheetOrigin, u32, usize, usize, usize) {
        (
            self.origin,
            self.specificity,
            usize::MAX.saturating_sub(self.scope_proximity),
            self.stylesheet_index,
            self.rule.order,
        )
    }
}

/// Resolves preserved UA stylesheet `em` margins after author font-size cascade.
///
/// WeasyPrint's HTML UA stylesheet defines heading margins in `em`. CSS Values
/// defines `em` as font-relative, and CSS Cascade defines computed values after
/// cascade:
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
pub(super) fn resolve_ua_relative_margins(style: &mut ComputedStyle) {
    if let Some(em) = style.ua_margin_em.top.take() {
        let margin = em * style.font_size;
        style.box_values.margin.top = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_length(margin),
        );
        style.margin.top = margin;
    }
    if let Some(em) = style.ua_margin_em.right.take() {
        let margin = em * style.font_size;
        style.box_values.margin.right = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_length(margin),
        );
        style.margin.right = margin;
    }
    if let Some(em) = style.ua_margin_em.bottom.take() {
        let margin = em * style.font_size;
        style.box_values.margin.bottom = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_length(margin),
        );
        style.margin.bottom = margin;
    }
    if let Some(em) = style.ua_margin_em.left.take() {
        let margin = em * style.font_size;
        style.box_values.margin.left = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_length(margin),
        );
        style.margin.left = margin;
    }
}

pub(crate) fn apply_pseudo_rules_with_parent_ch_advance(
    style: &mut ComputedStyle,
    current: &ElementSignature,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    parent_ch_advance: f32,
) {
    apply_marker_rules_with_parent_ch_advance(
        style,
        current,
        stylesheets,
        ancestors,
        parent_ch_advance,
    );
    apply_generated_pseudo_rules_with_parent_ch_advance(
        style,
        current,
        stylesheets,
        ancestors,
        parent_ch_advance,
    );
    apply_typographic_pseudo_rules_with_parent_ch_advance(
        style,
        current,
        stylesheets,
        ancestors,
        parent_ch_advance,
    );
}

pub(super) fn apply_marker_rules_with_parent_ch_advance(
    style: &mut ComputedStyle,
    current: &ElementSignature,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    parent_ch_advance: f32,
) {
    let mut matching_rules = Vec::new();
    let layer_order = global_layer_order(stylesheets);
    for (stylesheet_index, stylesheet) in stylesheets.iter().enumerate() {
        for rule in &stylesheet.marker_rules {
            if let Some(scope_proximity) = selector_matches_with_scope_proximity(
                &rule.selector,
                &rule.scopes,
                current,
                ancestors,
            ) {
                matching_rules.push(MatchedRule {
                    origin: stylesheet.origin,
                    specificity: stylesheet.specificity_override.unwrap_or(rule.specificity),
                    stylesheet_index,
                    rule,
                    scope_proximity,
                });
            }
        }
    }
    if matching_rules.is_empty() && !style.display.is_list_item() {
        return;
    }
    matching_rules.sort_by_key(MatchedRule::cascade_key);

    // CSS Pseudo-Elements 4 and CSS Lists 3 model `::marker` as a generated
    // marker box with a restricted property set. The marker starts from the
    // originating element's inherited font/color state, then marker rules
    // cascade onto that pseudo-element style.
    // https://www.w3.org/TR/css-pseudo-4/#marker-pseudo
    // https://www.w3.org/TR/css-lists-3/#marker-properties
    let inheritance_source = style.clone();
    let mut marker_style = pseudo_inherited_base_style(style);
    marker_style.marker_style = None;
    marker_style.before_style = None;
    marker_style.after_style = None;
    marker_style.first_line_style = None;
    marker_style.first_letter_style = None;
    marker_style.display = marker_style.display.with_list_item(false);
    marker_style.unicode_bidi = UnicodeBidi::Isolate;
    marker_style.white_space = WhiteSpace::Pre;
    marker_style.text_transform = TextTransform::NONE;
    let mut cascaded_declarations = Vec::new();
    for matched in &matching_rules {
        push_cascaded_rule_declarations(
            &mut cascaded_declarations,
            &matched.rule.declarations,
            RuleCascadeMeta::from_matched(matched, &layer_order),
        );
    }
    sort_cascaded_declarations(&mut cascaded_declarations);
    apply_cascaded_marker_declarations_with_inheritance_source_and_parent_ch_advance(
        &mut marker_style,
        &cascaded_declarations,
        &inheritance_source,
        parent_ch_advance,
    );
    marker_style
        .quotes
        .resolve_auto_language(inheritance_source.language.as_deref());
    style.marker_style = Some(Box::new(marker_style));
}

pub(super) fn apply_generated_pseudo_rules_with_parent_ch_advance(
    style: &mut ComputedStyle,
    current: &ElementSignature,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    parent_ch_advance: f32,
) {
    style.before_style = generated_pseudo_style_with_parent_ch_advance(
        style,
        current,
        stylesheets,
        ancestors,
        |stylesheet| &stylesheet.before_rules,
        parent_ch_advance,
    )
    .map(Box::new);
    style.after_style = generated_pseudo_style_with_parent_ch_advance(
        style,
        current,
        stylesheets,
        ancestors,
        |stylesheet| &stylesheet.after_rules,
        parent_ch_advance,
    )
    .map(Box::new);
}

pub(super) fn generated_pseudo_style_with_parent_ch_advance(
    originating_style: &ComputedStyle,
    current: &ElementSignature,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    rule_set: fn(&Stylesheet) -> &[StyleRule],
    parent_ch_advance: f32,
) -> Option<ComputedStyle> {
    let mut matching_rules = Vec::new();
    let layer_order = global_layer_order(stylesheets);
    for (stylesheet_index, stylesheet) in stylesheets.iter().enumerate() {
        for rule in rule_set(stylesheet) {
            if let Some(scope_proximity) = selector_matches_with_scope_proximity(
                &rule.selector,
                &rule.scopes,
                current,
                ancestors,
            ) {
                matching_rules.push(MatchedRule {
                    origin: stylesheet.origin,
                    specificity: stylesheet.specificity_override.unwrap_or(rule.specificity),
                    stylesheet_index,
                    rule,
                    scope_proximity,
                });
            }
        }
    }
    if matching_rules.is_empty() {
        return None;
    }
    matching_rules.sort_by_key(MatchedRule::cascade_key);

    // CSS Pseudo-Elements 4: `::before`/`::after` are generated boxes whose
    // styles inherit from their originating element, then cascade pseudo rules.
    // https://www.w3.org/TR/css-pseudo-4/#generated-content
    let inheritance_source = originating_style.clone();
    let mut pseudo_style = pseudo_inherited_base_style(originating_style);
    pseudo_style.content = Content::None;
    pseudo_style.display = Display::INLINE;
    pseudo_style.before_style = None;
    pseudo_style.after_style = None;
    pseudo_style.marker_style = None;
    pseudo_style.first_line_style = None;
    pseudo_style.first_letter_style = None;
    pseudo_style.counter_resets.clear();
    pseudo_style.counter_increments.clear();
    pseudo_style.counter_sets.clear();
    let mut cascaded_declarations = Vec::new();
    for matched in &matching_rules {
        push_cascaded_rule_declarations(
            &mut cascaded_declarations,
            &matched.rule.declarations,
            RuleCascadeMeta::from_matched(matched, &layer_order),
        );
    }
    sort_cascaded_declarations(&mut cascaded_declarations);
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
        &mut pseudo_style,
        &cascaded_declarations,
        &inheritance_source,
        parent_ch_advance,
    );
    pseudo_style
        .quotes
        .resolve_auto_language(inheritance_source.language.as_deref());
    pseudo_style.content.is_generated().then_some(pseudo_style)
}

pub(super) fn apply_typographic_pseudo_rules_with_parent_ch_advance(
    style: &mut ComputedStyle,
    current: &ElementSignature,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    parent_ch_advance: f32,
) {
    style.first_line_style = typographic_pseudo_style(
        style,
        current,
        stylesheets,
        ancestors,
        |stylesheet| &stylesheet.first_line_rules,
        is_first_line_allowed_property,
        parent_ch_advance,
    )
    .map(Box::new);
    style.first_letter_style = typographic_pseudo_style(
        style,
        current,
        stylesheets,
        ancestors,
        |stylesheet| &stylesheet.first_letter_rules,
        is_first_letter_allowed_property,
        parent_ch_advance,
    )
    .map(Box::new);
}

pub(super) fn typographic_pseudo_style(
    originating_style: &ComputedStyle,
    current: &ElementSignature,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    rule_set: fn(&Stylesheet) -> &[StyleRule],
    allows_property: fn(&str) -> bool,
    parent_ch_advance: f32,
) -> Option<ComputedStyle> {
    let mut matching_rules = Vec::new();
    let layer_order = global_layer_order(stylesheets);
    for (stylesheet_index, stylesheet) in stylesheets.iter().enumerate() {
        for rule in rule_set(stylesheet) {
            if let Some(scope_proximity) = selector_matches_with_scope_proximity(
                &rule.selector,
                &rule.scopes,
                current,
                ancestors,
            ) {
                matching_rules.push(MatchedRule {
                    origin: stylesheet.origin,
                    specificity: stylesheet.specificity_override.unwrap_or(rule.specificity),
                    stylesheet_index,
                    rule,
                    scope_proximity,
                });
            }
        }
    }
    if matching_rules.is_empty() {
        return None;
    }
    matching_rules.sort_by_key(MatchedRule::cascade_key);

    // CSS Pseudo-Elements 4 models `::first-line` and `::first-letter` as
    // tree-abiding typographic pseudo-elements that inherit from their
    // originating element before pseudo-element rules cascade.
    // https://www.w3.org/TR/css-pseudo-4/#first-line-pseudo
    // https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo
    let inheritance_source = originating_style.clone();
    let mut pseudo_style = pseudo_inherited_base_style(originating_style);
    pseudo_style.before_style = None;
    pseudo_style.after_style = None;
    pseudo_style.marker_style = None;
    pseudo_style.first_line_style = None;
    pseudo_style.first_letter_style = None;
    pseudo_style.counter_resets.clear();
    pseudo_style.counter_increments.clear();
    pseudo_style.counter_sets.clear();
    let mut cascaded_declarations = Vec::new();
    for matched in &matching_rules {
        push_cascaded_rule_declarations(
            &mut cascaded_declarations,
            &matched.rule.declarations,
            RuleCascadeMeta::from_matched(matched, &layer_order),
        );
    }
    sort_cascaded_declarations(&mut cascaded_declarations);
    cascaded_declarations.retain(|declaration| {
        declaration.name.starts_with("--") || allows_property(declaration.name.as_ref())
    });
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
        &mut pseudo_style,
        &cascaded_declarations,
        &inheritance_source,
        parent_ch_advance,
    );
    pseudo_style
        .quotes
        .resolve_auto_language(inheritance_source.language.as_deref());
    Some(pseudo_style)
}

/// CSS Pseudo-Elements 4 restricts `::first-line` to font, color/background,
/// text-spacing, decoration, text-transform, and selected inline text
/// properties.
///
/// <https://www.w3.org/TR/css-pseudo-4/#first-line-styling>
fn is_first_line_allowed_property(name: &str) -> bool {
    name.starts_with("font")
        || name.starts_with("background")
        || matches!(
            name,
            "color"
                | "letter-spacing"
                | "line-height"
                | "opacity"
                | "tab-size"
                | "text-decoration"
                | "text-decoration-line"
                | "text-decoration-style"
                | "text-decoration-color"
                | "text-decoration-thickness"
                | "text-decoration-inset"
                | "text-decoration-skip"
                | "text-decoration-skip-ink"
                | "text-decoration-skip-self"
                | "text-decoration-skip-box"
                | "text-decoration-skip-spaces"
                | "text-underline-offset"
                | "text-underline-position"
                | "text-emphasis"
                | "text-emphasis-color"
                | "text-emphasis-style"
                | "text-emphasis-position"
                | "text-emphasis-skip"
                | "text-shadow"
                | "text-transform"
                | "vertical-align"
                | "word-spacing"
        )
}

/// CSS Pseudo-Elements 4 restricts `::first-letter` to first-line properties
/// plus box-decoration properties, margin/padding/border, float, and clear.
///
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-styling>
fn is_first_letter_allowed_property(name: &str) -> bool {
    is_first_line_allowed_property(name)
        || name.starts_with("border")
        || name.starts_with("margin")
        || name.starts_with("padding")
        || matches!(name, "box-shadow" | "clear" | "float")
}

fn push_cascaded_rule_declarations<'a>(
    output: &mut Vec<CascadedDeclaration<'a>>,
    declarations: &'a Declarations,
    meta: RuleCascadeMeta,
) {
    output.extend(
        declarations
            .iter()
            .enumerate()
            .map(|(declaration_order, (name, value))| CascadedDeclaration {
                name: std::borrow::Cow::Borrowed(name.as_str()),
                value: std::borrow::Cow::Borrowed(value.as_str()),
                origin: meta.origin,
                base_url: declarations.base_url(),
                root_url: declarations.root_url(),
                important: declaration_is_important(value),
                layer_order: meta.layer_order,
                specificity: meta.specificity,
                scope_proximity: meta.scope_proximity,
                stylesheet_index: meta.stylesheet_index,
                rule_order: meta.rule_order,
                declaration_order,
            }),
    );
}

/// Cascade-sort metadata shared by all declarations from one matched rule.
///
/// CSS Cascade Level 5 sorts declarations by origin, layer, specificity,
/// scoped proximity, and source order before computed-value resolution:
/// <https://www.w3.org/TR/css-cascade-5/#cascade-sort>.
#[derive(Debug, Clone, Copy)]
struct RuleCascadeMeta {
    origin: StylesheetOrigin,
    specificity: u32,
    scope_proximity: usize,
    layer_order: Option<usize>,
    stylesheet_index: usize,
    rule_order: usize,
}

impl RuleCascadeMeta {
    fn from_matched(matched: &MatchedRule<'_>, layer_order: &HashMap<String, usize>) -> Self {
        Self {
            origin: matched.origin,
            specificity: matched.specificity,
            scope_proximity: matched.scope_proximity,
            layer_order: rule_layer_order(matched.rule, layer_order),
            stylesheet_index: matched.stylesheet_index,
            rule_order: matched.rule.order,
        }
    }

    fn inline_author() -> Self {
        Self {
            origin: StylesheetOrigin::Author,
            specificity: u32::MAX,
            scope_proximity: usize::MAX,
            layer_order: None,
            stylesheet_index: usize::MAX,
            rule_order: usize::MAX,
        }
    }
}

/// Builds the document-wide cascade layer order for the supplied stylesheets.
///
/// CSS Cascade Level 5 makes named layer order global to the cascade origin by
/// first declaration order. Origin sorting is handled separately, so sharing
/// this map across UA and author sheets does not change origin precedence:
/// <https://www.w3.org/TR/css-cascade-5/#layer-order>.
fn global_layer_order(stylesheets: &[Stylesheet]) -> HashMap<String, usize> {
    let mut result = HashMap::new();
    for stylesheet in stylesheets {
        for layer_name in &stylesheet.layer_names {
            let next_order = result.len();
            result.entry(layer_name.clone()).or_insert(next_order);
        }
    }
    result
}

fn rule_layer_order(rule: &StyleRule, layer_order: &HashMap<String, usize>) -> Option<usize> {
    rule.layer_name
        .as_ref()
        .and_then(|layer_name| layer_order.get(layer_name))
        .copied()
}
