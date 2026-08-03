use super::*;
use crate::css::html5_user_agent_stylesheet;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

/// Whether an attribute-free element with `tag` has a block-level default
/// display in Quire's HTML UA stylesheet.
///
/// This is intentionally derived through the ordinary UA cascade rather than
/// duplicating the stylesheet's display selector list. Inline-text collection
/// calls this for every nested element, however, and cloning the complete UA
/// stylesheet for each call is needlessly expensive. The CSS is process-wide
/// immutable, so caching this tag-only cascade result preserves its semantics.
pub(crate) fn default_display_is_block_level_for_tag(tag: &str) -> bool {
    // This cache is process-wide. Bound it so documents with attacker-chosen
    // custom element names cannot retain arbitrary input indefinitely.
    const MAXIMUM_CACHED_TAGS: usize = 128;
    static BLOCK_LEVEL_BY_TAG: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();

    let cache = BLOCK_LEVEL_BY_TAG.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(&is_block_level) = cache
        .lock()
        .expect("default display cache mutex must not be poisoned")
        .get(tag)
    {
        return is_block_level;
    }

    // Do the cascade outside the cache lock. A concurrent first lookup may
    // duplicate this one-time calculation, but no steady-state lookup waits on
    // a full cascade.
    let is_block_level = default_style_for_tag(tag).display.is_block_level();
    let mut cache = cache
        .lock()
        .expect("default display cache mutex must not be poisoned");
    if cache.len() < MAXIMUM_CACHED_TAGS {
        cache.insert(tag.to_owned(), is_block_level);
    }
    is_block_level
}

pub(crate) fn default_style_for_tag(tag: &str) -> ComputedStyle {
    // HTML's suggested rendering is expressed as a user-agent stylesheet, not
    // as renderer-side tag switches. Synthetic styles use the same cascade path
    // as DOM elements so defaults stay aligned with `css/ua/html5_ua.css`.
    // https://html.spec.whatwg.org/multipage/rendering.html#rendering
    let ua = html5_user_agent_stylesheet();
    let stylesheets = Stylesheets::for_document(ua, None, &[]);
    style_for_element_with_signature(
        ElementSignature::new(tag, HashMap::new()),
        None,
        &stylesheets,
        None,
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_default_display_classification_matches_the_ua_cascade() {
        for tag in ["p", "span", "table", "custom-element"] {
            assert_eq!(
                default_display_is_block_level_for_tag(tag),
                default_style_for_tag(tag).display.is_block_level(),
                "{tag}"
            );
        }
    }
}

/// Build the style for a generated anonymous block box.
///
/// CSS 2.2 requires anonymous block boxes to inherit properties from their
/// enclosing non-anonymous box while all non-inherited properties take their
/// initial values. The box has no originating element, so it must not receive
/// element, UA-tag, or pseudo-element rules:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
pub(crate) fn anonymous_block_style(parent: &ComputedStyle) -> ComputedStyle {
    let mut style = inherited_base_style(parent);
    style.display = Display::BLOCK;
    style
}

/// Build the style carried by text in an anonymous inline box.
///
/// A DOM text node does not inherit non-inherited box properties from its
/// parent. In particular, copying a parent's background onto a text run turns
/// an otherwise transparent anonymous table cell into a painted inline-sized
/// rectangle. Keep this distinct from [`anonymous_block_style`]: text has no
/// generated principal box, so its `display` value is not meaningful here.
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous>
pub(crate) fn anonymous_text_style(parent: &ComputedStyle) -> ComputedStyle {
    inherited_base_style(parent)
}

pub(crate) fn style_for_element_with_signature<Collection: StylesheetCollection + ?Sized>(
    current: ElementSignature,
    inline_style: Option<&str>,
    stylesheets: &Collection,
    parent: Option<&ComputedStyle>,
    ancestors: &[ElementSignature],
) -> ComputedStyle {
    let stylesheets = stylesheets.stylesheet_view();
    let initial_style = ComputedStyle::initial();
    let parent_ch_advance = fallback_ch_advance_for_style(parent.unwrap_or(&initial_style));
    style_for_element_with_signature_inner(
        current,
        inline_style,
        &stylesheets,
        parent,
        ancestors,
        parent_ch_advance,
        true,
    )
}

pub(crate) fn style_for_element_with_signature_and_parent_ch_advance<
    Collection: StylesheetCollection + ?Sized,
>(
    current: ElementSignature,
    inline_style: Option<&str>,
    stylesheets: &Collection,
    parent: Option<&ComputedStyle>,
    ancestors: &[ElementSignature],
    parent_ch_advance: LayoutLength,
) -> ComputedStyle {
    let stylesheets = stylesheets.stylesheet_view();
    style_for_element_with_signature_inner(
        current,
        inline_style,
        &stylesheets,
        parent,
        ancestors,
        parent_ch_advance,
        false,
    )
}

struct ElementCascadeContext<'a> {
    chain: std::rc::Rc<Vec<std::borrow::Cow<'a, ElementSignature>>>,
    selector_caches: SelectorCaches,
    layer_order: HashMap<String, usize>,
    matching_rules: Vec<MatchedRule<'a>>,
    cascaded_declarations: Vec<CascadedDeclaration<'a>>,
}

impl<'a> ElementCascadeContext<'a> {
    fn new(
        current: &'a ElementSignature,
        stylesheets: &Stylesheets<'_>,
        ancestors: &'a [ElementSignature],
    ) -> Self {
        Self {
            chain: selector_chain(current, ancestors),
            selector_caches: SelectorCaches::default(),
            layer_order: global_layer_order(stylesheets),
            matching_rules: Vec::new(),
            cascaded_declarations: Vec::new(),
        }
    }

    fn current_index(&self) -> usize {
        self.chain.len() - 1
    }

    fn collect_matching_rules(
        &mut self,
        stylesheets: &'a Stylesheets<'a>,
        rule_set: fn(&Stylesheet) -> &[StyleRule],
    ) {
        self.matching_rules.clear();
        for (stylesheet_index, stylesheet) in stylesheets.iter().enumerate() {
            for rule in rule_set(stylesheet) {
                if let Some((scope_proximity, matching_specificity)) =
                    selector_matches_with_scope_proximity_in_chain(
                        &rule.selector,
                        &rule.scopes,
                        &self.chain,
                        self.current_index(),
                        &mut self.selector_caches,
                    )
                {
                    self.matching_rules.push(MatchedRule {
                        origin: stylesheet.origin,
                        specificity: stylesheet
                            .specificity_override
                            .unwrap_or(matching_specificity),
                        stylesheet_index,
                        rule,
                        scope_proximity,
                    });
                }
            }
        }
        self.matching_rules.sort_by_key(MatchedRule::cascade_key);
    }

    fn rebuild_cascaded_declarations(&mut self) {
        self.cascaded_declarations.clear();
        for matched in &self.matching_rules {
            push_cascaded_rule_declarations(
                &mut self.cascaded_declarations,
                &matched.rule.declarations,
                RuleCascadeMeta::from_matched(matched, &self.layer_order),
            );
        }
    }
}

