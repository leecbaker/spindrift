use super::*;

/// Returns whether a modeled property inherits by default.
///
/// The inherited-property decision is property-specific in each CSS module.
/// This list covers the properties currently represented in `ComputedStyle`:
/// <https://www.w3.org/TR/css-cascade-5/#inheritance>.
pub(super) fn property_is_inherited(name: &str) -> bool {
    matches!(
        name,
        "border-collapse"
            | "border-spacing"
            | "caption-side"
            | "color"
            | "direction"
            | "dominant-baseline"
            | "empty-cells"
            | "font-family"
            | "font-feature-settings"
            | "font-kerning"
            | "font-size"
            | "font-size-adjust"
            | "font-style"
            | "font-stretch"
            | "font-variant-alternates"
            | "font-variant-caps"
            | "font-variant-east-asian"
            | "font-variant-emoji"
            | "font-variant-ligatures"
            | "font-variant-numeric"
            | "font-variant-position"
            | "font-width"
            | "font-weight"
            | "hyphenate-limit-chars"
            | "hyphens"
            | "letter-spacing"
            | "line-break"
            | "line-height"
            | "list-style-image"
            | "list-style-position"
            | "list-style-type"
            | "marker-side"
            | "orphans"
            | "overflow-wrap"
            | "quotes"
            | "text-align"
            | "text-align-all"
            | "text-align-last"
            | "text-justify"
            | "text-autospace"
            | "text-orientation"
            | "text-decoration-skip-ink"
            | "text-decoration-skip-self"
            | "text-decoration-skip-box"
            | "text-decoration-skip-spaces"
            | "text-underline-offset"
            | "text-underline-position"
            | "text-emphasis-color"
            | "text-emphasis-position"
            | "text-emphasis-skip"
            | "text-emphasis-style"
            | "text-shadow"
            | "text-indent"
            | "hanging-punctuation"
            | "text-transform"
            | "tab-size"
            | "visibility"
            | "white-space"
            | "widows"
            | "word-break"
            | "word-spacing"
            | "word-wrap"
            | "writing-mode"
    )
}

pub(super) const ALL_MODELED_LONGHANDS: &[&str] = &[
    "display",
    "flex-direction",
    "justify-content",
    "justify-items",
    "justify-self",
    "align-content",
    "align-items",
    "align-self",
    "place-content",
    "place-items",
    "place-self",
    "flex-wrap",
    "flex-grow",
    "flex-shrink",
    "flex-basis",
    "order",
    "row-gap",
    "column-gap",
    "column-count",
    "column-width",
    "margin-trim",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-style",
    "border-right-style",
    "border-bottom-style",
    "border-left-style",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
    "corner-top-left-shape",
    "corner-top-right-shape",
    "corner-bottom-right-shape",
    "corner-bottom-left-shape",
    "border-image-source",
    "border-image-slice",
    "border-image-width",
    "border-image-outset",
    "border-image-repeat",
    "border-collapse",
    "caption-side",
    "table-layout",
    "empty-cells",
    "border-spacing",
    "background-color",
    "background-image",
    "background-size",
    "background-position",
    "background-repeat",
    "background-origin",
    "background-clip",
    "box-shadow",
    "color",
    "direction",
    "unicode-bidi",
    "writing-mode",
    "text-orientation",
    "font-size",
    "font-size-adjust",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "width",
    "height",
    "inline-size",
    "block-size",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "min-inline-size",
    "max-inline-size",
    "min-block-size",
    "max-block-size",
    "box-sizing",
    "left",
    "top",
    "right",
    "bottom",
    "position",
    "float",
    "clear",
    "z-index",
    "opacity",
    "transform",
    "transform-origin",
    "isolation",
    "mix-blend-mode",
    "filter",
    "clip-path",
    "mask",
    "mask-image",
    "contain",
    "content-visibility",
    "will-change",
    "text-align-all",
    "text-align-last",
    "text-justify",
    "text-autospace",
    "text-indent",
    "hanging-punctuation",
    "vertical-align",
    "dominant-baseline",
    "alignment-baseline",
    "baseline-source",
    "baseline-shift",
    "font-weight",
    "font-style",
    "font-width",
    "font-family",
    "font-feature-settings",
    "font-kerning",
    "font-variant-ligatures",
    "font-variant-position",
    "font-variant-caps",
    "font-variant-numeric",
    "font-variant-alternates",
    "font-variant-east-asian",
    "font-variant-emoji",
    "bookmark-level",
    "bookmark-label",
    "bookmark-state",
    "text-transform",
    "tab-size",
    "visibility",
    "list-style-type",
    "list-style-position",
    "list-style-image",
    "marker-side",
    "counter-reset",
    "counter-increment",
    "counter-set",
    "string-set",
    "page",
    "break-before",
    "break-after",
    "break-inside",
    "orphans",
    "widows",
    "text-decoration-line",
    "text-decoration-style",
    "text-decoration-color",
    "text-decoration-thickness",
    "text-decoration-inset",
    "text-decoration-skip-ink",
    "text-decoration-skip-self",
    "text-decoration-skip-box",
    "text-decoration-skip-spaces",
    "text-underline-offset",
    "text-underline-position",
    "text-emphasis",
    "text-emphasis-style",
    "text-emphasis-color",
    "text-emphasis-position",
    "text-emphasis-skip",
    "text-shadow",
    "white-space",
    "word-break",
    "overflow",
    "overflow-x",
    "overflow-y",
    "overflow-wrap",
    "line-break",
    "hyphens",
    "hyphenate-limit-chars",
    "content",
    "quotes",
];

