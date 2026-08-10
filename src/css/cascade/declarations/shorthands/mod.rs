use super::*;

/// Expands simple modeled shorthands into their longhands before rollback.
///
/// CSS Cascade Level 5 says shorthands set all of their longhands. Expanding
/// them before `revert-layer` lets a longhand rollback remove only that
/// longhand from an earlier shorthand while preserving unaffected sides:
/// <https://www.w3.org/TR/css-cascade-5/#shorthand>.
pub(in crate::css) fn expand_modeled_shorthands<'a>(
    declarations: &'a [CascadedDeclaration<'a>],
    direction: Direction,
    writing_mode: WritingMode,
) -> Vec<CascadedDeclaration<'a>> {
    let mut expanded = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let is_all_shorthand = matches!(
            declaration.property,
            CascadedProperty::Modeled(ModeledProperty::All)
        );
        if contains_css_variable_reference(&declaration.value)
            || (!is_all_shorthand
                && (declaration_is_revert(&declaration.value)
                    || declaration_is_revert_layer(&declaration.value)))
        {
            expanded.push(declaration.clone());
            continue;
        }
        if is_all_shorthand {
            // `all` is an ordinary shorthand at the cascade boundary. Expand
            // it here, rather than applying it as a late bulk reset, so that
            // property-specific prepasses and rollback keywords see the same
            // longhand cascade as every other shorthand:
            // <https://drafts.csswg.org/css-cascade-5/#all-shorthand>.
            for target in all_shorthand_longhands() {
                let mut longhand = declaration.clone();
                longhand.property = CascadedProperty::Modeled(ModeledProperty::Longhand(target));
                expanded.push(longhand);
            }
        } else if matches!(
            declaration.property,
            CascadedProperty::Modeled(ModeledProperty::Shorthand(ModeledShorthand::Font))
        ) {
            // `font` needs the inherited font metrics to parse its value, so
            // retain its token stream while exposing one canonical component
            // declaration per affected longhand. This gives rollback and
            // variable-invalidity the same partial-longhand behavior as every
            // other shorthand without prematurely resolving relative units.
            for target in declaration
                .property
                .modeled()
                .expect("font shorthand is modeled")
                .resolve_targets(direction, writing_mode)
            {
                let mut component = declaration.clone();
                component.property =
                    CascadedProperty::Modeled(ModeledProperty::FontComponent(target));
                expanded.push(component);
            }
        } else if let Some(parts) =
            expand_box_edge_shorthand(declaration.property.css_name(), &declaration.value)
        {
            for (name, value) in parts {
                let mut longhand = declaration.clone();
                longhand.property = CascadedProperty::from_name(Cow::Borrowed(name));
                longhand.value = Cow::Owned(value);
                expanded.push(longhand);
            }
        } else if let Some(parts) = expand_simple_modeled_shorthand(
            declaration.property.css_name(),
            &declaration.value,
            direction,
            writing_mode,
        ) {
            for (name, value) in parts {
                let mut longhand = declaration.clone();
                longhand.property = CascadedProperty::from_name(Cow::Borrowed(name));
                longhand.value = Cow::Owned(value);
                expanded.push(longhand);
            }
        } else {
            expanded.push(declaration.clone());
        }
    }
    expanded
}