fn style_for_element_with_signature_inner(
    mut current: ElementSignature,
    inline_style: Option<&str>,
    stylesheets: &Stylesheets<'_>,
    parent: Option<&ComputedStyle>,
    ancestors: &[ElementSignature],
    parent_ch_advance: LayoutLength,
    apply_pseudos: bool,
) -> ComputedStyle {
    let initial_style = ComputedStyle::initial();
    // The document root has no element parent. Its inherited values, including
    // the `em` basis of its own `font-size`, therefore begin at CSS initial
    // values rather than at Quire's outer rendering defaults.
    // https://www.w3.org/TR/css-cascade-5/#root-element
    // https://www.w3.org/TR/css-values-4/#em
    let is_document_root = ancestors.is_empty() && current.tag.eq_ignore_ascii_case("html");
    let inheritance_source = if is_document_root {
        &initial_style
    } else {
        parent.unwrap_or(&initial_style)
    };
    let mut style = if is_document_root {
        ComputedStyle::initial()
    } else {
        parent
            .map(inherited_base_style)
            .unwrap_or_else(ComputedStyle::initial)
    };
    style.registered_custom_properties = Arc::new(stylesheets.registered_custom_properties());
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

    let inline_declarations = inline_style.map(parse_declarations);
    let mut cascade = ElementCascadeContext::new(&current, stylesheets, ancestors);
    let mut presentational_hints_stylesheet_index = None;
    for (stylesheet_index, stylesheet) in stylesheets.iter().enumerate() {
        if stylesheet.html_presentational_hints {
            presentational_hints_stylesheet_index = Some(stylesheet_index);
        }
    }
    cascade.collect_matching_rules(stylesheets, |stylesheet| &stylesheet.rules);
    cascade.rebuild_cascaded_declarations();
    let user_agent_stylesheet_index = stylesheets
        .iter()
        .enumerate()
        .filter_map(|(index, stylesheet)| {
            (stylesheet.origin == StylesheetOrigin::UserAgent).then_some(index)
        })
        .last()
        .unwrap_or(0);
    push_dynamic_html_list_user_agent_declarations(
        &mut cascade.cascaded_declarations,
        &current,
        user_agent_stylesheet_index,
    );
    if let Some(stylesheet_index) = presentational_hints_stylesheet_index {
        push_dynamic_html_presentational_hint_declarations(
            &mut cascade.cascaded_declarations,
            &current,
            stylesheet_index,
            stylesheets
                .get(stylesheet_index)
                .expect("matched stylesheet index must refer to the stylesheet collection"),
            ancestors,
            stylesheets.html_container_frame_body_margins(),
        );
    }
    if let Some(inline_declarations) = &inline_declarations {
        push_cascaded_rule_declarations(
            &mut cascade.cascaded_declarations,
            inline_declarations,
            RuleCascadeMeta::inline_author(),
        );
    }
    if let Some(direction) = current.html_direction {
        push_html_direction_declaration(&mut cascade.cascaded_declarations, direction);
    }
    resolve_typed_attr_references(&mut cascade.cascaded_declarations, &current, stylesheets);
    sort_cascaded_declarations(&mut cascade.cascaded_declarations);
    let animation_declarations =
        animation_snapshot_declarations(&cascade.cascaded_declarations, stylesheets);
    if !animation_declarations.is_empty() {
        // The animation origin wins over all ordinary author declarations but
        // loses to author-important declarations. Its synthetic source-order
        // position lets the ordinary Cascade 5 sorter express that relation
        // without folding animation behavior into every property parser.
        // <https://www.w3.org/TR/css-animations-1/#animation-cascade-order>
        push_cascaded_rule_declarations(
            &mut cascade.cascaded_declarations,
            &animation_declarations,
            RuleCascadeMeta::animation(),
        );
        sort_cascaded_declarations(&mut cascade.cascaded_declarations);
    }
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
        &mut style,
        &cascade.cascaded_declarations,
        inheritance_source,
        parent_ch_advance,
        is_document_root,
        stylesheets.color_scheme_preference(),
    );
    select_style_image_sets(&mut style, stylesheets.image_set_resolution_dppx());
    if is_document_root {
        style.root_font_size = style.font_size;
        style.page_color_scheme = style.used_color_scheme;
    }
    if display_contents_computes_to_none_for_html_element(&current, &style) {
        style.display = Display::NONE;
    }
    let quotes_auto_language = match parent {
        Some(parent) => parent.language.as_deref(),
        None => style.language.as_deref(),
    };
    style.quotes.resolve_auto_language(quotes_auto_language);
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        style.abspos_static_source = StaticPositionSource::from_display(style.display);
    }
    resolve_ua_relative_margins(&mut style);
    if apply_pseudos {
        let pseudo_parent_ch_advance = fallback_ch_advance_for_style(&style);
        apply_pseudo_rules_with_context(
            &mut style,
            stylesheets,
            &mut cascade,
            pseudo_parent_ch_advance,
        );
        suppress_generated_pseudos_for_html_replaced_control(&mut style, &current);
    }
    finalize_text_decoration_layers(&mut style);
    apply_forced_color_used_values(&mut style, &current, parent, stylesheets);
    style
}

/// Resolve image-set candidate lists after declaration parsing, variable
/// substitution, and cascade have produced one computed style. CSS Images
/// selection is a rendering-environment decision, not declaration parsing.
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
fn select_style_image_sets(style: &mut ComputedStyle, resolution_dppx: f32) {
    style.background_image.select_image_set(resolution_dppx);
    for layer in &mut style.background_layers {
        layer.image.select_image_set(resolution_dppx);
    }
    style.border_image.source.select_image_set(resolution_dppx);
    style.list_style_image.select_image_set(resolution_dppx);
    match &mut style.content {
        Content::List { parts, .. } => select_generated_content_image_sets(parts, resolution_dppx),
        Content::Replacement { image, .. } => {
            select_generated_content_image_sets(std::slice::from_mut(image), resolution_dppx)
        }
        Content::Normal | Content::None => {}
    }
}

fn select_generated_content_image_sets(parts: &mut [GeneratedContentPart], resolution_dppx: f32) {
    for part in parts {
        if let GeneratedContentPart::Image { image } = part {
            image.select_image_set(resolution_dppx);
        }
    }
}

/// Apply CSS CssColor Adjustment's forced-colors substitutions at the boundary
/// between cascade and layout. The renderer does not expose computed styles to
/// script, so layout stores the resolved used colors while retaining a marker
/// on direct system-color values until this point.
/// <https://www.w3.org/TR/css-color-adjust-1/#forced-colors-mode>
fn apply_forced_color_used_values(
    style: &mut ComputedStyle,
    current: &ElementSignature,
    parent: Option<&ComputedStyle>,
    stylesheets: &Stylesheets<'_>,
) {
    let Some(palette) = stylesheets
        .iter()
        .find_map(|stylesheet| stylesheet.forced_colors.palette())
    else {
        return;
    };

    if style.forced_color_adjust == ForcedColorAdjust::None {
        resolve_style_system_colors(style, palette);
        return;
    }
    if style.forced_color_adjust == ForcedColorAdjust::PreserveParentColor {
        if let Some(parent) = parent {
            style.color = parent.color;
        } else {
            style.color = palette.canvas_text;
        }
        resolve_style_system_colors(style, palette);
        return;
    }

    let resolve_or = |color: CssColor, fallback: CssColor| match color.system_color() {
        // Keep the system-color identity while substituting the active palette
        // value. Descendants inherit that identity and must not treat the
        // resulting concrete RGB value as an authored color to override.
        Some(system) => CssColor::system(system, palette.color(system)),
        None => fallback,
    };
    let foreground = if current.tag.eq_ignore_ascii_case("a") && current.attrs.contains_key("href")
    {
        palette.link_text
    } else {
        palette.canvas_text
    };
    style.color = resolve_or(style.color, foreground);
    if let CssColorOrCurrentColor::Color(color) = style.text_fill_color {
        style.text_fill_color = CssColorOrCurrentColor::Color(resolve_or(color, style.color));
    }
    let had_nondefault_border_color = style.border_color != CssColor::BLACK
        || [
            style.border_colors.top,
            style.border_colors.right,
            style.border_colors.bottom,
            style.border_colors.left,
        ]
        .into_iter()
        .any(|color| color.system_color().is_some() || color != CssColor::BLACK);
    style.border_color = resolve_or(style.border_color, palette.canvas_text);
    style.border_colors = BorderColors {
        top: resolve_or(style.border_colors.top, palette.canvas_text),
        right: resolve_or(style.border_colors.right, palette.canvas_text),
        bottom: resolve_or(style.border_colors.bottom, palette.canvas_text),
        left: resolve_or(style.border_colors.left, palette.canvas_text),
    };
    // Preserve a visible visited-link border when the cascaded shorthand
    // supplied only a non-default color. The forced-color used value is a
    // one-CSS-pixel CanvasText indicator; an untouched link keeps no border.
    if current.tag.eq_ignore_ascii_case("a")
        && current.attrs.contains_key("href")
        && style.border_styles == BorderStyles::NONE
        && had_nondefault_border_color
    {
        let width = ComputedLengthPercentage::from_points(CSS_PX_TO_PT);
        style.border_styles = BorderStyles {
            top: BorderStyle::Solid,
            right: BorderStyle::Solid,
            bottom: BorderStyle::Solid,
            left: BorderStyle::Solid,
        };
        style.border_widths = Edges {
            top: CSS_PX_TO_PT,
            right: CSS_PX_TO_PT,
            bottom: CSS_PX_TO_PT,
            left: CSS_PX_TO_PT,
        };
        style.border_width_values = CssEdges::all(width);
        style.border_width = CSS_PX_TO_PT;
    }
    style.outline_color = resolve_or(style.outline_color, palette.canvas_text);
    style.background_color = BackgroundColor::Color(
        match style
            .background_color
            .resolved_color(style.color)
            .system_color()
        {
            Some(system) => palette.color(system),
            None => palette
                .canvas
                .with_alpha(style.background_color.resolved_color(style.color).alpha()),
        },
    );
    if current.namespace_url == "http://www.w3.org/2000/svg" {
        for paint in [&mut style.svg_fill, &mut style.svg_stroke] {
            if let SvgPaint::Color(color) = paint.paint {
                paint.paint = SvgPaint::Color(resolve_or(color, palette.canvas_text));
            }
        }
    }
    if let CssColorOrCurrentColor::Color(color) = style.text_emphasis_color {
        style.text_emphasis_color =
            CssColorOrCurrentColor::Color(resolve_or(color, palette.canvas_text));
    }
    if let CssColorOrCurrentColor::Color(color) = style.text_decoration.color {
        style.text_decoration.color =
            CssColorOrCurrentColor::Color(resolve_or(color, palette.canvas_text));
    }
    for layer in &mut style.text_decoration_layers {
        if let CssColorOrCurrentColor::Color(color) = layer.decoration.color {
            layer.decoration.color =
                CssColorOrCurrentColor::Color(resolve_or(color, palette.canvas_text));
        }
    }
    style.row_rule.colors = GapRuleList::single(palette.canvas_text);
    style.column_rule.colors = GapRuleList::single(palette.canvas_text);
    style.box_shadow.clear();
    style.text_shadow.clear();
    // URL images are retained, except on ordinary inline boxes. Their
    // backgrounds are split across line fragments and are not painted as the
    // retained atomic-image case covered by the forced-colors rules.
    // <https://www.w3.org/TR/css-color-adjust-1/#forced-colors-properties>
    if !style_has_url_background_image(style)
        || (style.display.is_inline_level() && !style.display.is_atomic_inline())
    {
        style.background_image = ComputedImage::None;
        style.background_layers.clear();
    }
    apply_forced_color_used_values_to_pseudos(style, palette);
}

