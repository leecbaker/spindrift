use super::*;
use crate::css::cascade::declarations::affected_longhands;

/// Whether the cascade owns the named property or shorthand.
///
/// This is the property identity boundary shared by declaration application
/// and CSS Conditional feature queries.  It deliberately uses the cascade's
/// longhand and shorthand model instead of a second hand-maintained list.
pub(in crate::css) fn is_modeled_property_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("all")
        || ALL_MODELED_LONGHANDS.contains(&name)
        || affected_longhands(name, Direction::Ltr, WritingMode::HorizontalTb).is_some()
}

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
            | "forced-color-adjust"
            | "fill"
            | "stroke"
            | "stroke-width"
            | "-webkit-text-fill-color"
            | "direction"
            | "dominant-baseline"
            | "empty-cells"
            | "font-family"
            | "font-feature-settings"
            | "font-variation-settings"
            | "font-palette"
            | "font-synthesis"
            | "font-synthesis-weight"
            | "font-synthesis-style"
            | "font-synthesis-small-caps"
            | "font-synthesis-position"
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
            | "hyphenate-character"
            | "hyphenate-limit-chars"
            | "hyphens"
            | "image-rendering"
            | "image-orientation"
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
            | "text-combine-upright"
            | "text-align-all"
            | "text-align-last"
            | "text-justify"
            | "text-autospace"
            | "text-spacing-trim"
            | "word-space-transform"
            | "initial-letter-align"
            | "initial-letter-wrap"
            | "line-fit-edge"
            | "text-box-edge"
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
            | "text-wrap"
            | "text-wrap-mode"
            | "text-wrap-style"
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