pub(in crate::css) fn expand_box_edge_shorthand(
    name: &str,
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let names = match name {
        "margin" => ["margin-top", "margin-right", "margin-bottom", "margin-left"],
        "padding" => [
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ],
        "scroll-padding" => [
            "scroll-padding-top",
            "scroll-padding-right",
            "scroll-padding-bottom",
            "scroll-padding-left",
        ],
        "scroll-margin" => [
            "scroll-margin-top",
            "scroll-margin-right",
            "scroll-margin-bottom",
            "scroll-margin-left",
        ],
        "inset" => ["top", "right", "bottom", "left"],
        _ => return None,
    };
    let value = trim_css_value(value);
    let parts = split_css_component_values(value);
    let [top, right, bottom, left] = match parts.as_slice() {
        [all] => [*all, *all, *all, *all],
        [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
        [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
        [top, right, bottom, left] => [*top, *right, *bottom, *left],
        _ => return None,
    };
    Some(
        names
            .into_iter()
            .zip([top, right, bottom, left])
            .map(|(name, value)| (name, value.to_string()))
            .collect(),
    )
}

pub(in crate::css) fn expand_simple_modeled_shorthand(
    name: &str,
    value: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    match name {
        "animation" => expand_animation_snapshot_shorthand(value),
        "gap" => expand_gap_shorthand(value),
        "line-clamp" => expand_line_clamp_shorthand(value, false),
        "-webkit-line-clamp" => expand_line_clamp_shorthand(value, true),
        "grid-gap" => expand_gap_shorthand(value),
        "grid-row-gap" => parse_gap(value, ROOT_FONT_SIZE_PT)
            .map(|_| vec![("row-gap", trim_css_value(value).to_string())]),
        "grid-column-gap" => parse_gap(value, ROOT_FONT_SIZE_PT)
            .map(|_| vec![("column-gap", trim_css_value(value).to_string())]),
        "column-rule" => expand_gap_rule_shorthand(value, "column-rule"),
        "row-rule" => expand_gap_rule_shorthand(value, "row-rule"),
        "rule" => expand_rule_shorthand(value),
        "rule-width" => expand_rule_axis_shorthand(value, "width"),
        "rule-style" => expand_rule_axis_shorthand(value, "style"),
        "rule-color" => expand_rule_axis_shorthand(value, "color"),
        "rule-break" => expand_rule_axis_shorthand(value, "break"),
        "rule-visibility-items" => expand_rule_axis_shorthand(value, "visibility-items"),
        "rule-inset" => expand_rule_axis_shorthand(value, "inset"),
        "rule-inset-start" => expand_rule_axis_shorthand(value, "inset-start"),
        "rule-inset-end" => expand_rule_axis_shorthand(value, "inset-end"),
        "rule-inset-cap" => expand_rule_axis_shorthand(value, "inset-cap"),
        "rule-inset-junction" => expand_rule_axis_shorthand(value, "inset-junction"),
        "column-rule-inset" | "row-rule-inset" => expand_gap_rule_inset_shorthand(name, value),
        "column-rule-inset-start"
        | "column-rule-inset-end"
        | "row-rule-inset-start"
        | "row-rule-inset-end" => expand_gap_rule_inset_side_shorthand(name, value),
        "column-rule-inset-cap"
        | "column-rule-inset-junction"
        | "row-rule-inset-cap"
        | "row-rule-inset-junction" => expand_gap_rule_inset_kind_shorthand(name, value),
        "flex-flow" => expand_flex_flow_shorthand(value),
        "flex" => expand_flex_shorthand(value),
        "grid-row" => expand_grid_placement_shorthand(value, "grid-row-start", "grid-row-end"),
        "grid-column" => {
            expand_grid_placement_shorthand(value, "grid-column-start", "grid-column-end")
        }
        "grid" => expand_grid_shorthand(value),
        "grid-template" => expand_grid_template_shorthand(value),
        "grid-area" => expand_grid_area_shorthand(value),
        "place-content" | "place-items" | "place-self" => {
            expand_alignment_place_shorthand(name, value)
        }
        "columns" => expand_columns_shorthand(value),
        "list-style" => expand_list_style_shorthand(value),
        "inline-size" | "block-size" | "min-inline-size" | "max-inline-size" | "min-block-size"
        | "max-block-size" => expand_logical_size_value(name, value, writing_mode),
        "contain-intrinsic-inline-size" | "contain-intrinsic-block-size" => {
            expand_logical_contain_intrinsic_size_value(name, value, writing_mode)
        }
        "margin-block"
        | "margin-inline"
        | "margin-block-start"
        | "margin-block-end"
        | "margin-inline-start"
        | "margin-inline-end" => {
            expand_logical_box_edge_values(name, value, "margin", direction, writing_mode)
        }
        "padding-block"
        | "padding-inline"
        | "padding-block-start"
        | "padding-block-end"
        | "padding-inline-start"
        | "padding-inline-end" => {
            expand_logical_box_edge_values(name, value, "padding", direction, writing_mode)
        }
        "scroll-padding-block"
        | "scroll-padding-inline"
        | "scroll-padding-block-start"
        | "scroll-padding-block-end"
        | "scroll-padding-inline-start"
        | "scroll-padding-inline-end" => {
            expand_logical_box_edge_values(name, value, "scroll-padding", direction, writing_mode)
        }
        "scroll-margin-block"
        | "scroll-margin-inline"
        | "scroll-margin-block-start"
        | "scroll-margin-block-end"
        | "scroll-margin-inline-start"
        | "scroll-margin-inline-end" => {
            expand_logical_box_edge_values(name, value, "scroll-margin", direction, writing_mode)
        }
        "inset-block" | "inset-inline" | "inset-block-start" | "inset-block-end"
        | "inset-inline-start" | "inset-inline-end" => {
            expand_logical_box_edge_values(name, value, "inset", direction, writing_mode)
        }
        "scroll-padding" | "scroll-margin" => expand_scroll_edge_shorthand(name, value),
        "border" => expand_border_shorthand(value),
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            expand_border_side_shorthand(name, value)
        }
        "outline" => expand_outline_shorthand(value),
        "border-radius" => expand_border_radius_shorthand(value),
        "border-top-radius"
        | "border-right-radius"
        | "border-bottom-radius"
        | "border-left-radius" => expand_border_side_radius_shorthand(name, value),
        "border-block-start-radius"
        | "border-block-end-radius"
        | "border-inline-start-radius"
        | "border-inline-end-radius" => {
            expand_logical_border_side_radius_shorthand(name, value, direction, writing_mode)
        }
        "corner" => expand_corner_shorthand(value),
        "corner-shape" => expand_corner_shape_shorthand(value),
        "border-block" | "border-inline" => {
            expand_logical_border_shorthand(name, value, direction, writing_mode)
        }
        "border-block-start" | "border-block-end" | "border-inline-start" | "border-inline-end" => {
            expand_logical_border_side_shorthand(name, value, direction, writing_mode)
        }
        "border-block-width" | "border-inline-width" => {
            expand_logical_border_axis_values(name, value, "width", direction, writing_mode)
        }
        "border-block-style" | "border-inline-style" => {
            expand_logical_border_axis_values(name, value, "style", direction, writing_mode)
        }
        "border-block-color" | "border-inline-color" => {
            expand_logical_border_axis_values(name, value, "color", direction, writing_mode)
        }
        _ => None,
    }
}

fn expand_animation_snapshot_shorthand(value: &str) -> Option<Vec<(&'static str, String)>> {
    let animation = parse_animation_snapshot_shorthand(value)?;
    Some(vec![
        (
            "animation-name",
            animation
                .name
                .as_ref()
                .map_or_else(|| "none".to_string(), KeyframesName::to_css_string),
        ),
        (
            "animation-duration",
            format!("{}s", animation.duration_seconds),
        ),
        ("animation-delay", format!("{}s", animation.delay_seconds)),
    ])
}

/// Expand the CSS Overflow Level 4 line-clamp shorthands into independently
/// cascaded longhands. Doing this before cascade defaulting is essential: a
/// later `block-ellipsis` declaration must be able to override only the
/// marker synthesized by an earlier shorthand.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp>
fn expand_line_clamp_shorthand(value: &str, webkit: bool) -> Option<Vec<(&'static str, String)>> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(vec![
            ("max-lines", "none".to_string()),
            // The legacy shorthand always resets the inherited marker to
            // `auto`, including for its `none` value.  The standardized
            // shorthand's `none` value instead resets it to `no-ellipsis`.
            // <https://drafts.csswg.org/css-overflow-4/#webkit-line-clamp>
            (
                "block-ellipsis",
                if webkit { "auto" } else { "no-ellipsis" }.to_string(),
            ),
            ("continue", "auto".to_string()),
        ]);
    }

    let components = split_css_component_values(value);
    if components.is_empty() || (webkit && components.len() != 1) {
        return None;
    }
    let mut max_lines = None;
    let mut ellipsis = None;
    let mut legacy = false;
    for component in components {
        if component.eq_ignore_ascii_case("-webkit-legacy") && !webkit && !legacy {
            legacy = true;
        } else if component.eq_ignore_ascii_case("auto") && ellipsis.is_none() {
            ellipsis = Some("auto".to_string());
        } else if component.eq_ignore_ascii_case("no-ellipsis") && ellipsis.is_none() {
            ellipsis = Some("no-ellipsis".to_string());
        } else if component.starts_with('"') || component.starts_with('\'') {
            if ellipsis.is_some() {
                return None;
            }
            // The longhand parser validates and unescapes the CSS string.
            ellipsis = Some(component.to_string());
        } else if max_lines.is_none()
            && component
                .parse::<usize>()
                .ok()
                .and_then(std::num::NonZeroUsize::new)
                .is_some()
        {
            max_lines = Some(component.to_string());
        } else {
            return None;
        }
    }
    Some(vec![
        ("max-lines", max_lines.unwrap_or_else(|| "none".to_string())),
        (
            "block-ellipsis",
            ellipsis.unwrap_or_else(|| "auto".to_string()),
        ),
        (
            "continue",
            if webkit || legacy {
                "-webkit-legacy".to_string()
            } else {
                "collapse".to_string()
            },
        ),
    ])
}