fn apply_forced_color_used_values_to_pseudos(
    style: &mut ComputedStyle,
    palette: ForcedColorPalette,
) {
    for pseudo in [
        style.marker_style.as_deref_mut(),
        style.before_style.as_deref_mut(),
        style.after_style.as_deref_mut(),
        style.footnote_call_style.as_deref_mut(),
        style.footnote_marker_style.as_deref_mut(),
        style.first_line_style.as_deref_mut(),
        style.first_letter_style.as_deref_mut(),
    ]
    .into_iter()
    .flatten()
    {
        if pseudo.forced_color_adjust == ForcedColorAdjust::Auto {
            pseudo.color = palette.canvas_text;
            pseudo.background_color = BackgroundColor::Color(
                palette
                    .canvas
                    .with_alpha(pseudo.background_color.resolved_color(pseudo.color).alpha()),
            );
            pseudo.border_color = palette.canvas_text;
            pseudo.border_colors = BorderColors {
                top: palette.canvas_text,
                right: palette.canvas_text,
                bottom: palette.canvas_text,
                left: palette.canvas_text,
            };
            pseudo.outline_color = palette.canvas_text;
            pseudo.box_shadow.clear();
            pseudo.text_shadow.clear();
        } else {
            resolve_style_system_colors(pseudo, palette);
        }
    }
}

fn resolve_style_system_colors(style: &mut ComputedStyle, palette: ForcedColorPalette) {
    let resolve = |color: CssColor| match color.system_color() {
        Some(system) => CssColor::system(system, palette.color(system)),
        None => color,
    };
    style.color = resolve(style.color);
    style.border_color = resolve(style.border_color);
    style.border_colors.top = resolve(style.border_colors.top);
    style.border_colors.right = resolve(style.border_colors.right);
    style.border_colors.bottom = resolve(style.border_colors.bottom);
    style.border_colors.left = resolve(style.border_colors.left);
    style.outline_color = resolve(style.outline_color);
    if let BackgroundColor::Color(color) = style.background_color {
        style.background_color = BackgroundColor::Color(resolve(color));
    }
    for paint in [&mut style.svg_fill, &mut style.svg_stroke] {
        if let SvgPaint::Color(color) = paint.paint {
            paint.paint = SvgPaint::Color(resolve(color));
        }
    }
    for color in [
        &mut style.text_fill_color,
        &mut style.text_emphasis_color,
        &mut style.text_decoration.color,
    ] {
        if let CssColorOrCurrentColor::Color(value) = *color {
            *color = CssColorOrCurrentColor::Color(resolve(value));
        }
    }
}

fn background_image_contains_url(image: &ComputedImage) -> bool {
    image
        .as_image()
        .is_some_and(|image| matches!(image.selected_image(), BackgroundImage::Url { .. }))
}

fn style_has_url_background_image(style: &ComputedStyle) -> bool {
    background_image_contains_url(&style.background_image)
        || style
            .background_layers
            .iter()
            .any(|layer| background_image_contains_url(&layer.image))
}

/// The portions of one CSS animation instance needed for static rendering.
///
/// A paged render has no advancing timeline, so its snapshot time is the
/// animation's creation time. Negative delay moves that snapshot into the
/// active interval, which is sufficient for deterministic CSS animations in
/// a document renderer:
/// <https://www.w3.org/TR/css-animations-1/#animation-delay>.
#[derive(Debug, Clone)]
struct AnimationSnapshot {
    name: String,
    duration_seconds: f32,
    delay_seconds: f32,
}