/// Builds a child style's pre-cascade inherited base from its parent.
///
/// CSS Cascade defines inheritance as each inherited property taking the
/// parent's computed value when no cascaded value applies. This helper keeps
/// that default inheritance path aligned with `inherit`/`unset` defaulting:
/// <https://www.w3.org/TR/css-cascade-5/#inheritance>.
pub(super) fn inherited_base_style(parent: &ComputedStyle) -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    style.custom_properties = parent.custom_properties.clone();
    style.language = parent.language.clone();
    style.text_decoration_layers = parent.text_decoration_layers.clone();
    for longhand in ALL_MODELED_LONGHANDS {
        if property_is_inherited(longhand) {
            copy_modeled_property(&mut style, parent, longhand);
        }
    }
    style
}

/// Builds a pseudo-element style's pre-cascade inherited base.
///
/// Pseudo-elements inherit from their originating element, but generated
/// quote content must use the originating element's already computed quote
/// system rather than re-resolving `quotes: auto` against the originating
/// element's own language:
/// <https://www.w3.org/TR/css-pseudo-4/#generated-content> and
/// <https://www.w3.org/TR/css-content-3/#quotes-property>.
pub(super) fn pseudo_inherited_base_style(originating_style: &ComputedStyle) -> ComputedStyle {
    let mut style = inherited_base_style(originating_style);
    style.quotes = originating_style.quotes.clone();
    style
}