pub(in crate::css) fn expand_logical_size_value(
    name: &str,
    value: &str,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    Some(vec![(
        logical_size_physical_longhand(name, writing_mode)?,
        trim_css_value(value).to_string(),
    )])
}

/// Expand a logical intrinsic-size override to the physical component stored
/// by the computed style.
///
/// CSS Containment's logical longhands follow the element's writing mode in
/// the same way as CSS Logical Properties sizing longhands:
/// <https://drafts.csswg.org/css-contain-3/#contain-intrinsic-size> and
/// <https://www.w3.org/TR/css-logical-1/#dimension-properties>.
fn expand_logical_contain_intrinsic_size_value(
    name: &str,
    value: &str,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    let physical_name = logical_contain_intrinsic_size_physical_longhand(name, writing_mode)?;
    Some(vec![(physical_name, trim_css_value(value).to_string())])
}

/// Return the physical containment sizing longhand addressed by a logical
/// containment intrinsic-size property.
pub(in crate::css) fn logical_contain_intrinsic_size_physical_longhand(
    name: &str,
    writing_mode: WritingMode,
) -> Option<&'static str> {
    let axes = WritingModeAxes::new(writing_mode, Direction::Ltr);
    let axis = match name {
        "contain-intrinsic-inline-size" => axes.physical_axis(LogicalAxis::Inline),
        "contain-intrinsic-block-size" => axes.physical_axis(LogicalAxis::Block),
        _ => return None,
    };
    Some(match axis {
        PhysicalAxis::Horizontal => "contain-intrinsic-width",
        PhysicalAxis::Vertical => "contain-intrinsic-height",
    })
}