/// Produces active keyframe declarations for the static document snapshot.
///
/// Keyframes are interpolated in their property's value space before ordinary
/// computed-value resolution, so deferred percentages, viewport lengths, and
/// CSS comparison functions retain their existing used-value behavior:
/// <https://www.w3.org/TR/css-animations-1/#keyframes>.
fn animation_snapshot_declarations(
    declarations: &[CascadedDeclaration<'_>],
    stylesheets: &Stylesheets<'_>,
) -> Declarations {
    let Some(animation) = animation_snapshot_from_declarations(declarations) else {
        return Declarations::new();
    };
    if animation.duration_seconds <= 0.0 {
        return Declarations::new();
    }
    let progress = -animation.delay_seconds / animation.duration_seconds;
    if !(0.0..=1.0).contains(&progress) {
        return Declarations::new();
    }
    let keyframes = stylesheets
        .iter()
        .flat_map(|stylesheet| stylesheet.keyframes.iter())
        .rev()
        .find(|rule| rule.name.eq_ignore_ascii_case(&animation.name));
    let Some(keyframes) = keyframes else {
        return Declarations::new();
    };
    let before = keyframes
        .steps
        .iter()
        .filter(|step| step.offset <= progress)
        .max_by(|left, right| left.offset.total_cmp(&right.offset));
    let after = keyframes
        .steps
        .iter()
        .filter(|step| step.offset >= progress)
        .min_by(|left, right| left.offset.total_cmp(&right.offset));
    let (Some(before), Some(after)) = (before, after) else {
        return Declarations::new();
    };
    let interval = after.offset - before.offset;
    let interval_progress = if interval == 0.0 {
        0.0
    } else {
        (progress - before.offset) / interval
    };
    let mut names = Vec::new();
    for (name, value) in before.declarations.iter().chain(after.declarations.iter()) {
        if !declaration_is_important(value) && !names.iter().any(|seen| seen == name) {
            names.push(name.clone());
        }
    }
    names
        .into_iter()
        // `contain` is a discrete, non-animatable property. It may appear in
        // a keyframe rule, but must not contribute a declaration at the
        // animation origin.
        // <https://www.w3.org/TR/css-contain-1/#contain-property>
        .filter(|name| !name.eq_ignore_ascii_case("contain"))
        .filter_map(|name| {
            let from = before.declarations.get(&name)?;
            let to = after.declarations.get(&name)?;
            Some((
                name,
                interpolate_keyframe_value(from, to, interval_progress),
            ))
        })
        .collect()
}

fn animation_snapshot_from_declarations(
    declarations: &[CascadedDeclaration<'_>],
) -> Option<AnimationSnapshot> {
    let mut animation = AnimationSnapshot {
        name: "none".to_string(),
        duration_seconds: 0.0,
        delay_seconds: 0.0,
    };
    for declaration in declarations {
        let value = trim_css_value(&declaration.value);
        match declaration.name.as_ref() {
            "animation" => apply_animation_shorthand(&mut animation, value),
            "animation-name" => {
                animation.name =
                    crate::css::component_values::split_css_top_level_delimiter(value, ',')
                        .first()?
                        .trim()
                        .to_string();
            }
            "animation-duration" => {
                animation.duration_seconds = parse_animation_time(
                    crate::css::component_values::split_css_top_level_delimiter(value, ',')
                        .first()?,
                )?;
            }
            "animation-delay" => {
                animation.delay_seconds = parse_animation_time(
                    crate::css::component_values::split_css_top_level_delimiter(value, ',')
                        .first()?,
                )?;
            }
            _ => {}
        }
    }
    (!animation.name.eq_ignore_ascii_case("none")).then_some(animation)
}

fn apply_animation_shorthand(animation: &mut AnimationSnapshot, value: &str) {
    *animation = AnimationSnapshot {
        name: "none".to_string(),
        duration_seconds: 0.0,
        delay_seconds: 0.0,
    };
    let mut time_count = 0;
    let first_animation = crate::css::component_values::split_css_top_level_delimiter(value, ',')
        .into_iter()
        .next()
        .unwrap_or(value);
    for component in split_css_component_values(first_animation) {
        if let Some(time) = parse_animation_time(component) {
            if time_count == 0 {
                animation.duration_seconds = time;
            } else if time_count == 1 {
                animation.delay_seconds = time;
            }
            time_count += 1;
        } else if !matches!(
            component.to_ascii_lowercase().as_str(),
            "linear"
                | "ease"
                | "ease-in"
                | "ease-out"
                | "ease-in-out"
                | "running"
                | "paused"
                | "normal"
                | "reverse"
                | "alternate"
                | "alternate-reverse"
                | "none"
                | "forwards"
                | "backwards"
                | "both"
        ) {
            animation.name = component.to_string();
        }
    }
}

fn parse_animation_time(value: &str) -> Option<f32> {
    let value = value.trim().to_ascii_lowercase();
    value
        .strip_suffix("ms")
        .and_then(|milliseconds| milliseconds.trim().parse::<f32>().ok())
        .map(|milliseconds| milliseconds / 1000.0)
        .or_else(|| {
            value
                .strip_suffix('s')
                .and_then(|seconds| seconds.trim().parse::<f32>().ok())
        })
}

fn interpolate_keyframe_value(from: &str, to: &str, progress: f32) -> String {
    if from.eq_ignore_ascii_case(to) {
        return from.to_string();
    }
    format!(
        "calc(({}) * {} + ({}) * {})",
        from,
        1.0 - progress,
        to,
        progress
    )
}

/// Resolves typed `attr()` functions against the originating element before
/// property-specific computed-value parsing.
///
/// Typed `attr()` substitutions are parsed as CSS values, not inserted as CSS
/// source text.  The cascade already processes declarations weakest to
/// strongest, so leaving an invalid-at-computed-value-time substitution
/// unparseable correctly retains an earlier valid declaration:
/// <https://drafts.csswg.org/css-values-5/#attr-notation>.
fn resolve_typed_attr_references(
    declarations: &mut [CascadedDeclaration<'_>],
    element: &ElementSignature,
    stylesheets: &Stylesheets<'_>,
) {
    for declaration in declarations {
        // Generated-content properties have their own `attr()` grammar.  In
        // particular, `bookmark-label` and `string-set` retain the attribute
        // name in the computed value and evaluate it at their respective
        // layout-time capture points.  Replacing a bare `attr(name)` here
        // would inject an unquoted string into their token stream and make a
        // valid declaration invalid.
        if declaration_defers_attr_evaluation(declaration.name.as_ref()) {
            continue;
        }
        let value = declaration.value.as_ref();
        if !value.to_ascii_lowercase().contains("attr(") {
            continue;
        }
        let namespaces = stylesheets
            .get(declaration.stylesheet_index)
            .map(|stylesheet| &stylesheet.namespace_prefixes);
        let Some(value) = resolve_typed_attr_value(value, element, namespaces) else {
            // An invalid typed substitution makes this declaration invalid at
            // computed-value time.  Keep a token sequence no modeled
            // property grammar accepts instead of accidentally treating an
            // attribute value as a CSS-wide keyword.
            declaration.value = std::borrow::Cow::Borrowed("--quire-invalid-attr--");
            continue;
        };
        declaration.value = std::borrow::Cow::Owned(value);
    }
}

/// Return whether a property owns the generated-content `attr()` function.
///
/// These grammars preserve the attribute name in their computed values, then
/// evaluate it against the originating element or captured fragment.  CSS
/// Generated Content for Paged Media defines this behavior for named strings
/// and bookmarks, while CSS Content defines it for generated pseudo-content.
/// <https://www.w3.org/TR/css-content-3/#attr-notation>
/// <https://www.w3.org/TR/css-gcpm-3/#named-strings>
fn declaration_defers_attr_evaluation(name: &str) -> bool {
    matches!(name, "bookmark-label" | "content" | "string-set")
}

fn resolve_typed_attr_value(
    value: &str,
    element: &ElementSignature,
    namespaces: Option<&HashMap<String, String>>,
) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    loop {
        let lower = remaining.to_ascii_lowercase();
        let Some(start) = lower.find("attr(") else {
            output.push_str(remaining);
            return Some(output);
        };
        output.push_str(&remaining[..start]);
        let after_open = &remaining[start + "attr(".len()..];
        let (arguments, rest) = split_function_argument(after_open)?;
        output.push_str(&resolve_one_typed_attr(arguments, element, namespaces)?);
        remaining = rest;
    }
}

fn resolve_one_typed_attr(
    arguments: &str,
    element: &ElementSignature,
    namespaces: Option<&HashMap<String, String>>,
) -> Option<String> {
    let (head, fallback) = split_attr_fallback(arguments);
    let mut parts = head.split_whitespace();
    let name = parts.next()?;
    let type_syntax = parts.collect::<Vec<_>>().join(" ");
    let raw = attr_value_for_element(element, name, namespaces);
    let replacement = raw.and_then(|raw| typed_attr_replacement(raw, &type_syntax));
    replacement
        .map(str::to_owned)
        .or_else(|| fallback.map(str::to_owned))
}

fn attr_value_for_element<'a>(
    element: &'a ElementSignature,
    name: &str,
    namespaces: Option<&HashMap<String, String>>,
) -> Option<&'a str> {
    if let Some((prefix, local_name)) = name.split_once('|') {
        let namespace_url = namespaces?.get(prefix)?;
        return element
            .namespace_attrs
            .iter()
            .find(|attribute| {
                attribute.namespace_url == *namespace_url && attribute.local_name == local_name
            })
            .map(|attribute| attribute.value.as_str());
    }
    if element.document_is_html {
        return element
            .attrs
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str());
    }
    element
        .namespace_attrs
        .iter()
        .find(|attribute| attribute.namespace_url.is_empty() && attribute.local_name == name)
        .map(|attribute| attribute.value.as_str())
}

fn split_attr_fallback(arguments: &str) -> (&str, Option<&str>) {
    let mut depth = 0usize;
    for (index, character) in arguments.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                return (
                    arguments[..index].trim(),
                    Some(arguments[index + 1..].trim()),
                );
            }
            _ => {}
        }
    }
    (arguments.trim(), None)
}