pub(super) fn copy_modeled_property(style: &mut ComputedStyle, source: &ComputedStyle, name: &str) {
    match name {
        "display" => style.display = source.display,
        "flex-direction" => style.flex_direction = source.flex_direction,
        "justify-content" => style.justify_content = source.justify_content,
        "justify-items" => style.justify_items = source.justify_items,
        "justify-self" => style.justify_self = source.justify_self,
        "align-content" => style.align_content = source.align_content,
        "align-items" => style.align_items = source.align_items,
        "align-self" => style.align_self = source.align_self,
        "flex-wrap" => style.flex_wrap = source.flex_wrap,
        "flex-grow" => style.flex_grow = source.flex_grow,
        "flex-shrink" => style.flex_shrink = source.flex_shrink,
        "flex-basis" => style.flex_basis = source.flex_basis,
        "order" => style.order = source.order,
        "row-gap" => style.row_gap = source.row_gap,
        "column-gap" => style.column_gap = source.column_gap,
        "column-count" => style.column_count = source.column_count,
        "column-width" => style.column_width = source.column_width,
        "margin-trim" => style.margin_trim = source.margin_trim,
        "margin-top" => {
            style.box_values.margin.top = source.box_values.margin.top;
            style.margin.top = source.margin.top;
            style.ua_margin_em.top = source.ua_margin_em.top;
        }
        "margin-right" => {
            style.box_values.margin.right = source.box_values.margin.right;
            style.margin.right = source.margin.right;
            style.ua_margin_em.right = source.ua_margin_em.right;
        }
        "margin-bottom" => {
            style.box_values.margin.bottom = source.box_values.margin.bottom;
            style.margin.bottom = source.margin.bottom;
            style.ua_margin_em.bottom = source.ua_margin_em.bottom;
        }
        "margin-left" => {
            style.box_values.margin.left = source.box_values.margin.left;
            style.margin.left = source.margin.left;
            style.ua_margin_em.left = source.ua_margin_em.left;
        }
        "padding-top" => {
            style.box_values.padding.top = source.box_values.padding.top;
            style.padding.top = source.padding.top;
        }
        "padding-right" => {
            style.box_values.padding.right = source.box_values.padding.right;
            style.padding.right = source.padding.right;
        }
        "padding-bottom" => {
            style.box_values.padding.bottom = source.box_values.padding.bottom;
            style.padding.bottom = source.padding.bottom;
        }
        "padding-left" => {
            style.box_values.padding.left = source.box_values.padding.left;
            style.padding.left = source.padding.left;
        }
        "border-block" => {
            for property in [
                "border-top-width",
                "border-bottom-width",
                "border-top-style",
                "border-bottom-style",
                "border-top-color",
                "border-bottom-color",
            ] {
                copy_modeled_property(style, source, property);
            }
        }
        "border-inline" => {
            for property in [
                "border-left-width",
                "border-right-width",
                "border-left-style",
                "border-right-style",
                "border-left-color",
                "border-right-color",
            ] {
                copy_modeled_property(style, source, property);
            }
        }
        "border-block-start" => {
            for property in ["border-top-width", "border-top-style", "border-top-color"] {
                copy_modeled_property(style, source, property);
            }
        }
        "border-block-end" => {
            for property in [
                "border-bottom-width",
                "border-bottom-style",
                "border-bottom-color",
            ] {
                copy_modeled_property(style, source, property);
            }
        }
        "border-inline-start" => {
            for property in [
                "border-left-width",
                "border-left-style",
                "border-left-color",
            ] {
                copy_modeled_property(style, source, property);
            }
        }
        "border-inline-end" => {
            for property in [
                "border-right-width",
                "border-right-style",
                "border-right-color",
            ] {
                copy_modeled_property(style, source, property);
            }
        }
        "border-block-width" => {
            copy_modeled_property(style, source, "border-top-width");
            copy_modeled_property(style, source, "border-bottom-width");
        }
        "border-inline-width" => {
            copy_modeled_property(style, source, "border-left-width");
            copy_modeled_property(style, source, "border-right-width");
        }
        "border-block-style" => {
            copy_modeled_property(style, source, "border-top-style");
            copy_modeled_property(style, source, "border-bottom-style");
        }
        "border-inline-style" => {
            copy_modeled_property(style, source, "border-left-style");
            copy_modeled_property(style, source, "border-right-style");
        }
        "border-block-color" => {
            copy_modeled_property(style, source, "border-top-color");
            copy_modeled_property(style, source, "border-bottom-color");
        }
        "border-inline-color" => {
            copy_modeled_property(style, source, "border-left-color");
            copy_modeled_property(style, source, "border-right-color");
        }
        "border-block-start-width" => copy_modeled_property(style, source, "border-top-width"),
        "border-block-end-width" => copy_modeled_property(style, source, "border-bottom-width"),
        "border-inline-start-width" => copy_modeled_property(style, source, "border-left-width"),
        "border-inline-end-width" => copy_modeled_property(style, source, "border-right-width"),
        "border-block-start-style" => copy_modeled_property(style, source, "border-top-style"),
        "border-block-end-style" => copy_modeled_property(style, source, "border-bottom-style"),
        "border-inline-start-style" => copy_modeled_property(style, source, "border-left-style"),
        "border-inline-end-style" => copy_modeled_property(style, source, "border-right-style"),
        "border-block-start-color" => copy_modeled_property(style, source, "border-top-color"),
        "border-block-end-color" => copy_modeled_property(style, source, "border-bottom-color"),
        "border-inline-start-color" => copy_modeled_property(style, source, "border-left-color"),
        "border-inline-end-color" => copy_modeled_property(style, source, "border-right-color"),
        "border-top-width" => {
            set_border_side_width(style, BorderSide::Top, source.border_widths.top)
        }
        "border-right-width" => {
            set_border_side_width(style, BorderSide::Right, source.border_widths.right)
        }
        "border-bottom-width" => {
            set_border_side_width(style, BorderSide::Bottom, source.border_widths.bottom)
        }
        "border-left-width" => {
            set_border_side_width(style, BorderSide::Left, source.border_widths.left)
        }
        "border-top-style" => style.border_styles.top = source.border_styles.top,
        "border-right-style" => style.border_styles.right = source.border_styles.right,
        "border-bottom-style" => style.border_styles.bottom = source.border_styles.bottom,
        "border-left-style" => style.border_styles.left = source.border_styles.left,
        "border-top-color" => {
            style.border_colors.top = source.border_colors.top;
            style.border_color = source.border_color;
        }
        "border-right-color" => style.border_colors.right = source.border_colors.right,
        "border-bottom-color" => style.border_colors.bottom = source.border_colors.bottom,
        "border-left-color" => style.border_colors.left = source.border_colors.left,
        "border-top-left-radius" => style.border_radius.top_left = source.border_radius.top_left,
        "border-top-right-radius" => {
            style.border_radius.top_right = source.border_radius.top_right;
        }
        "border-bottom-right-radius" => {
            style.border_radius.bottom_right = source.border_radius.bottom_right;
        }
        "border-bottom-left-radius" => {
            style.border_radius.bottom_left = source.border_radius.bottom_left;
        }
        "corner-top-left-shape" => style.corner_shapes.top_left = source.corner_shapes.top_left,
        "corner-top-right-shape" => style.corner_shapes.top_right = source.corner_shapes.top_right,
        "corner-bottom-right-shape" => {
            style.corner_shapes.bottom_right = source.corner_shapes.bottom_right;
        }
        "corner-bottom-left-shape" => {
            style.corner_shapes.bottom_left = source.corner_shapes.bottom_left;
        }
        "border-image-source" => {
            style.border_image.source = source.border_image.source.clone();
            style.border_image.source_base_url = source.border_image.source_base_url.clone();
            style.border_image.source_root_url = source.border_image.source_root_url.clone();
        }
        "border-image-slice" => style.border_image.slice = source.border_image.slice,
        "border-image-width" => style.border_image.width = source.border_image.width,
        "border-image-outset" => style.border_image.outset = source.border_image.outset,
        "border-image-repeat" => style.border_image.repeat = source.border_image.repeat,
        "border-collapse" => style.border_collapse = source.border_collapse,
        "caption-side" => style.caption_side = source.caption_side,
        "table-layout" => style.table_layout = source.table_layout,
        "empty-cells" => style.empty_cells = source.empty_cells,
        "border-spacing" => {
            style.border_spacing = source.border_spacing;
            style.border_spacing_explicit = source.border_spacing_explicit;
        }
        "background-color" => style.background_color = source.background_color,
        "background-image" => {
            style.background_image = source.background_image.clone();
            style.background_layers = source.background_layers.clone();
        }
        "background-size" => {
            style.background_size = source.background_size;
            style.background_layers = source.background_layers.clone();
        }
        "background-position" => {
            style.background_position = source.background_position;
            style.background_layers = source.background_layers.clone();
        }
        "background-repeat" => {
            style.background_repeat = source.background_repeat;
            style.background_layers = source.background_layers.clone();
        }
        "background-origin" => {
            style.background_origin = source.background_origin;
            style.background_layers = source.background_layers.clone();
        }
        "background-clip" => {
            style.background_clip = source.background_clip;
            style.background_layers = source.background_layers.clone();
        }
        "color" => style.color = source.color,
        "direction" => style.direction = source.direction,
        "unicode-bidi" => style.unicode_bidi = source.unicode_bidi,
        "writing-mode" => style.writing_mode = source.writing_mode,
        "text-orientation" => style.text_orientation = source.text_orientation,
        "font-size" => set_font_size(style, source.font_size),
        "font-size-adjust" => style.font_size_adjust = source.font_size_adjust,
        "line-height" => {
            style.line_height_value = source.line_height_value;
            style.line_height = source.line_height;
            style.line_height_multiplier = source.line_height_multiplier;
            style.line_height_is_normal = source.line_height_is_normal;
        }
        "letter-spacing" => style.letter_spacing = source.letter_spacing,
        "word-spacing" => style.word_spacing = source.word_spacing,
        "width" => style.box_values.width = source.box_values.width,
        "height" => style.box_values.height = source.box_values.height,
        "min-width" => style.box_values.min_width = source.box_values.min_width,
        "max-width" => style.box_values.max_width = source.box_values.max_width,
        "min-height" => style.box_values.min_height = source.box_values.min_height,
        "max-height" => style.box_values.max_height = source.box_values.max_height,
        "box-sizing" => style.box_sizing = source.box_sizing,
        "left" => style.box_values.inset_left = source.box_values.inset_left,
        "top" => style.box_values.inset_top = source.box_values.inset_top,
        "right" => style.box_values.inset_right = source.box_values.inset_right,
        "bottom" => style.box_values.inset_bottom = source.box_values.inset_bottom,
        "position" => style.position = source.position,
        "float" => style.float = source.float,
        "clear" => style.clear = source.clear,
        "z-index" => style.z_index = source.z_index,
        "opacity" => style.opacity = source.opacity,
        "transform" => style.transform = source.transform.clone(),
        "transform-origin" => style.transform_origin = source.transform_origin,
        "isolation" => style.isolation = source.isolation,
        "mix-blend-mode" => style.mix_blend_mode = source.mix_blend_mode,
        "filter" => style.filter = source.filter.clone(),
        "clip-path" => style.clip_path = source.clip_path,
        "mask" | "mask-image" => style.mask = source.mask.clone(),
        "contain" => style.contain = source.contain,
        "content-visibility" => style.content_visibility = source.content_visibility,
        "will-change" => style.will_change = source.will_change,
        "text-align" | "text-align-all" => style.text_align = source.text_align,
        "text-align-last" => style.text_align_last = source.text_align_last,
        "text-justify" => style.text_justify = source.text_justify,
        "text-autospace" => style.text_autospace = source.text_autospace,
        "text-indent" => style.text_indent = source.text_indent,
        "hanging-punctuation" => style.hanging_punctuation = source.hanging_punctuation,
        "vertical-align" => style.vertical_align = source.vertical_align,
        "dominant-baseline" => {
            style.vertical_align.dominant_baseline = source.vertical_align.dominant_baseline;
        }
        "alignment-baseline" => {
            style.vertical_align.alignment_baseline = source.vertical_align.alignment_baseline;
        }
        "baseline-source" => {
            style.vertical_align.baseline_source = source.vertical_align.baseline_source;
        }
        "baseline-shift" => {
            style.vertical_align.baseline_shift = source.vertical_align.baseline_shift;
        }
        "font-weight" => style.font_weight = source.font_weight,
        "font-style" => style.font_style = source.font_style,
        "font-width" | "font-stretch" => style.font_width = source.font_width,
        "font-family" => style.font_family = source.font_family.clone(),
        "font-feature-settings" => {
            style.font_feature_settings = source.font_feature_settings.clone();
        }
        "font-kerning" => style.font_kerning = source.font_kerning,
        "font-variant" => {
            style.font_variant_ligatures = source.font_variant_ligatures;
            style.font_variant_position = source.font_variant_position;
            style.font_variant_caps = source.font_variant_caps;
            style.font_variant_numeric = source.font_variant_numeric.clone();
            style.font_variant_alternates = source.font_variant_alternates.clone();
            style.font_variant_east_asian = source.font_variant_east_asian.clone();
            style.font_variant_emoji = source.font_variant_emoji;
        }
        "font-variant-ligatures" => {
            style.font_variant_ligatures = source.font_variant_ligatures;
        }
        "font-variant-position" => style.font_variant_position = source.font_variant_position,
        "font-variant-caps" => style.font_variant_caps = source.font_variant_caps,
        "font-variant-numeric" => {
            style.font_variant_numeric = source.font_variant_numeric.clone();
        }
        "font-variant-alternates" => {
            style.font_variant_alternates = source.font_variant_alternates.clone();
        }
        "font-variant-east-asian" => {
            style.font_variant_east_asian = source.font_variant_east_asian.clone();
        }
        "font-variant-emoji" => style.font_variant_emoji = source.font_variant_emoji,
        "bookmark-level" => style.bookmark_level = source.bookmark_level,
        "bookmark-label" => style.bookmark_label = source.bookmark_label.clone(),
        "bookmark-state" => style.bookmark_state = source.bookmark_state,
        "text-transform" => style.text_transform = source.text_transform,
        "tab-size" => style.tab_size = source.tab_size,
        "visibility" => style.visibility = source.visibility,
        "list-style-type" => style.list_style_type = source.list_style_type.clone(),
        "list-style-position" => style.list_style_position = source.list_style_position,
        "list-style-image" => {
            style.list_style_image = source.list_style_image.clone();
            style.list_style_image_base_url = source.list_style_image_base_url.clone();
            style.list_style_image_root_url = source.list_style_image_root_url.clone();
        }
        "marker-side" => style.marker_side = source.marker_side,
        "counter-reset" => style.counter_resets = source.counter_resets.clone(),
        "counter-increment" => style.counter_increments = source.counter_increments.clone(),
        "counter-set" => style.counter_sets = source.counter_sets.clone(),
        "string-set" => style.string_sets = source.string_sets.clone(),
        "page" => {
            style.page_name = source.page_name.clone();
            style.page_name_specified = source.page_name_specified;
        }
        "break-before" | "page-break-before" => style.break_before = source.break_before,
        "break-after" | "page-break-after" => style.break_after = source.break_after,
        "break-inside" | "page-break-inside" => {
            style.break_inside_avoid = source.break_inside_avoid;
        }
        "orphans" => style.orphans = source.orphans,
        "widows" => style.widows = source.widows,
        "text-decoration-line" | "text-decoration" => {
            style.text_decoration.underline = source.text_decoration.underline;
            style.text_decoration.overline = source.text_decoration.overline;
            style.text_decoration.line_through = source.text_decoration.line_through;
            style.text_decoration.blink = source.text_decoration.blink;
            style.text_decoration.spelling_error = source.text_decoration.spelling_error;
            style.text_decoration.grammar_error = source.text_decoration.grammar_error;
        }
        "text-decoration-style" => style.text_decoration.style = source.text_decoration.style,
        "text-decoration-color" => style.text_decoration.color = source.text_decoration.color,
        "text-decoration-thickness" => {
            style.text_decoration.thickness = source.text_decoration.thickness;
        }
        "text-decoration-inset" => {
            style.text_decoration.inset = source.text_decoration.inset;
        }
        "text-decoration-skip-ink" => {
            style.text_decoration.skip_ink = source.text_decoration.skip_ink;
        }
        "text-decoration-skip-self" => {
            style.text_decoration.skip_self = source.text_decoration.skip_self;
        }
        "text-decoration-skip-box" => {
            style.text_decoration.skip_box = source.text_decoration.skip_box;
        }
        "text-decoration-skip-spaces" => {
            style.text_decoration.skip_spaces = source.text_decoration.skip_spaces;
        }
        "text-underline-offset" => {
            style.text_decoration.underline_offset = source.text_decoration.underline_offset;
        }
        "text-underline-position" => {
            style.text_decoration.underline_position = source.text_decoration.underline_position;
        }
        "text-emphasis-style" => {
            style.text_emphasis_style = source.text_emphasis_style.clone();
        }
        "text-emphasis" => {
            style.text_emphasis_style = source.text_emphasis_style.clone();
            style.text_emphasis_color = source.text_emphasis_color;
        }
        "text-emphasis-color" => style.text_emphasis_color = source.text_emphasis_color,
        "text-emphasis-position" => {
            style.text_emphasis_position = source.text_emphasis_position;
        }
        "text-emphasis-skip" => style.text_emphasis_skip = source.text_emphasis_skip,
        "text-shadow" => style.text_shadow = source.text_shadow.clone(),
        "box-shadow" => style.box_shadow = source.box_shadow.clone(),
        "white-space" => style.white_space = source.white_space,
        "word-break" => style.word_break = source.word_break,
        "overflow" => {
            style.overflow = source.overflow;
            style.overflow_x = source.overflow_x;
            style.overflow_y = source.overflow_y;
        }
        "overflow-x" => style.overflow_x = source.overflow_x,
        "overflow-y" => style.overflow_y = source.overflow_y,
        "overflow-wrap" | "word-wrap" => style.overflow_wrap = source.overflow_wrap,
        "line-break" => style.line_break = source.line_break,
        "hyphens" => style.hyphens = source.hyphens,
        "hyphenate-limit-chars" => {
            style.hyphenate_limit_chars = source.hyphenate_limit_chars;
        }
        "content" => {
            style.content = source.content.clone();
            style.marker_content = source.marker_content.clone();
        }
        "quotes" => style.quotes = source.quotes.inherited(),
        _ => {}
    }
}