/// Return the physical sizing longhand addressed by a logical size property.
///
/// CSS Logical Properties maps inline/block size longhands through the
/// element's writing mode:
/// <https://www.w3.org/TR/css-logical-1/#dimension-properties>.
pub(in crate::css) fn logical_size_physical_longhand(
    name: &str,
    writing_mode: WritingMode,
) -> Option<&'static str> {
    let axes = WritingModeAxes::new(writing_mode, Direction::Ltr);
    let inline_axis = axes.physical_axis(LogicalAxis::Inline);
    let block_axis = axes.physical_axis(LogicalAxis::Block);
    match name {
        "inline-size" => Some(size_longhand_for_axis(inline_axis)),
        "block-size" => Some(size_longhand_for_axis(block_axis)),
        "min-inline-size" => Some(min_size_longhand_for_axis(inline_axis)),
        "max-inline-size" => Some(max_size_longhand_for_axis(inline_axis)),
        "min-block-size" => Some(min_size_longhand_for_axis(block_axis)),
        "max-block-size" => Some(max_size_longhand_for_axis(block_axis)),
        _ => None,
    }
}

pub(in crate::css) fn size_longhand_for_axis(axis: PhysicalAxis) -> &'static str {
    match axis {
        PhysicalAxis::Horizontal => "width",
        PhysicalAxis::Vertical => "height",
    }
}

pub(in crate::css) fn min_size_longhand_for_axis(axis: PhysicalAxis) -> &'static str {
    match axis {
        PhysicalAxis::Horizontal => "min-width",
        PhysicalAxis::Vertical => "min-height",
    }
}

pub(in crate::css) fn max_size_longhand_for_axis(axis: PhysicalAxis) -> &'static str {
    match axis {
        PhysicalAxis::Horizontal => "max-width",
        PhysicalAxis::Vertical => "max-height",
    }
}

pub(in crate::css) struct ListStyleShorthandComponents {
    pub(in crate::css) style_type: String,
    pub(in crate::css) position: String,
    pub(in crate::css) image: String,
}

/// Expands the CSS Lists `list-style` shorthand into its three longhands.
///
/// CSS Lists Level 3 defines `list-style` as an unordered shorthand for
/// `list-style-type`, `list-style-position`, and `list-style-image`, with
/// ambiguous `none` tokens assigned to whichever of type/image are not
/// otherwise specified:
/// <https://www.w3.org/TR/css-lists-3/#propdef-list-style>.
pub(in crate::css) fn expand_list_style_shorthand(
    value: &str,
) -> Option<Vec<(&'static str, String)>> {
    let components = parse_list_style_shorthand(value)?;
    Some(vec![
        ("list-style-type", components.style_type),
        ("list-style-position", components.position),
        ("list-style-image", components.image),
    ])
}