fn typed_attr_replacement<'a>(raw: &'a str, type_syntax: &str) -> Option<&'a str> {
    let type_syntax = type_syntax.trim();
    if type_syntax.is_empty() || type_syntax.eq_ignore_ascii_case("raw-string") {
        return Some(raw);
    }
    let grammar = type_syntax.strip_prefix("type(")?.strip_suffix(')')?.trim();
    if grammar == "*" {
        return Some(raw);
    }
    if grammar.eq_ignore_ascii_case("<color>") {
        return parse_color(raw).map(|_| raw);
    }
    if grammar.eq_ignore_ascii_case("<length>") {
        let raw = raw.trim();
        if raw == "0" {
            return Some("0px");
        }
        let lower = raw.to_ascii_lowercase();
        let has_length_unit = [
            "px", "pt", "pc", "in", "cm", "mm", "q", "em", "rem", "ch", "vw", "vh",
        ]
        .iter()
        .any(|unit| lower.contains(unit));
        return (has_length_unit
            && parse_computed_length_percentage(raw, ROOT_FONT_SIZE_PT).is_some())
        .then_some(raw);
    }
    None
}

/// Adds the value-dependent portion of HTML's user-agent list rules.
///
/// Unlike optional legacy presentational hints, `ol[start]`, `ol[reversed]`,
/// and `li[value]` define the list's semantic ordinal. Expressing them as UA
/// counter declarations lets the ordinary cascade give author CSS precedence:
/// <https://html.spec.whatwg.org/multipage/rendering.html#lists>.
fn push_dynamic_html_list_user_agent_declarations(
    output: &mut Vec<CascadedDeclaration<'_>>,
    element: &ElementSignature,
    stylesheet_index: usize,
) {
    if !element.document_is_html
        || !matches!(
            element.namespace_url.as_str(),
            "" | "http://www.w3.org/1999/xhtml"
        )
    {
        return;
    }

    // The obsolete HTML `font` element remains part of the HTML rendering
    // rules. Its positive integer size maps to the legacy absolute-size
    // ladder, independently of optional presentational-hint support.
    // <https://html.spec.whatwg.org/multipage/rendering.html#phrasing-content-3>
    if element.tag == "font"
        && let Some(size) = element
            .attrs
            .get("size")
            .and_then(|value| value.trim().parse::<u8>().ok())
        && let Some(value) = match size {
            1 => Some("xx-small"),
            2 => Some("x-small"),
            3 => Some("small"),
            4 => Some("medium"),
            5 => Some("large"),
            6 => Some("xx-large"),
            7 => Some("xxx-large"),
            _ => None,
        }
    {
        output.push(CascadedDeclaration {
            name: std::borrow::Cow::Borrowed("font-size"),
            value: std::borrow::Cow::Borrowed(value),
            origin: StylesheetOrigin::UserAgent,
            base_url: None,
            root_url: None,
            important: false,
            layer_order: None,
            specificity: 1,
            scope_proximity: usize::MAX,
            stylesheet_index,
            rule_order: usize::MAX,
            declaration_order: 0,
        });
    }

    let declaration = match element.tag.as_str() {
        "ol" => {
            let start = element
                .attrs
                .get("start")
                .and_then(|value| value.trim().parse::<i32>().ok());
            if element.attrs.contains_key("reversed") {
                Some((
                    "counter-reset",
                    start.map_or_else(
                        || "reversed(list-item)".to_string(),
                        |start| format!("reversed(list-item) {}", start.saturating_add(1)),
                    ),
                ))
            } else {
                start.map(|start| {
                    (
                        "counter-reset",
                        format!("list-item {}", start.saturating_sub(1)),
                    )
                })
            }
        }
        "li" => element
            .attrs
            .get("value")
            .and_then(|value| value.trim().parse::<i32>().ok())
            .map(|value| ("counter-set", format!("list-item {value}"))),
        _ => None,
    };
    let Some((name, value)) = declaration else {
        return;
    };
    output.push(CascadedDeclaration {
        name: std::borrow::Cow::Borrowed(name),
        value: std::borrow::Cow::Owned(value),
        origin: StylesheetOrigin::UserAgent,
        base_url: None,
        root_url: None,
        important: false,
        layer_order: None,
        // One type selector plus one attribute selector.
        specificity: 1025,
        scope_proximity: usize::MAX,
        stylesheet_index,
        rule_order: usize::MAX,
        declaration_order: 0,
    });
}

/// Return whether CSS Display makes `display: contents` compute to `display: none`.
///
/// CSS Display 3 Appendix B defines this for HTML elements whose rendering is
/// not fully controlled by ordinary CSS boxes, including replaced elements,
/// form controls, and line-break controls:
/// <https://drafts.csswg.org/css-display/#unbox-html>.
fn display_contents_computes_to_none_for_html_element(
    element: &ElementSignature,
    style: &ComputedStyle,
) -> bool {
    style.display.is_contents()
        && (matches!(style.content, Content::Replacement { .. })
            || (element.document_is_html
                && matches!(
                    element.namespace_url.as_str(),
                    "" | "http://www.w3.org/1999/xhtml"
                )
                && matches!(
                    element.tag.as_str(),
                    "br" | "wbr"
                        | "meter"
                        | "progress"
                        | "canvas"
                        | "embed"
                        | "object"
                        | "audio"
                        | "iframe"
                        | "img"
                        | "video"
                        | "frame"
                        | "frameset"
                        | "input"
                        | "textarea"
                        | "select"
                )))
}

/// Add value-dependent HTML presentational hints for the current element.
///
/// Static presentational hints live in `html5_ph.css`. Legacy attributes whose
/// declarations depend on parsed attribute values are injected here with the
/// same author-origin, zero-specificity cascade priority:
/// <https://html.spec.whatwg.org/multipage/rendering.html#presentational-hints>.
fn push_dynamic_html_presentational_hint_declarations<'a>(
    output: &mut Vec<CascadedDeclaration<'a>>,
    element: &ElementSignature,
    stylesheet_index: usize,
    stylesheet: &'a Stylesheet,
    ancestors: &[ElementSignature],
    container_frame_body_margins: Option<HtmlContainerFrameBodyMargins>,
) {
    let mut declaration_order = 0usize;
    push_dynamic_body_margin_presentational_hint_declarations(
        output,
        element,
        stylesheet_index,
        stylesheet,
        &mut declaration_order,
        container_frame_body_margins,
    );
    if element.tag == "hr" {
        push_dynamic_hr_presentational_hint_declarations(
            output,
            element,
            stylesheet_index,
            stylesheet,
            &mut declaration_order,
        );
    }
    push_dynamic_replaced_element_dimension_hint_declarations(
        output,
        element,
        stylesheet_index,
        stylesheet,
        &mut declaration_order,
    );
    push_dynamic_table_presentational_hint_declarations(
        output,
        element,
        stylesheet_index,
        stylesheet,
        ancestors,
        &mut declaration_order,
    );
}