pub(in crate::css) const ALL_MODELED_LONGHANDS: &[&str] = &[
    "zoom",
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
    "row-rule-width",
    "row-rule-style",
    "row-rule-color",
    "row-rule-break",
    "row-rule-visibility-items",
    "row-rule-inset-cap-start",
    "row-rule-inset-cap-end",
    "row-rule-inset-junction-start",
    "row-rule-inset-junction-end",
    "column-rule-width",
    "column-rule-style",
    "column-rule-color",
    "column-rule-break",
    "column-rule-visibility-items",
    "column-rule-inset-cap-start",
    "column-rule-inset-cap-end",
    "column-rule-inset-junction-start",
    "column-rule-inset-junction-end",
    "rule-overlap",
    "grid-template-rows",
    "grid-template-columns",
    "grid-template-areas",
    "grid-auto-rows",
    "grid-auto-columns",
    "grid-auto-flow",
    "grid-lanes-direction",
    "flow-tolerance",
    "grid-row-start",
    "grid-row-end",
    "grid-column-start",
    "grid-column-end",
    "column-count",
    "column-width",
    "column-fill",
    "column-span",
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
    "shape-outside",
    "shape-margin",
    "shape-image-threshold",
    "border-shape",
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
    "background-position-x",
    "background-position-y",
    "background-repeat",
    "background-origin",
    "background-clip",
    "object-fit",
    "object-view-box",
    "object-position",
    "image-rendering",
    "image-orientation",
    "box-decoration-break",
    "outline-offset",
    "box-shadow",
    "color",
    "forced-color-adjust",
    "fill",
    "stroke",
    "stroke-width",
    "-webkit-text-fill-color",
    "direction",
    "unicode-bidi",
    "writing-mode",
    "text-orientation",
    "text-combine-upright",
    "line-fit-edge",
    "text-box-trim",
    "text-box-edge",
    "initial-letter",
    "initial-letter-align",
    "initial-letter-wrap",
    "font-size",
    "font-size-adjust",
    "font-synthesis",
    "font-synthesis-weight",
    "font-synthesis-style",
    "font-synthesis-small-caps",
    "font-synthesis-position",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "width",
    "height",
    "aspect-ratio",
    "contain-intrinsic-size",
    "contain-intrinsic-inline-size",
    "contain-intrinsic-block-size",
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
    "footnote-display",
    "footnote-policy",
    "clear",
    "z-index",
    "opacity",
    "transform",
    "translate",
    "rotate",
    "scale",
    "transform-origin",
    "transform-box",
    "isolation",
    "mix-blend-mode",
    "filter",
    "clip-path",
    "mask",
    "mask-image",
    "contain",
    "container-type",
    "container-name",
    "container",
    "content-visibility",
    "will-change",
    "text-align-all",
    "text-align-last",
    "text-justify",
    "text-autospace",
    "text-spacing-trim",
    "word-space-transform",
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
    "font-stretch",
    "font-family",
    "font-feature-settings",
    "font-variation-settings",
    "font-palette",
    "font-synthesis",
    "font-synthesis-weight",
    "font-synthesis-style",
    "font-synthesis-small-caps",
    "font-synthesis-position",
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
    "text-wrap",
    "text-wrap-mode",
    "text-wrap-style",
    "wrap-inside",
    "line-clamp",
    "-webkit-line-clamp",
    "word-break",
    "overflow",
    "overflow-x",
    "overflow-y",
    "scroll-snap-type",
    "scroll-snap-align",
    "scroll-snap-stop",
    "scroll-padding",
    "scroll-padding-top",
    "scroll-padding-right",
    "scroll-padding-bottom",
    "scroll-padding-left",
    "scroll-margin",
    "scroll-margin-top",
    "scroll-margin-right",
    "scroll-margin-bottom",
    "scroll-margin-left",
    "overflow-clip-margin",
    "overflow-wrap",
    "line-break",
    "hyphens",
    "hyphenate-character",
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
    style.root_font_size = parent.root_font_size;
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
        "zoom" => style.zoom = source.zoom,
        "display" => {
            style.display = source.display;
            style.legacy_webkit_box = source.legacy_webkit_box;
        }
        "flex-direction" => style.flex_direction = source.flex_direction,
        "justify-content" => style.justify_content = source.justify_content,
        "justify-items" => style.justify_items = source.justify_items,
        "justify-self" => style.justify_self = source.justify_self,
        "align-content" => style.align_content = source.align_content,
        "align-items" => style.align_items = source.align_items,
        "align-self" => style.align_self = source.align_self,
        "flex-wrap" => style.flex_wrap = source.flex_wrap,
        "flex-line-count" => style.flex_line_count = source.flex_line_count,
        "flex-grow" => style.flex_grow = source.flex_grow,
        "flex-shrink" => style.flex_shrink = source.flex_shrink,
        "flex-basis" => style.flex_basis = source.flex_basis.clone(),
        "order" => style.order = source.order,
        "row-gap" => style.row_gap = source.row_gap.clone(),
        "column-gap" => style.column_gap = source.column_gap.clone(),
        "row-rule-width" => style.row_rule.widths = source.row_rule.clone().widths,
        "row-rule-style" => style.row_rule.styles = source.row_rule.clone().styles,
        "row-rule-color" => style.row_rule.colors = source.row_rule.clone().colors,
        "row-rule-break" => style.row_rule.rule_break = source.row_rule.clone().rule_break,
        "row-rule-visibility-items" => {
            style.row_rule.visibility_items = source.row_rule.clone().visibility_items;
        }
        "row-rule-inset-cap-start" => {
            style.row_rule.inset_cap_start = source.row_rule.clone().inset_cap_start;
        }
        "row-rule-inset-cap-end" => {
            style.row_rule.inset_cap_end = source.row_rule.clone().inset_cap_end;
        }
        "row-rule-inset-junction-start" => {
            style.row_rule.inset_junction_start = source.row_rule.clone().inset_junction_start;
        }
        "row-rule-inset-junction-end" => {
            style.row_rule.inset_junction_end = source.row_rule.clone().inset_junction_end;
        }
        "column-rule-width" => style.column_rule.widths = source.column_rule.clone().widths,
        "column-rule-style" => style.column_rule.styles = source.column_rule.clone().styles,
        "column-rule-color" => style.column_rule.colors = source.column_rule.clone().colors,
        "column-rule-break" => style.column_rule.rule_break = source.column_rule.clone().rule_break,
        "column-rule-visibility-items" => {
            style.column_rule.visibility_items = source.column_rule.clone().visibility_items;
        }
        "column-rule-inset-cap-start" => {
            style.column_rule.inset_cap_start = source.column_rule.clone().inset_cap_start;
        }
        "column-rule-inset-cap-end" => {
            style.column_rule.inset_cap_end = source.column_rule.clone().inset_cap_end;
        }
        "column-rule-inset-junction-start" => {
            style.column_rule.inset_junction_start =
                source.column_rule.clone().inset_junction_start;
        }
        "column-rule-inset-junction-end" => {
            style.column_rule.inset_junction_end = source.column_rule.clone().inset_junction_end;
        }
        "rule-overlap" => style.rule_overlap = source.rule_overlap,
        "grid-template-rows" => style.grid_template_rows = source.grid_template_rows.clone(),
        "grid-template-columns" => {
            style.grid_template_columns = source.grid_template_columns.clone();
        }
        "grid-template-areas" => style.grid_template_areas = source.grid_template_areas.clone(),
        "grid-auto-rows" => style.grid_auto_rows = source.grid_auto_rows.clone(),
        "grid-auto-columns" => style.grid_auto_columns = source.grid_auto_columns.clone(),
        "grid-auto-flow" => style.grid_auto_flow = source.grid_auto_flow,
        "grid-lanes-direction" => style.grid_lanes_direction = source.grid_lanes_direction,
        "flow-tolerance" => {
            style.grid_lanes_flow_tolerance = source.grid_lanes_flow_tolerance.clone()
        }
        "grid-row-start" => style.grid_row_start = source.grid_row_start.clone(),
        "grid-row-end" => style.grid_row_end = source.grid_row_end.clone(),
        "grid-column-start" => style.grid_column_start = source.grid_column_start.clone(),
        "grid-column-end" => style.grid_column_end = source.grid_column_end.clone(),
        "column-count" => style.column_count = source.column_count,
        "column-width" => style.column_width = source.column_width.clone(),
        "column-height" => style.column_height = source.column_height.clone(),
        "column-wrap" => style.column_wrap = source.column_wrap,
        "column-fill" => style.column_fill = source.column_fill,
        "column-span" => style.column_span = source.column_span,
        "margin-trim" => style.margin_trim = source.margin_trim,
        "margin-top" => {
            style.box_values.margin.top = source.box_values.clone().margin.top;
            style.margin.top = source.margin.top;
            style.ua_margin_em.top = source.ua_margin_em.top;
        }
        "margin-right" => {
            style.box_values.margin.right = source.box_values.clone().margin.right;
            style.margin.right = source.margin.right;
            style.ua_margin_em.right = source.ua_margin_em.right;
        }
        "margin-bottom" => {
            style.box_values.margin.bottom = source.box_values.clone().margin.bottom;
            style.margin.bottom = source.margin.bottom;
            style.ua_margin_em.bottom = source.ua_margin_em.bottom;
        }
        "margin-left" => {
            style.box_values.margin.left = source.box_values.clone().margin.left;
            style.margin.left = source.margin.left;
            style.ua_margin_em.left = source.ua_margin_em.left;
        }
        "padding-top" => {
            style.box_values.padding.top = source.box_values.clone().padding.top;
            style.padding.top = source.padding.top;
        }
        "padding-right" => {
            style.box_values.padding.right = source.box_values.clone().padding.right;
            style.padding.right = source.padding.right;
        }
        "padding-bottom" => {
            style.box_values.padding.bottom = source.box_values.clone().padding.bottom;
            style.padding.bottom = source.padding.bottom;
        }
        "padding-left" => {
            style.box_values.padding.left = source.box_values.clone().padding.left;
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
        "border-top-width" => set_border_side_width(
            style,
            BorderSide::Top,
            source.border_width_values.clone().top,
        ),
        "border-right-width" => set_border_side_width(
            style,
            BorderSide::Right,
            source.border_width_values.clone().right,
        ),
        "border-bottom-width" => set_border_side_width(
            style,
            BorderSide::Bottom,
            source.border_width_values.clone().bottom,
        ),
        "border-left-width" => set_border_side_width(
            style,
            BorderSide::Left,
            source.border_width_values.clone().left,
        ),
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
        "border-top-left-radius" => {
            style.border_radius.top_left = source.border_radius.clone().top_left
        }
        "border-top-right-radius" => {
            style.border_radius.top_right = source.border_radius.clone().top_right;
        }
        "border-bottom-right-radius" => {
            style.border_radius.bottom_right = source.border_radius.clone().bottom_right;
        }
        "border-bottom-left-radius" => {
            style.border_radius.bottom_left = source.border_radius.clone().bottom_left;
        }
        "corner-top-left-shape" => style.corner_shapes.top_left = source.corner_shapes.top_left,
        "corner-top-right-shape" => style.corner_shapes.top_right = source.corner_shapes.top_right,
        "corner-bottom-right-shape" => {
            style.corner_shapes.bottom_right = source.corner_shapes.bottom_right;
        }
        "corner-bottom-left-shape" => {
            style.corner_shapes.bottom_left = source.corner_shapes.bottom_left;
        }
        "border-shape" => style.border_shape = source.border_shape.clone(),
        "shape-outside" => style.shape_outside = source.shape_outside.clone(),
        "shape-margin" => style.shape_margin = source.shape_margin.clone(),
        "shape-image-threshold" => style.shape_image_threshold = source.shape_image_threshold,
        "border-image-source" => {
            style.border_image.source = source.border_image.clone().source;
            style.border_image.source_base_url = source.border_image.clone().source_base_url;
            style.border_image.source_root_url = source.border_image.clone().source_root_url;
        }
        "border-image-slice" => style.border_image.slice = source.border_image.clone().slice,
        "border-image-width" => style.border_image.width = source.border_image.clone().width,
        "border-image-outset" => style.border_image.outset = source.border_image.clone().outset,
        "border-image-repeat" => style.border_image.repeat = source.border_image.clone().repeat,
        "border-collapse" => style.border_collapse = source.border_collapse,
        "caption-side" => style.caption_side = source.caption_side,
        "table-layout" => style.table_layout = source.table_layout,
        "empty-cells" => style.empty_cells = source.empty_cells,
        "border-spacing" => {
            style.border_spacing = source.border_spacing.clone();
            style.border_spacing_explicit = source.border_spacing_explicit;
        }
        "background-color" => {
            style.background_color = source.background_color;
            style.background_color_is_current_color = source.background_color_is_current_color;
            style.background_color_current_color_expression =
                source.background_color_current_color_expression.clone();
        }
        "background-image" => {
            style.background_image = source.background_image.clone();
            style.background_layers = source.background_layers.clone();
            style.background_image_layer_count = source.background_image_layer_count;
        }
        "background-size" => {
            style.background_size = source.background_size.clone();
            for (index, layer) in style.background_layers.iter_mut().enumerate() {
                layer.size = source
                    .background_layers
                    .get(index % source.background_layers.clone().len().max(1))
                    .map(|layer| layer.size.clone())
                    .unwrap_or(source.background_size.clone());
            }
        }
        "object-fit" => style.object_fit = source.object_fit,
        "object-view-box" => style.object_view_box = source.object_view_box.clone(),
        "image-rendering" => style.image_rendering = source.image_rendering,
        "image-orientation" => style.image_orientation = source.image_orientation,
        "object-position" => style.object_position = source.object_position.clone(),
        "background-position" => {
            style.background_position = source.background_position.clone();
            for (index, layer) in style.background_layers.iter_mut().enumerate() {
                layer.position = source
                    .background_layers
                    .get(index % source.background_layers.clone().len().max(1))
                    .map(|layer| layer.position.clone())
                    .unwrap_or(source.background_position.clone());
            }
        }
        "background-position-x" => {
            style.background_position.x = source.background_position.clone().x;
            for (index, layer) in style.background_layers.iter_mut().enumerate() {
                layer.position.x = source
                    .background_layers
                    .get(index % source.background_layers.clone().len().max(1))
                    .map(|layer| layer.position.x.clone())
                    .unwrap_or(source.background_position.clone().x);
            }
        }
        "background-position-y" => {
            style.background_position.y = source.background_position.clone().y;
            for (index, layer) in style.background_layers.iter_mut().enumerate() {
                layer.position.y = source
                    .background_layers
                    .get(index % source.background_layers.clone().len().max(1))
                    .map(|layer| layer.position.y.clone())
                    .unwrap_or(source.background_position.clone().y);
            }
        }
        "background-repeat" => {
            style.background_repeat = source.background_repeat;
            for (index, layer) in style.background_layers.iter_mut().enumerate() {
                layer.repeat = source
                    .background_layers
                    .get(index % source.background_layers.clone().len().max(1))
                    .map(|layer| layer.repeat)
                    .unwrap_or(source.background_repeat);
            }
        }
        "background-origin" => {
            style.background_origin = source.background_origin;
            for (index, layer) in style.background_layers.iter_mut().enumerate() {
                layer.origin = source
                    .background_layers
                    .get(index % source.background_layers.clone().len().max(1))
                    .map(|layer| layer.origin)
                    .unwrap_or(source.background_origin);
            }
        }
        "background-clip" => {
            style.background_clip = source.background_clip;
            for (index, layer) in style.background_layers.iter_mut().enumerate() {
                layer.clip = source
                    .background_layers
                    .get(index % source.background_layers.clone().len().max(1))
                    .map(|layer| layer.clip)
                    .unwrap_or(source.background_clip);
            }
        }
        "color" => style.color = source.color,
        "forced-color-adjust" => style.forced_color_adjust = source.forced_color_adjust,
        "fill" => {
            style.svg_fill = source.svg_fill;
            style.svg_fill_is_current_color = source.svg_fill_is_current_color;
            style.svg_fill_overridden = source.svg_fill_overridden;
        }
        "stroke" => {
            style.svg_stroke = source.svg_stroke;
            style.svg_stroke_is_current_color = source.svg_stroke_is_current_color;
            style.svg_stroke_overridden = source.svg_stroke_overridden;
        }
        "stroke-width" => {
            style.svg_stroke_width = source.svg_stroke_width.clone();
            style.svg_stroke_width_overridden = source.svg_stroke_width_overridden;
        }
        "-webkit-text-fill-color" => style.text_fill_color = source.text_fill_color,
        "direction" => style.direction = source.direction,
        "unicode-bidi" => style.unicode_bidi = source.unicode_bidi,
        "writing-mode" => style.writing_mode = source.writing_mode,
        "text-orientation" => style.text_orientation = source.text_orientation,
        "text-combine-upright" => style.text_combine_upright = source.text_combine_upright,
        "font-size" => {
            style.font_size = source.font_size;
            style.deferred_font_size = DeferredFontSize::Inherit;
            project_line_height(style);
        }
        "font-size-adjust" => style.font_size_adjust = source.font_size_adjust,
        "font-synthesis" => style.font_synthesis = source.font_synthesis,
        "font-synthesis-weight" => style.font_synthesis.weight = source.font_synthesis.weight,
        "font-synthesis-style" => style.font_synthesis.style = source.font_synthesis.style,
        "font-synthesis-small-caps" => {
            style.font_synthesis.small_caps = source.font_synthesis.small_caps
        }
        "font-synthesis-position" => style.font_synthesis.position = source.font_synthesis.position,
        "line-height" => {
            style.line_height_value = source.line_height_value.clone();
            style.line_height = source.line_height;
            style.line_height_multiplier = source.line_height_multiplier;
            style.line_height_is_normal = source.line_height_is_normal;
        }
        "letter-spacing" => style.letter_spacing = source.letter_spacing.clone(),
        "word-spacing" => style.word_spacing = source.word_spacing.clone(),
        "width" => style.box_values.width = source.box_values.clone().width,
        "height" => {
            style.box_values.height = source.box_values.clone().height;
            style.physical_height_has_font_metric = source.physical_height_has_font_metric;
        }
        "min-width" => style.box_values.min_width = source.box_values.clone().min_width,
        "max-width" => style.box_values.max_width = source.box_values.clone().max_width,
        "min-height" => style.box_values.min_height = source.box_values.clone().min_height,
        "max-height" => style.box_values.max_height = source.box_values.clone().max_height,
        "box-sizing" => style.box_sizing = source.box_sizing,
        "left" => style.box_values.inset_left = source.box_values.clone().inset_left,
        "top" => style.box_values.inset_top = source.box_values.clone().inset_top,
        "right" => style.box_values.inset_right = source.box_values.clone().inset_right,
        "bottom" => style.box_values.inset_bottom = source.box_values.clone().inset_bottom,
        "position" => style.position = source.position,
        "float" => style.float = source.float,
        "footnote-display" => style.footnote_display = source.footnote_display,
        "footnote-policy" => style.footnote_policy = source.footnote_policy,
        "clear" => style.clear = source.clear,
        "z-index" => style.z_index = source.z_index,
        "opacity" => style.opacity = source.opacity,
        "transform" => style.transform = source.transform.clone(),
        "translate" => {
            style.individual_transforms.translate = source.individual_transforms.clone().translate
        }
        "rotate" => {
            style.individual_transforms.rotate = source.individual_transforms.clone().rotate
        }
        "scale" => style.individual_transforms.scale = source.individual_transforms.clone().scale,
        "transform-origin" => style.transform_origin = source.transform_origin.clone(),
        "transform-box" => style.transform_box = source.transform_box,
        "backface-visibility" => style.backface_visibility = source.backface_visibility,
        "isolation" => style.isolation = source.isolation,
        "mix-blend-mode" => style.mix_blend_mode = source.mix_blend_mode,
        "filter" => style.filter = source.filter.clone(),
        "clip-path" => style.clip_path = source.clip_path.clone(),
        "mask" | "mask-image" => style.mask = source.mask.clone(),
        "contain" => style.contain = source.contain,
        "container-type" => style.container_type = source.container_type,
        "container-name" => style.container_names = source.container_names.clone(),
        "container" => {
            style.container_type = source.container_type;
            style.container_names = source.container_names.clone();
        }
        "content-visibility" => style.content_visibility = source.content_visibility,
        "will-change" => style.will_change = source.will_change,
        "text-align" | "text-align-all" => style.text_align = source.text_align,
        "text-align-last" => style.text_align_last = source.text_align_last,
        "text-justify" => style.text_justify = source.text_justify,
        "text-autospace" => style.text_autospace = source.text_autospace,
        "text-spacing-trim" => style.text_spacing_trim = source.text_spacing_trim,
        "word-space-transform" => style.word_space_transform = source.word_space_transform,
        "initial-letter" => style.initial_letter = source.initial_letter,
        "initial-letter-align" => style.initial_letter_align = source.initial_letter_align,
        "initial-letter-wrap" => style.initial_letter_wrap = source.initial_letter_wrap.clone(),
        "line-fit-edge" => style.line_fit_edge = source.line_fit_edge,
        "text-box-trim" => style.text_box_trim = source.text_box_trim,
        "text-box-edge" => style.text_box_edge = source.text_box_edge,
        "box-decoration-break" => style.box_decoration_break = source.box_decoration_break,
        "text-indent" => style.text_indent = source.text_indent.clone(),
        "hanging-punctuation" => style.hanging_punctuation = source.hanging_punctuation,
        "vertical-align" => style.vertical_align = source.vertical_align.clone(),
        "dominant-baseline" => {
            style.vertical_align.dominant_baseline =
                source.vertical_align.clone().dominant_baseline;
        }
        "alignment-baseline" => {
            style.vertical_align.alignment_baseline =
                source.vertical_align.clone().alignment_baseline;
        }
        "baseline-source" => {
            style.vertical_align.baseline_source = source.vertical_align.clone().baseline_source;
        }
        "baseline-shift" => {
            style.vertical_align.baseline_shift = source.vertical_align.clone().baseline_shift;
        }
        "font-weight" => style.font_weight = source.font_weight,
        "font-style" => style.font_style = source.font_style,
        "font-width" | "font-stretch" => style.font_width = source.font_width,
        "font-family" => style.font_family = source.font_family.clone(),
        "font-feature-settings" => {
            style.font_feature_settings = source.font_feature_settings.clone();
        }
        "font-variation-settings" => {
            style.font_variation_settings = source.font_variation_settings.clone();
        }
        "font-palette" => style.font_palette = source.font_palette.clone(),
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
        "tab-size" => style.tab_size = source.tab_size.clone(),
        "visibility" => style.visibility = source.visibility,
        "list-style-type" => style.list_style_type = source.list_style_type.clone(),
        "list-style-position" => style.list_style_position = source.list_style_position,
        "list-style-image" => {
            style.list_style_image = source.list_style_image.clone();
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
            style.break_inside_avoid_column = source.break_inside_avoid_column;
        }
        "orphans" => style.orphans = source.orphans,
        "widows" => style.widows = source.widows,
        "text-decoration-line" | "text-decoration" => {
            style.text_decoration.underline = source.text_decoration.clone().underline;
            style.text_decoration.overline = source.text_decoration.clone().overline;
            style.text_decoration.line_through = source.text_decoration.clone().line_through;
            style.text_decoration.blink = source.text_decoration.clone().blink;
            style.text_decoration.spelling_error = source.text_decoration.clone().spelling_error;
            style.text_decoration.grammar_error = source.text_decoration.clone().grammar_error;
        }
        "text-decoration-style" => {
            style.text_decoration.style = source.text_decoration.clone().style
        }
        "text-decoration-color" => {
            style.text_decoration.color = source.text_decoration.clone().color
        }
        "text-decoration-thickness" => {
            style.text_decoration.thickness = source.text_decoration.clone().thickness;
        }
        "text-decoration-inset" => {
            style.text_decoration.inset = source.text_decoration.clone().inset;
        }
        "text-decoration-skip-ink" => {
            style.text_decoration.skip_ink = source.text_decoration.clone().skip_ink;
        }
        "text-decoration-skip-self" => {
            style.text_decoration.skip_self = source.text_decoration.clone().skip_self;
        }
        "text-decoration-skip-box" => {
            style.text_decoration.skip_box = source.text_decoration.clone().skip_box;
        }
        "text-decoration-skip-spaces" => {
            style.text_decoration.skip_spaces = source.text_decoration.clone().skip_spaces;
        }
        "text-underline-offset" => {
            style.text_decoration.underline_offset =
                source.text_decoration.clone().underline_offset;
        }
        "text-underline-position" => {
            style.text_decoration.underline_position =
                source.text_decoration.clone().underline_position;
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
        "white-space" => {
            style.white_space = source.white_space;
            style.text_wrap_mode = source.text_wrap_mode;
        }
        "text-wrap" => {
            style.text_wrap_mode = source.text_wrap_mode;
            style.text_wrap_style = source.text_wrap_style;
        }
        "text-wrap-mode" => style.text_wrap_mode = source.text_wrap_mode,
        "text-wrap-style" => style.text_wrap_style = source.text_wrap_style,
        "wrap-inside" => style.wrap_inside = source.wrap_inside,
        "line-clamp" | "-webkit-line-clamp" => style.line_clamp = source.line_clamp.clone(),
        "word-break" => style.word_break = source.word_break,
        "overflow" => {
            style.overflow = source.overflow;
            style.overflow_x = source.overflow_x;
            style.overflow_y = source.overflow_y;
        }
        "overflow-x" => style.overflow_x = source.overflow_x,
        "overflow-y" => style.overflow_y = source.overflow_y,
        "scroll-snap-type" => style.scroll_snap_type = source.scroll_snap_type,
        "scroll-snap-align" => style.scroll_snap_align = source.scroll_snap_align,
        "scroll-snap-stop" => style.scroll_snap_stop = source.scroll_snap_stop,
        "scroll-padding" => style.scroll_padding = source.scroll_padding.clone(),
        "scroll-padding-top" => style.scroll_padding.top = source.scroll_padding.top.clone(),
        "scroll-padding-right" => style.scroll_padding.right = source.scroll_padding.right.clone(),
        "scroll-padding-bottom" => {
            style.scroll_padding.bottom = source.scroll_padding.bottom.clone()
        }
        "scroll-padding-left" => style.scroll_padding.left = source.scroll_padding.left.clone(),
        "scroll-margin" => style.scroll_margin = source.scroll_margin.clone(),
        "scroll-margin-top" => style.scroll_margin.top = source.scroll_margin.top.clone(),
        "scroll-margin-right" => style.scroll_margin.right = source.scroll_margin.right.clone(),
        "scroll-margin-bottom" => style.scroll_margin.bottom = source.scroll_margin.bottom.clone(),
        "scroll-margin-left" => style.scroll_margin.left = source.scroll_margin.left.clone(),
        "overflow-clip-margin" => style.overflow_clip_margin = source.overflow_clip_margin,
        "overflow-wrap" | "word-wrap" => style.overflow_wrap = source.overflow_wrap,
        "line-break" => style.line_break = source.line_break,
        "hyphens" => style.hyphens = source.hyphens,
        "hyphenate-character" => style.hyphenate_character = source.hyphenate_character.clone(),
        "hyphenate-limit-chars" => {
            style.hyphenate_limit_chars = source.hyphenate_limit_chars;
        }
        "content" => {
            style.content = source.content.clone();
            style.marker_content = source.marker_content.clone();
        }
        "quotes" => style.quotes = source.quotes.clone().inherited(),
        _ => {}
    }
}