pub(in crate::css) fn parse_list_style_shorthand(
    value: &str,
) -> Option<ListStyleShorthandComponents> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }

    let mut style_type = None;
    let mut position = None;
    let mut image = None;
    let mut none_count = 0usize;

    for part in parts {
        if part.eq_ignore_ascii_case("none") {
            none_count += 1;
            continue;
        }

        if image.is_none() && parse_list_style_image_component(part, None, None).is_some() {
            image = Some(part.to_string());
        } else if position.is_none() && parse_list_style_position(part).is_some() {
            position = Some(part.to_string());
        } else if style_type.is_none() && parse_list_style_type(part).is_some() {
            style_type = Some(part.to_string());
        } else {
            return None;
        }
    }

    if style_type.is_none() && none_count > 0 {
        style_type = Some("none".to_string());
        none_count -= 1;
    }
    if image.is_none() && none_count > 0 {
        image = Some("none".to_string());
        none_count -= 1;
    }
    if none_count > 0 {
        return None;
    }

    Some(ListStyleShorthandComponents {
        style_type: style_type.unwrap_or_else(|| "disc".to_string()),
        position: position.unwrap_or_else(|| "outside".to_string()),
        image: image.unwrap_or_else(|| "none".to_string()),
    })
}

/// Parse a `list-style-image` value while retaining its CSS image metadata.
///
/// CSS Lists accepts any CSS `<image>`, including `image-set()`. The latter's
/// resolution descriptor contributes to the used intrinsic size of a marker,
/// so reducing this to a URL would lose required layout information.
/// <https://drafts.csswg.org/css-lists-3/#list-style-image-property>
/// <https://drafts.csswg.org/css-images-4/#image-set-notation>
pub(in crate::css) fn parse_list_style_image_component(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<ComputedImage> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(ComputedImage::None);
    }
    match parse_css_image(value, base_url, root_url) {
        ParsedImage::Image(image) => Some(image),
        ParsedImage::NotAnImage | ParsedImage::SyntaxError => None,
    }
}

/// Expands logical margin and padding properties into physical longhands.
///
/// CSS Logical Properties defines flow-relative box edges as aliases for
/// physical margin and padding edges after resolving `writing-mode` and
/// `direction`:
/// <https://www.w3.org/TR/css-logical-1/#box>.
pub(in crate::css) fn expand_logical_box_edge_values(
    name: &str,
    value: &str,
    property: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<(&'static str, String)>> {
    let longhand = |logical_side| {
        let side = logical_box_side(logical_side, direction, writing_mode)?;
        match property {
            "margin" => Some(physical_margin_side_longhand(side)),
            "padding" => Some(physical_padding_side_longhand(side)),
            "scroll-padding" => Some(physical_scroll_padding_side_longhand(side)),
            "scroll-margin" => Some(physical_scroll_margin_side_longhand(side)),
            "inset" => Some(physical_inset_side_longhand(side)),
            _ => None,
        }
    };
    if matches!(
        name,
        "margin-block"
            | "margin-inline"
            | "padding-block"
            | "padding-inline"
            | "scroll-padding-block"
            | "scroll-padding-inline"
            | "scroll-margin-block"
            | "scroll-margin-inline"
            | "inset-block"
            | "inset-inline"
    ) {
        let [start, end] = logical_box_axis_side_names(name)?;
        let parts = split_css_component_values(trim_css_value(value));
        let [start_value, end_value] = match parts.as_slice() {
            [all] => [*all, *all],
            [start, end] => [*start, *end],
            _ => return None,
        };
        return Some(vec![
            (longhand(start)?, start_value.to_string()),
            (longhand(end)?, end_value.to_string()),
        ]);
    }
    Some(vec![(longhand(name)?, trim_css_value(value).to_string())])
}

/// Expand the physical scroll padding and margin shorthands exactly as the
/// corresponding four-sided box shorthands.
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-padding>
/// <https://www.w3.org/TR/css-scroll-snap-1/#propdef-scroll-margin>
fn expand_scroll_edge_shorthand(name: &str, value: &str) -> Option<Vec<(&'static str, String)>> {
    let values = split_css_component_values(trim_css_value(value));
    let [top, right, bottom, left] = match values.as_slice() {
        [all] => [*all, *all, *all, *all],
        [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
        [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
        [top, right, bottom, left] => [*top, *right, *bottom, *left],
        _ => return None,
    };
    let names = match name {
        "scroll-padding" => [
            "scroll-padding-top",
            "scroll-padding-right",
            "scroll-padding-bottom",
            "scroll-padding-left",
        ],
        "scroll-margin" => [
            "scroll-margin-top",
            "scroll-margin-right",
            "scroll-margin-bottom",
            "scroll-margin-left",
        ],
        _ => return None,
    };
    Some(vec![
        (names[0], top.to_string()),
        (names[1], right.to_string()),
        (names[2], bottom.to_string()),
        (names[3], left.to_string()),
    ])
}

pub(in crate::css) fn physical_scroll_padding_side_longhand(side: BoxSide) -> &'static str {
    match side {
        BoxSide::Top => "scroll-padding-top",
        BoxSide::Right => "scroll-padding-right",
        BoxSide::Bottom => "scroll-padding-bottom",
        BoxSide::Left => "scroll-padding-left",
    }
}

pub(in crate::css) fn physical_scroll_margin_side_longhand(side: BoxSide) -> &'static str {
    match side {
        BoxSide::Top => "scroll-margin-top",
        BoxSide::Right => "scroll-margin-right",
        BoxSide::Bottom => "scroll-margin-bottom",
        BoxSide::Left => "scroll-margin-left",
    }
}

pub(in crate::css) fn physical_inset_side_longhand(side: BoxSide) -> &'static str {
    match side {
        BoxSide::Top => "top",
        BoxSide::Right => "right",
        BoxSide::Bottom => "bottom",
        BoxSide::Left => "left",
    }
}