/// Inject HTML's width, height, and aspect-ratio presentational hints for
/// replaced embedded content.
///
/// These are ordinary author-origin, zero-specificity CSS declarations.  They
/// must be produced by the cascade rather than by a particular image layout
/// path so `content: <image>` replacements retain the same outer-box sizing
/// and author declarations can override the attributes uniformly.
/// <https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images>
fn push_dynamic_replaced_element_dimension_hint_declarations<'a>(
    output: &mut Vec<CascadedDeclaration<'a>>,
    element: &ElementSignature,
    stylesheet_index: usize,
    stylesheet: &'a Stylesheet,
    declaration_order: &mut usize,
) {
    if !element.document_is_html {
        return;
    }
    let is_image_button = element.tag == "input"
        && element
            .attrs
            .get("type")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("image"));
    let maps_dimensions = matches!(
        element.tag.as_str(),
        "img" | "embed" | "iframe" | "object" | "video"
    ) || is_image_button;
    if !maps_dimensions {
        return;
    }

    let width = element
        .attrs
        .get("width")
        .and_then(|value| html_non_negative_length_property_value(value));
    let height = element
        .attrs
        .get("height")
        .and_then(|value| html_non_negative_length_property_value(value));
    if let Some(width) = &width {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "width",
            width.clone(),
        );
    }
    if let Some(height) = &height {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "height",
            height.clone(),
        );
    }

    // HTML maps the paired dimensions to `auto <ratio>` only for image and
    // video elements (and image buttons). Percentages cannot form that hint.
    if matches!(element.tag.as_str(), "img" | "video") || is_image_button {
        let ratio =
            element
                .attrs
                .get("width")
                .and_then(|value| html_dimension_number(value))
                .zip(
                    element
                        .attrs
                        .get("height")
                        .and_then(|value| html_dimension_number(value)),
                )
                .filter(|(width, height)| {
                    *width > 0.0
                        && *height > 0.0
                        && element.attrs.get("width").is_some_and(|value| {
                            !value.trim_start_matches(is_html_space).contains('%')
                        })
                        && element.attrs.get("height").is_some_and(|value| {
                            !value.trim_start_matches(is_html_space).contains('%')
                        })
                });
        if let Some((width, height)) = ratio {
            push_dynamic_html_presentational_hint_declaration(
                output,
                stylesheet_index,
                stylesheet,
                declaration_order,
                "aspect-ratio",
                format!("auto {width} / {height}"),
            );
        }
    }
}

/// Inject the legacy body-margin hints from HTML's page rendering rules.
///
/// These values have a source precedence that is distinct from ordinary CSS:
/// a present-but-invalid body attribute suppresses every later legacy source,
/// leaving the UA's 8px body margin in effect. Valid results enter the normal
/// cascade as author-origin, zero-specificity declarations, so authored CSS
/// remains authoritative:
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-page>.
fn push_dynamic_body_margin_presentational_hint_declarations<'a>(
    output: &mut Vec<CascadedDeclaration<'a>>,
    element: &ElementSignature,
    stylesheet_index: usize,
    stylesheet: &'a Stylesheet,
    declaration_order: &mut usize,
    container_frame_body_margins: Option<HtmlContainerFrameBodyMargins>,
) {
    if element.tag != "body" || !element.document_is_html {
        return;
    }

    let horizontal = body_margin_hint_value(
        element,
        "marginwidth",
        "leftmargin",
        container_frame_body_margins.and_then(|margins| margins.horizontal),
    );
    let vertical = body_margin_hint_value(
        element,
        "marginheight",
        "topmargin",
        container_frame_body_margins.and_then(|margins| margins.vertical),
    );

    if let Some(value) = horizontal {
        let value = format!("{value}px");
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "margin-left",
            value.clone(),
        );
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "margin-right",
            value,
        );
    }
    if let Some(value) = vertical {
        let value = format!("{value}px");
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "margin-top",
            value.clone(),
        );
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "margin-bottom",
            value,
        );
    }
}

/// Resolve one physical pair of legacy body-margin sources.
///
/// Once either body attribute exists, its parsed result (including failure) is
/// final. The container frame can be used only when neither body attribute is
/// present, as required by HTML's ordered source table.
fn body_margin_hint_value(
    element: &ElementSignature,
    preferred_body_attribute: &str,
    fallback_body_attribute: &str,
    container_frame_value: Option<i32>,
) -> Option<i32> {
    for attribute in [preferred_body_attribute, fallback_body_attribute] {
        if let Some(value) = element.attrs.get(attribute) {
            return parse_html_non_negative_integer(value);
        }
    }
    container_frame_value
}

fn push_dynamic_hr_presentational_hint_declarations<'a>(
    output: &mut Vec<CascadedDeclaration<'a>>,
    element: &ElementSignature,
    stylesheet_index: usize,
    stylesheet: &'a Stylesheet,
    declaration_order: &mut usize,
) {
    if let Some(width) = element
        .attrs
        .get("width")
        .and_then(|value| html_dimension_property_value(value))
    {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
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
                    stylesheet,
                    declaration_order,
                    "border-width",
                    format!("{}px", format_html_number(size as f32 / 2.0)),
                );
            }
        } else if size == 1 {
            push_dynamic_html_presentational_hint_declaration(
                output,
                stylesheet_index,
                stylesheet,
                declaration_order,
                "border-bottom-width",
                "0".to_string(),
            );
        } else if size > 1 {
            push_dynamic_html_presentational_hint_declaration(
                output,
                stylesheet_index,
                stylesheet,
                declaration_order,
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
            stylesheet,
            declaration_order,
            "border-color",
            color.clone(),
        );
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "color",
            color,
        );
    }
}

/// Inject value-dependent HTML table hints.
///
/// The rendering rules express these as author-origin declarations with zero
/// specificity. Keeping the values in cascade (rather than table layout)
/// makes ordinary author CSS override them and keeps table display overrides
/// independent from legacy markup:
/// <https://html.spec.whatwg.org/multipage/rendering.html#tables-2>.
fn push_dynamic_table_presentational_hint_declarations<'a>(
    output: &mut Vec<CascadedDeclaration<'a>>,
    element: &ElementSignature,
    stylesheet_index: usize,
    stylesheet: &'a Stylesheet,
    ancestors: &[ElementSignature],
    declaration_order: &mut usize,
) {
    let is_table = element.tag == "table";
    let is_cell = matches!(element.tag.as_str(), "td" | "th");
    let is_row_group = matches!(element.tag.as_str(), "thead" | "tbody" | "tfoot");
    let is_row = element.tag == "tr";
    if !(is_table || is_cell || is_row_group || is_row) {
        return;
    }

    if (is_table || is_cell)
        && let Some(width) = element
            .attrs
            .get("width")
            .and_then(|value| html_positive_dimension_property_value(value))
    {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "width",
            width,
        );
    }
    if (is_table || is_cell || is_row_group || is_row)
        && let Some(height) = element
            .attrs
            .get("height")
            .and_then(|value| html_dimension_property_value(value))
    {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "height",
            height,
        );
    }
    if is_table {
        if let Some(spacing) = element
            .attrs
            .get("cellspacing")
            .and_then(|value| html_non_negative_length_property_value(value))
        {
            push_dynamic_html_presentational_hint_declaration(
                output,
                stylesheet_index,
                stylesheet,
                declaration_order,
                "border-spacing",
                spacing,
            );
        }
        if let Some(color) = element
            .attrs
            .get("bordercolor")
            .and_then(|value| html_legacy_color_hint_value(value))
        {
            push_dynamic_html_presentational_hint_declaration(
                output,
                stylesheet_index,
                stylesheet,
                declaration_order,
                "border-color",
                color,
            );
        }
        if element
            .attrs
            .get("border")
            .and_then(|value| parse_html_non_negative_integer(value))
            == Some(0)
        {
            push_dynamic_html_presentational_hint_declaration(
                output,
                stylesheet_index,
                stylesheet,
                declaration_order,
                "border-style",
                "none".to_string(),
            );
        }
    }
    if is_cell
        && let Some(padding) = ancestors.iter().rev().find_map(|ancestor| {
            (ancestor.tag == "table")
                .then(|| ancestor.attrs.get("cellpadding"))
                .flatten()
                .and_then(|value| html_non_negative_length_property_value(value))
        })
    {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "padding",
            padding,
        );
    }
    if is_cell
        && ancestors.iter().rev().any(|ancestor| {
            ancestor.tag == "table"
                && ancestor
                    .attrs
                    .get("border")
                    .and_then(|value| parse_html_non_negative_integer(value))
                    == Some(0)
        })
    {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "border-style",
            "none".to_string(),
        );
    }
    if let Some(background) = element.attrs.get("background")
        && !background.trim_matches(is_html_space).is_empty()
    {
        push_dynamic_html_presentational_hint_declaration(
            output,
            stylesheet_index,
            stylesheet,
            declaration_order,
            "background-image",
            format!("url({background:?})"),
        );
    }
}

fn push_dynamic_html_presentational_hint_declaration<'a>(
    output: &mut Vec<CascadedDeclaration<'a>>,
    stylesheet_index: usize,
    stylesheet: &'a Stylesheet,
    declaration_order: &mut usize,
    name: &'static str,
    value: String,
) {
    output.push(CascadedDeclaration {
        name: std::borrow::Cow::Borrowed(name),
        value: std::borrow::Cow::Owned(value),
        origin: StylesheetOrigin::Author,
        base_url: stylesheet.base_url.as_ref(),
        root_url: stylesheet.root_url.as_ref(),
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

fn html_positive_dimension_property_value(value: &str) -> Option<String> {
    let dimension = html_dimension_property_value(value)?;
    html_dimension_number(value).filter(|number| *number > 0.0)?;
    Some(dimension)
}

fn html_non_negative_length_property_value(value: &str) -> Option<String> {
    let trimmed = value.trim_matches(is_html_space);
    if trimmed.is_empty() {
        return None;
    }
    if let Some(number) = trimmed.strip_suffix("pt")
        && number
            .trim()
            .parse::<f32>()
            .ok()
            .is_some_and(|number| number >= 0.0)
    {
        return Some(trimmed.to_string());
    }
    html_dimension_property_value(trimmed)
}

fn html_dimension_number(value: &str) -> Option<f32> {
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
    saw_digit.then(|| value[..end].parse().ok()).flatten()
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

fn html_legacy_color_hint_value(value: &str) -> Option<String> {
    let trimmed = value.trim_matches(is_html_space);
    if trimmed.eq_ignore_ascii_case("transparent") || trimmed.eq_ignore_ascii_case("currentcolor") {
        return None;
    }
    let color = parse_color(trimmed)?;
    if color.alpha() < 0.999 {
        return None;
    }
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        css_color_channel(color.components()[0]),
        css_color_channel(color.components()[1]),
        css_color_channel(color.components()[2])
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
    if style.text_decoration.clone().has_visible_line() {
        let decoration = style.text_decoration.clone();
        let mut origin_style = style.clone();
        origin_style.text_decoration_layers.clear();
        style.text_decoration_layers.push(TextDecorationLayer {
            decoration,
            origin_style: Rc::new(origin_style),
        });
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
            ComputedLengthPercentage::from_points(margin),
        );
        style.margin.top = margin;
    }
    if let Some(em) = style.ua_margin_em.right.take() {
        let margin = em * style.font_size;
        style.box_values.margin.right = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_points(margin),
        );
        style.margin.right = margin;
    }
    if let Some(em) = style.ua_margin_em.bottom.take() {
        let margin = em * style.font_size;
        style.box_values.margin.bottom = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_points(margin),
        );
        style.margin.bottom = margin;
    }
    if let Some(em) = style.ua_margin_em.left.take() {
        let margin = em * style.font_size;
        style.box_values.margin.left = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_points(margin),
        );
        style.margin.left = margin;
    }
}

pub(crate) fn apply_pseudo_rules_with_parent_ch_advance(
    style: &mut ComputedStyle,
    current: &ElementSignature,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    parent_ch_advance: LayoutLength,
) {
    let mut cascade = ElementCascadeContext::new(current, stylesheets, ancestors);
    apply_pseudo_rules_with_context(style, stylesheets, &mut cascade, parent_ch_advance);
    suppress_generated_pseudos_for_html_replaced_control(style, current);
}

/// Suppress generated-content pseudo-elements on HTML controls rendered as
/// replaced elements.
///
/// HTML's rendering rules treat `input` and `textarea` as replaced for CSS
/// rendering. CSS Pseudo-Elements therefore suppresses their `::before` and
/// `::after` boxes, even when selectors and the cascade would otherwise
/// produce generated content:
/// <https://html.spec.whatwg.org/multipage/dom.html#rendering>
/// <https://drafts.csswg.org/css-pseudo-4/#generated-content>
fn suppress_generated_pseudos_for_html_replaced_control(
    style: &mut ComputedStyle,
    current: &ElementSignature,
) {
    let is_html_element =
        current.document_is_html || current.namespace_url == "http://www.w3.org/1999/xhtml";
    if is_html_element && matches!(current.tag.as_str(), "input" | "textarea") {
        style.before_style = None;
        style.after_style = None;
    }
}

fn apply_pseudo_rules_with_context<'a>(
    style: &mut ComputedStyle,
    stylesheets: &'a Stylesheets<'a>,
    cascade: &mut ElementCascadeContext<'a>,
    parent_ch_advance: LayoutLength,
) {
    apply_marker_rules_with_context(style, stylesheets, cascade, parent_ch_advance);
    apply_generated_pseudo_rules_with_context(style, stylesheets, cascade, parent_ch_advance);
    apply_footnote_pseudo_rules_with_context(style, stylesheets, cascade, parent_ch_advance);
    apply_typographic_pseudo_rules_with_context(style, stylesheets, cascade, parent_ch_advance);
}

fn apply_marker_rules_with_context<'a>(
    style: &mut ComputedStyle,
    stylesheets: &'a Stylesheets<'a>,
    cascade: &mut ElementCascadeContext<'a>,
    parent_ch_advance: LayoutLength,
) {
    apply_marker_rules_from_rule_set(
        style,
        stylesheets,
        cascade,
        parent_ch_advance,
        |stylesheet| &stylesheet.marker_rules,
    );
}

fn apply_marker_rules_from_rule_set<'a>(
    style: &mut ComputedStyle,
    stylesheets: &'a Stylesheets<'a>,
    cascade: &mut ElementCascadeContext<'a>,
    parent_ch_advance: LayoutLength,
    rule_set: fn(&Stylesheet) -> &[StyleRule],
) {
    cascade.collect_matching_rules(stylesheets, rule_set);
    if cascade.matching_rules.is_empty() && !style.display.is_list_item() {
        return;
    }

    // CSS Pseudo-Elements 4 and CSS Lists 3 model `::marker` as a generated
    // marker box with a restricted property set. The marker starts from the
    // originating element's inherited font/color state, then marker rules
    // cascade onto that pseudo-element style.
    // https://www.w3.org/TR/css-pseudo-4/#marker-pseudo
    // https://www.w3.org/TR/css-lists-3/#marker-properties
    let mut marker_style = pseudo_inherited_base_style(style);
    marker_style.marker_style = None;
    marker_style.before_style = None;
    marker_style.after_style = None;
    marker_style.footnote_call_style = None;
    marker_style.footnote_marker_style = None;
    marker_style.first_line_style = None;
    marker_style.first_letter_style = None;
    marker_style.display = marker_style.display.with_list_item(false);
    marker_style.unicode_bidi = UnicodeBidi::Isolate;
    marker_style.white_space = WhiteSpace::Pre;
    marker_style.text_transform = TextTransform::NONE;
    cascade.rebuild_cascaded_declarations();
    sort_cascaded_declarations(&mut cascade.cascaded_declarations);
    apply_cascaded_marker_declarations_with_inheritance_source_and_parent_ch_advance(
        &mut marker_style,
        &cascade.cascaded_declarations,
        style,
        parent_ch_advance,
        stylesheets.color_scheme_preference(),
    );
    select_style_image_sets(&mut marker_style, stylesheets.image_set_resolution_dppx());
    marker_style
        .quotes
        .resolve_auto_language(style.language.as_deref());
    style.marker_style = Some(Box::new(marker_style));
}

fn apply_generated_pseudo_rules_with_context<'a>(
    style: &mut ComputedStyle,
    stylesheets: &'a Stylesheets<'a>,
    cascade: &mut ElementCascadeContext<'a>,
    parent_ch_advance: LayoutLength,
) {
    style.before_style = generated_pseudo_style_with_context(
        style,
        stylesheets,
        cascade,
        |stylesheet| &stylesheet.before_rules,
        |stylesheet| &stylesheet.before_marker_rules,
        parent_ch_advance,
    )
    .map(Box::new);
    style.after_style = generated_pseudo_style_with_context(
        style,
        stylesheets,
        cascade,
        |stylesheet| &stylesheet.after_rules,
        |stylesheet| &stylesheet.after_marker_rules,
        parent_ch_advance,
    )
    .map(Box::new);
}