/// Expand the physical `border` shorthand into side longhands.
///
pub(in crate::css) fn split_css_top_level_slashes(value: &str) -> Vec<&str> {
    crate::css::component_values::split_css_top_level_delimiter(value, '/')
}

/// Find a top-level delimiter without splitting nested CSS component values.
///
/// CSS Syntax Level 3 models function bodies and bracketed values as nested
/// component values:
/// <https://www.w3.org/TR/css-syntax-3/#component-value>.
pub(in crate::css) fn split_top_level_once(value: &str, delimiter: char) -> Option<(&str, &str)> {
    crate::css::component_values::split_css_top_level_once(value, delimiter)
}

/// Expand one-to-four corner radius values using CSS box-edge ordering.
///
/// CSS Backgrounds and Borders Level 3 uses top, right, bottom, left expansion
/// for the horizontal and vertical radius groups:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-radius>.
pub(in crate::css) fn expand_four_radius_components(values: &[String]) -> Option<Vec<String>> {
    match values {
        [all] => Some(vec![all.clone(), all.clone(), all.clone(), all.clone()]),
        [vertical, horizontal] => Some(vec![
            vertical.clone(),
            horizontal.clone(),
            vertical.clone(),
            horizontal.clone(),
        ]),
        [top, horizontal, bottom] => Some(vec![
            top.clone(),
            horizontal.clone(),
            bottom.clone(),
            horizontal.clone(),
        ]),
        [top, right, bottom, left] => Some(vec![
            top.clone(),
            right.clone(),
            bottom.clone(),
            left.clone(),
        ]),
        _ => None,
    }
}

/// Serialize one physical corner radius longhand from horizontal/vertical radii.
///
/// A corner radius longhand accepts one value when both radii match, otherwise
/// two values for horizontal then vertical radius:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-radius>.
pub(in crate::css) fn radius_pair(horizontal: &str, vertical: &str) -> String {
    if horizontal == vertical {
        horizontal.to_string()
    } else {
        format!("{horizontal} {vertical}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(name: &'static str, value: &'static str) -> CascadedDeclaration<'static> {
        CascadedDeclaration {
            property: CascadedProperty::from_name(Cow::Borrowed(name)),
            value: Cow::Borrowed(value),
            origin: StylesheetOrigin::Author,
            base_url: None,
            root_url: None,
            important: false,
            layer_order: None,
            specificity: 0,
            scope_proximity: usize::MAX,
            stylesheet_index: 0,
            rule_order: 0,
            declaration_order: 0,
        }
    }

    #[test]
    fn generated_canonical_longhand_names_are_borrowed() {
        let declaration = declaration("margin", "1px 2px");
        let expanded = expand_modeled_shorthands(
            std::slice::from_ref(&declaration),
            Direction::Ltr,
            WritingMode::HorizontalTb,
        );
        assert_eq!(expanded.len(), 4);
        assert!(
            expanded
                .iter()
                .all(|declaration| declaration.property.css_name().starts_with("margin-"))
        );
    }
}