fn generated_pseudo_style_with_context<'a>(
    originating_style: &ComputedStyle,
    stylesheets: &'a Stylesheets<'a>,
    cascade: &mut ElementCascadeContext<'a>,
    rule_set: fn(&Stylesheet) -> &[StyleRule],
    marker_rule_set: fn(&Stylesheet) -> &[StyleRule],
    parent_ch_advance: LayoutLength,
) -> Option<ComputedStyle> {
    cascade.collect_matching_rules(stylesheets, rule_set);
    if cascade.matching_rules.is_empty() {
        return None;
    }

    // CSS Pseudo-Elements 4: `::before`/`::after` are generated boxes whose
    // styles inherit from their originating element, then cascade pseudo rules.
    // https://www.w3.org/TR/css-pseudo-4/#generated-content
    let mut pseudo_style = pseudo_inherited_base_style(originating_style);
    pseudo_style.content = Content::None;
    pseudo_style.display = Display::INLINE;
    pseudo_style.before_style = None;
    pseudo_style.after_style = None;
    pseudo_style.marker_style = None;
    pseudo_style.footnote_call_style = None;
    pseudo_style.footnote_marker_style = None;
    pseudo_style.first_line_style = None;
    pseudo_style.first_letter_style = None;
    pseudo_style.counter_resets.clear();
    pseudo_style.counter_increments.clear();
    pseudo_style.counter_sets.clear();
    cascade.rebuild_cascaded_declarations();
    sort_cascaded_declarations(&mut cascade.cascaded_declarations);
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
        &mut pseudo_style,
        &cascade.cascaded_declarations,
        originating_style,
        parent_ch_advance,
        false,
        stylesheets.color_scheme_preference(),
    );
    select_style_image_sets(&mut pseudo_style, stylesheets.image_set_resolution_dppx());
    pseudo_style
        .quotes
        .resolve_auto_language(originating_style.language.as_deref());
    apply_marker_rules_from_rule_set(
        &mut pseudo_style,
        stylesheets,
        cascade,
        parent_ch_advance,
        marker_rule_set,
    );
    pseudo_style.content.is_generated().then_some(pseudo_style)
}

fn apply_footnote_pseudo_rules_with_context<'a>(
    style: &mut ComputedStyle,
    stylesheets: &'a Stylesheets<'a>,
    cascade: &mut ElementCascadeContext<'a>,
    parent_ch_advance: LayoutLength,
) {
    if style.float != Float::Footnote {
        style.footnote_call_style = None;
        style.footnote_marker_style = None;
        return;
    }

    // GCPM defines both pseudo-elements for every `float: footnote` element;
    // unlike ::before/::after their counter content exists without an author
    // rule.  Author declarations cascade over that generated default.
    // https://www.w3.org/TR/css-gcpm-3/#footnotes
    style.footnote_call_style = Some(Box::new(footnote_pseudo_style_with_context(
        style,
        stylesheets,
        cascade,
        |stylesheet| &stylesheet.footnote_call_rules,
        Content::List {
            parts: vec![GeneratedContentPart::Counter {
                name: "footnote".to_string(),
                style: None,
            }],
            alt: None,
        },
        parent_ch_advance,
    )));
    style.footnote_marker_style = Some(Box::new(footnote_pseudo_style_with_context(
        style,
        stylesheets,
        cascade,
        |stylesheet| &stylesheet.footnote_marker_rules,
        Content::List {
            parts: vec![
                GeneratedContentPart::Counter {
                    name: "footnote".to_string(),
                    style: None,
                },
                GeneratedContentPart::Text(". ".to_string()),
            ],
            alt: None,
        },
        parent_ch_advance,
    )));
}

fn footnote_pseudo_style_with_context<'a>(
    originating_style: &ComputedStyle,
    stylesheets: &'a Stylesheets<'a>,
    cascade: &mut ElementCascadeContext<'a>,
    rule_set: fn(&Stylesheet) -> &[StyleRule],
    default_content: Content,
    parent_ch_advance: LayoutLength,
) -> ComputedStyle {
    let mut pseudo_style = pseudo_inherited_base_style(originating_style);
    pseudo_style.content = default_content;
    pseudo_style.display = Display::INLINE;
    // The call and marker are generated inline boxes, not footnote floats.
    pseudo_style.float = Float::None;
    pseudo_style.before_style = None;
    pseudo_style.after_style = None;
    pseudo_style.marker_style = None;
    pseudo_style.footnote_call_style = None;
    pseudo_style.footnote_marker_style = None;
    pseudo_style.first_line_style = None;
    pseudo_style.first_letter_style = None;
    pseudo_style.counter_resets.clear();
    pseudo_style.counter_increments.clear();
    pseudo_style.counter_sets.clear();
    cascade.collect_matching_rules(stylesheets, rule_set);
    cascade.rebuild_cascaded_declarations();
    sort_cascaded_declarations(&mut cascade.cascaded_declarations);
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
        &mut pseudo_style,
        &cascade.cascaded_declarations,
        originating_style,
        parent_ch_advance,
        false,
        stylesheets.color_scheme_preference(),
    );
    pseudo_style
        .quotes
        .resolve_auto_language(originating_style.language.as_deref());
    pseudo_style
}

fn apply_typographic_pseudo_rules_with_context<'a>(
    style: &mut ComputedStyle,
    stylesheets: &'a Stylesheets<'a>,
    cascade: &mut ElementCascadeContext<'a>,
    parent_ch_advance: LayoutLength,
) {
    style.first_line_style = typographic_pseudo_style_with_context(
        style,
        stylesheets,
        cascade,
        |stylesheet| &stylesheet.first_line_rules,
        is_first_line_allowed_property,
        parent_ch_advance,
    )
    .map(Box::new);
    style.first_letter_style = typographic_pseudo_style_with_context(
        style,
        stylesheets,
        cascade,
        |stylesheet| &stylesheet.first_letter_rules,
        is_first_letter_allowed_property,
        parent_ch_advance,
    )
    .map(Box::new);
}

fn typographic_pseudo_style_with_context<'a>(
    originating_style: &ComputedStyle,
    stylesheets: &'a Stylesheets<'a>,
    cascade: &mut ElementCascadeContext<'a>,
    rule_set: fn(&Stylesheet) -> &[StyleRule],
    allows_property: fn(&str) -> bool,
    parent_ch_advance: LayoutLength,
) -> Option<ComputedStyle> {
    cascade.collect_matching_rules(stylesheets, rule_set);
    if cascade.matching_rules.is_empty() {
        return None;
    }

    // CSS Pseudo-Elements 4 models `::first-line` and `::first-letter` as
    // tree-abiding typographic pseudo-elements that inherit from their
    // originating element before pseudo-element rules cascade.
    // https://www.w3.org/TR/css-pseudo-4/#first-line-pseudo
    // https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo
    let mut pseudo_style = pseudo_inherited_base_style(originating_style);
    pseudo_style.before_style = None;
    pseudo_style.after_style = None;
    pseudo_style.marker_style = None;
    pseudo_style.footnote_call_style = None;
    pseudo_style.footnote_marker_style = None;
    pseudo_style.first_line_style = None;
    pseudo_style.first_letter_style = None;
    pseudo_style.counter_resets.clear();
    pseudo_style.counter_increments.clear();
    pseudo_style.counter_sets.clear();
    cascade.rebuild_cascaded_declarations();
    sort_cascaded_declarations(&mut cascade.cascaded_declarations);
    cascade.cascaded_declarations.retain(|declaration| {
        declaration.name.starts_with("--") || allows_property(declaration.name.as_ref())
    });
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
        &mut pseudo_style,
        &cascade.cascaded_declarations,
        originating_style,
        parent_ch_advance,
        false,
        stylesheets.color_scheme_preference(),
    );
    pseudo_style
        .quotes
        .resolve_auto_language(originating_style.language.as_deref());
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
                | "ruby-position"
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
        || matches!(
            name,
            "box-shadow"
                | "clear"
                | "float"
                | "initial-letter"
                | "initial-letter-align"
                | "initial-letter-wrap"
        )
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

    fn animation() -> Self {
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
fn global_layer_order(stylesheets: &Stylesheets<'_>) -> HashMap<String, usize> {
    let mut result = HashMap::new();
    for stylesheet in stylesheets.iter() {
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
        .cloned()
}
