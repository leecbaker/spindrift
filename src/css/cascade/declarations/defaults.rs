use super::*;

/// Returns the physical longhands affected by a property in a writing context.
///
/// CSS Cascade Level 5 applies defaulting and rollback to the longhands of a
/// shorthand, while CSS Logical Properties resolves flow-relative border
/// properties through `writing-mode` and `direction`:
/// <https://www.w3.org/TR/css-cascade-5/#shorthand> and
/// <https://www.w3.org/TR/css-logical-1/#border-properties>.
pub(in crate::css) fn affected_longhands(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<Vec<&'static str>> {
    if let Some(longhand) = logical_size_physical_longhand(name, writing_mode) {
        return Some(vec![longhand]);
    }
    if matches!(name, "margin-block" | "margin-inline") {
        let [start, end] = logical_box_axis_side_names(name)?;
        return Some(vec![
            physical_margin_side_longhand(logical_box_side(start, direction, writing_mode)?),
            physical_margin_side_longhand(logical_box_side(end, direction, writing_mode)?),
        ]);
    }
    if matches!(name, "padding-block" | "padding-inline") {
        let [start, end] = logical_box_axis_side_names(name)?;
        return Some(vec![
            physical_padding_side_longhand(logical_box_side(start, direction, writing_mode)?),
            physical_padding_side_longhand(logical_box_side(end, direction, writing_mode)?),
        ]);
    }
    if matches!(name, "scroll-padding-block" | "scroll-padding-inline") {
        let [start, end] = logical_box_axis_side_names(name)?;
        return Some(vec![
            physical_scroll_padding_side_longhand(logical_box_side(
                start,
                direction,
                writing_mode,
            )?),
            physical_scroll_padding_side_longhand(logical_box_side(end, direction, writing_mode)?),
        ]);
    }
    if matches!(name, "scroll-margin-block" | "scroll-margin-inline") {
        let [start, end] = logical_box_axis_side_names(name)?;
        return Some(vec![
            physical_scroll_margin_side_longhand(logical_box_side(start, direction, writing_mode)?),
            physical_scroll_margin_side_longhand(logical_box_side(end, direction, writing_mode)?),
        ]);
    }
    if matches!(name, "inset-block" | "inset-inline") {
        let [start, end] = logical_box_axis_side_names(name)?;
        return Some(vec![
            physical_inset_side_longhand(logical_box_side(start, direction, writing_mode)?),
            physical_inset_side_longhand(logical_box_side(end, direction, writing_mode)?),
        ]);
    }
    if matches!(
        name,
        "margin-block-start" | "margin-block-end" | "margin-inline-start" | "margin-inline-end"
    ) {
        return Some(vec![physical_margin_side_longhand(logical_box_side(
            name,
            direction,
            writing_mode,
        )?)]);
    }
    if matches!(
        name,
        "padding-block-start" | "padding-block-end" | "padding-inline-start" | "padding-inline-end"
    ) {
        return Some(vec![physical_padding_side_longhand(logical_box_side(
            name,
            direction,
            writing_mode,
        )?)]);
    }
    if matches!(
        name,
        "scroll-padding-block-start"
            | "scroll-padding-block-end"
            | "scroll-padding-inline-start"
            | "scroll-padding-inline-end"
    ) {
        return Some(vec![physical_scroll_padding_side_longhand(
            logical_box_side(name, direction, writing_mode)?,
        )]);
    }
    if matches!(
        name,
        "scroll-margin-block-start"
            | "scroll-margin-block-end"
            | "scroll-margin-inline-start"
            | "scroll-margin-inline-end"
    ) {
        return Some(vec![physical_scroll_margin_side_longhand(
            logical_box_side(name, direction, writing_mode)?,
        )]);
    }
    if matches!(
        name,
        "inset-block-start" | "inset-block-end" | "inset-inline-start" | "inset-inline-end"
    ) {
        return Some(vec![physical_inset_side_longhand(logical_box_side(
            name,
            direction,
            writing_mode,
        )?)]);
    }
    if name == "inset" {
        return Some(vec!["top", "right", "bottom", "left"]);
    }
    if matches!(
        name,
        "border-block-start-radius"
            | "border-block-end-radius"
            | "border-inline-start-radius"
            | "border-inline-end-radius"
    ) {
        let corners = match name {
            "border-block-start-radius" => ["border-start-start-radius", "border-start-end-radius"],
            "border-block-end-radius" => ["border-end-start-radius", "border-end-end-radius"],
            "border-inline-start-radius" => {
                ["border-start-start-radius", "border-end-start-radius"]
            }
            "border-inline-end-radius" => ["border-start-end-radius", "border-end-end-radius"],
            _ => unreachable!(),
        };
        return corners
            .into_iter()
            .map(|corner| logical_corner_radius_longhand(corner, direction, writing_mode))
            .collect();
    }
    if matches!(name, "border-block" | "border-inline") {
        let logical_sides = logical_axis_side_names(name)?;
        return Some(
            logical_sides
                .into_iter()
                .map(|logical_side| {
                    Some(border_side_component_longhands(logical_border_side(
                        logical_side,
                        direction,
                        writing_mode,
                    )?))
                })
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect(),
        );
    }
    if matches!(
        name,
        "border-block-start" | "border-block-end" | "border-inline-start" | "border-inline-end"
    ) {
        return Some(
            border_side_component_longhands(logical_border_side(name, direction, writing_mode)?)
                .to_vec(),
        );
    }
    if matches!(name, "border-block-width" | "border-inline-width") {
        return Some(
            logical_axis_sides(name, direction, writing_mode)?
                .into_iter()
                .map(|side| physical_border_side_component_longhand(side, "width"))
                .collect(),
        );
    }
    if matches!(name, "border-block-style" | "border-inline-style") {
        return Some(
            logical_axis_sides(name, direction, writing_mode)?
                .into_iter()
                .map(|side| physical_border_side_component_longhand(side, "style"))
                .collect(),
        );
    }
    if matches!(name, "border-block-color" | "border-inline-color") {
        return Some(
            logical_axis_sides(name, direction, writing_mode)?
                .into_iter()
                .map(|side| physical_border_side_component_longhand(side, "color"))
                .collect(),
        );
    }
    if matches!(
        name,
        "border-block-start-width"
            | "border-block-end-width"
            | "border-inline-start-width"
            | "border-inline-end-width"
    ) {
        return Some(vec![physical_border_side_component_longhand(
            logical_border_side(name, direction, writing_mode)?,
            "width",
        )]);
    }
    if matches!(
        name,
        "border-block-start-style"
            | "border-block-end-style"
            | "border-inline-start-style"
            | "border-inline-end-style"
    ) {
        return Some(vec![physical_border_side_component_longhand(
            logical_border_side(name, direction, writing_mode)?,
            "style",
        )]);
    }
    if matches!(
        name,
        "border-block-start-color"
            | "border-block-end-color"
            | "border-inline-start-color"
            | "border-inline-end-color"
    ) {
        return Some(vec![physical_border_side_component_longhand(
            logical_border_side(name, direction, writing_mode)?,
            "color",
        )]);
    }
    if matches!(
        name,
        "border-start-start-radius"
            | "border-start-end-radius"
            | "border-end-start-radius"
            | "border-end-end-radius"
    ) {
        return Some(vec![logical_corner_radius_longhand(
            name,
            direction,
            writing_mode,
        )?]);
    }

    let longhands: &[&str] = match name {
        "margin" => &["margin-top", "margin-right", "margin-bottom", "margin-left"],
        "margin-top" => &["margin-top"],
        "margin-right" => &["margin-right"],
        "margin-bottom" => &["margin-bottom"],
        "margin-left" => &["margin-left"],
        "padding" => &[
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ],
        "padding-top" => &["padding-top"],
        "padding-right" => &["padding-right"],
        "padding-bottom" => &["padding-bottom"],
        "padding-left" => &["padding-left"],
        "border" => &[
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
        ],
        "border-top" => &["border-top-width", "border-top-style", "border-top-color"],
        "border-right" => &[
            "border-right-width",
            "border-right-style",
            "border-right-color",
        ],
        "border-bottom" => &[
            "border-bottom-width",
            "border-bottom-style",
            "border-bottom-color",
        ],
        "border-left" => &[
            "border-left-width",
            "border-left-style",
            "border-left-color",
        ],
        "border-width" => &[
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
        ],
        "border-top-width" => &["border-top-width"],
        "border-right-width" => &["border-right-width"],
        "border-bottom-width" => &["border-bottom-width"],
        "border-left-width" => &["border-left-width"],
        "border-style" => &[
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
        ],
        "border-top-style" => &["border-top-style"],
        "border-right-style" => &["border-right-style"],
        "border-bottom-style" => &["border-bottom-style"],
        "border-left-style" => &["border-left-style"],
        "border-color" => &[
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
        ],
        "border-top-color" => &["border-top-color"],
        "border-right-color" => &["border-right-color"],
        "border-bottom-color" => &["border-bottom-color"],
        "border-left-color" => &["border-left-color"],
        "border-radius" => &[
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ],
        "border-top-radius" => &["border-top-left-radius", "border-top-right-radius"],
        "border-right-radius" => &["border-top-right-radius", "border-bottom-right-radius"],
        "border-bottom-radius" => &["border-bottom-right-radius", "border-bottom-left-radius"],
        "border-left-radius" => &["border-top-left-radius", "border-bottom-left-radius"],
        "corner" => &[
            "border-top-left-radius",
            "corner-top-left-shape",
            "border-top-right-radius",
            "corner-top-right-shape",
            "border-bottom-right-radius",
            "corner-bottom-right-shape",
            "border-bottom-left-radius",
            "corner-bottom-left-shape",
        ],
        "corner-shape" => &[
            "corner-top-left-shape",
            "corner-top-right-shape",
            "corner-bottom-right-shape",
            "corner-bottom-left-shape",
        ],
        "border-top-left-radius" => &["border-top-left-radius"],
        "border-top-right-radius" => &["border-top-right-radius"],
        "border-bottom-right-radius" => &["border-bottom-right-radius"],
        "border-bottom-left-radius" => &["border-bottom-left-radius"],
        "corner-top-left-shape" => &["corner-top-left-shape"],
        "corner-top-right-shape" => &["corner-top-right-shape"],
        "corner-bottom-right-shape" => &["corner-bottom-right-shape"],
        "corner-bottom-left-shape" => &["corner-bottom-left-shape"],
        "border-image" => &[
            "border-image-source",
            "border-image-slice",
            "border-image-width",
            "border-image-outset",
            "border-image-repeat",
        ],
        "border-image-source" => &["border-image-source"],
        "border-image-slice" => &["border-image-slice"],
        "border-image-width" => &["border-image-width"],
        "border-image-outset" => &["border-image-outset"],
        "border-image-repeat" => &["border-image-repeat"],
        "outline" => &["outline-width", "outline-style", "outline-color"],
        "outline-width" => &["outline-width"],
        "outline-style" => &["outline-style"],
        "outline-color" => &["outline-color"],
        "background" => &[
            "background-color",
            "background-image",
            "background-size",
            "background-position",
            "background-position-x",
            "background-position-y",
            "background-repeat",
            "background-origin",
            "background-clip",
        ],
        "background-color" => &["background-color"],
        "background-image" => &["background-image"],
        "background-size" => &["background-size"],
        "background-position" => &[
            "background-position",
            "background-position-x",
            "background-position-y",
        ],
        "background-position-x" => &["background-position-x"],
        "background-position-y" => &["background-position-y"],
        "background-repeat" => &["background-repeat"],
        "background-origin" => &["background-origin"],
        "background-clip" => &["background-clip"],
        "text-align" => &["text-align-all", "text-align-last"],
        "text-spacing" => &["text-spacing-trim", "text-autospace"],
        "text-spacing-trim" => &["text-spacing-trim"],
        "text-autospace" => &["text-autospace"],
        "text-align-all" => &["text-align-all"],
        "text-align-last" => &["text-align-last"],
        "line-fit-edge" => &["line-fit-edge"],
        "text-box" => &["text-box-trim", "text-box-edge"],
        "text-box-trim" => &["text-box-trim"],
        "text-box-edge" => &["text-box-edge"],
        "initial-letter" => &["initial-letter"],
        "initial-letter-align" => &["initial-letter-align"],
        "initial-letter-wrap" => &["initial-letter-wrap"],
        "box-decoration-break" => &["box-decoration-break"],
        "flex" => &["flex-grow", "flex-shrink", "flex-basis"],
        "flex-grow" => &["flex-grow"],
        "flex-shrink" => &["flex-shrink"],
        "flex-basis" => &["flex-basis"],
        "order" => &["order"],
        "flex-flow" => &["flex-direction", "flex-wrap"],
        "flex-direction" => &["flex-direction"],
        "flex-wrap" => &["flex-wrap"],
        "place-content" => &["align-content", "justify-content"],
        "place-items" => &["align-items", "justify-items"],
        "place-self" => &["align-self", "justify-self"],
        "justify-content" => &["justify-content"],
        "justify-items" => &["justify-items"],
        "justify-self" => &["justify-self"],
        "align-content" => &["align-content"],
        "align-items" => &["align-items"],
        "align-self" => &["align-self"],
        "gap" => &["row-gap", "column-gap"],
        "grid-gap" => &["row-gap", "column-gap"],
        "row-gap" => &["row-gap"],
        "grid-row-gap" => &["row-gap"],
        "column-gap" => &["column-gap"],
        "grid-column-gap" => &["column-gap"],
        "rule" => &[
            "column-rule-width",
            "column-rule-style",
            "column-rule-color",
            "row-rule-width",
            "row-rule-style",
            "row-rule-color",
        ],
        "column-rule" => &[
            "column-rule-width",
            "column-rule-style",
            "column-rule-color",
        ],
        "row-rule" => &["row-rule-width", "row-rule-style", "row-rule-color"],
        "rule-width" => &["column-rule-width", "row-rule-width"],
        "rule-style" => &["column-rule-style", "row-rule-style"],
        "rule-color" => &["column-rule-color", "row-rule-color"],
        "column-rule-width" => &["column-rule-width"],
        "row-rule-width" => &["row-rule-width"],
        "column-rule-style" => &["column-rule-style"],
        "row-rule-style" => &["row-rule-style"],
        "column-rule-color" => &["column-rule-color"],
        "row-rule-color" => &["row-rule-color"],
        "rule-break" => &["column-rule-break", "row-rule-break"],
        "column-rule-break" => &["column-rule-break"],
        "row-rule-break" => &["row-rule-break"],
        "rule-visibility-items" => &["column-rule-visibility-items", "row-rule-visibility-items"],
        "column-rule-visibility-items" => &["column-rule-visibility-items"],
        "row-rule-visibility-items" => &["row-rule-visibility-items"],
        "rule-overlap" => &["rule-overlap"],
        "rule-inset" => &[
            "column-rule-inset-cap-start",
            "column-rule-inset-cap-end",
            "column-rule-inset-junction-start",
            "column-rule-inset-junction-end",
            "row-rule-inset-cap-start",
            "row-rule-inset-cap-end",
            "row-rule-inset-junction-start",
            "row-rule-inset-junction-end",
        ],
        "column-rule-inset" => &[
            "column-rule-inset-cap-start",
            "column-rule-inset-cap-end",
            "column-rule-inset-junction-start",
            "column-rule-inset-junction-end",
        ],
        "row-rule-inset" => &[
            "row-rule-inset-cap-start",
            "row-rule-inset-cap-end",
            "row-rule-inset-junction-start",
            "row-rule-inset-junction-end",
        ],
        "rule-inset-start" => &[
            "column-rule-inset-cap-start",
            "column-rule-inset-junction-start",
            "row-rule-inset-cap-start",
            "row-rule-inset-junction-start",
        ],
        "rule-inset-end" => &[
            "column-rule-inset-cap-end",
            "column-rule-inset-junction-end",
            "row-rule-inset-cap-end",
            "row-rule-inset-junction-end",
        ],
        "rule-inset-cap" => &[
            "column-rule-inset-cap-start",
            "column-rule-inset-cap-end",
            "row-rule-inset-cap-start",
            "row-rule-inset-cap-end",
        ],
        "rule-inset-junction" => &[
            "column-rule-inset-junction-start",
            "column-rule-inset-junction-end",
            "row-rule-inset-junction-start",
            "row-rule-inset-junction-end",
        ],
        "column-rule-inset-start" => &[
            "column-rule-inset-cap-start",
            "column-rule-inset-junction-start",
        ],
        "column-rule-inset-end" => &[
            "column-rule-inset-cap-end",
            "column-rule-inset-junction-end",
        ],
        "row-rule-inset-start" => &["row-rule-inset-cap-start", "row-rule-inset-junction-start"],
        "row-rule-inset-end" => &["row-rule-inset-cap-end", "row-rule-inset-junction-end"],
        "column-rule-inset-cap" => &["column-rule-inset-cap-start", "column-rule-inset-cap-end"],
        "column-rule-inset-junction" => &[
            "column-rule-inset-junction-start",
            "column-rule-inset-junction-end",
        ],
        "row-rule-inset-cap" => &["row-rule-inset-cap-start", "row-rule-inset-cap-end"],
        "row-rule-inset-junction" => &[
            "row-rule-inset-junction-start",
            "row-rule-inset-junction-end",
        ],
        "column-rule-inset-cap-start" => &["column-rule-inset-cap-start"],
        "column-rule-inset-cap-end" => &["column-rule-inset-cap-end"],
        "column-rule-inset-junction-start" => &["column-rule-inset-junction-start"],
        "column-rule-inset-junction-end" => &["column-rule-inset-junction-end"],
        "row-rule-inset-cap-start" => &["row-rule-inset-cap-start"],
        "row-rule-inset-cap-end" => &["row-rule-inset-cap-end"],
        "row-rule-inset-junction-start" => &["row-rule-inset-junction-start"],
        "row-rule-inset-junction-end" => &["row-rule-inset-junction-end"],
        "columns" => &[
            "column-count",
            "column-width",
            "column-height",
            "column-wrap",
        ],
        "column-count" => &["column-count"],
        "column-width" => &["column-width"],
        "column-height" => &["column-height"],
        "column-wrap" => &["column-wrap"],
        "list-style" => &["list-style-type", "list-style-position", "list-style-image"],
        "list-style-type" => &["list-style-type"],
        "list-style-position" => &["list-style-position"],
        "list-style-image" => &["list-style-image"],
        "text-decoration" => &[
            "text-decoration-line",
            "text-decoration-style",
            "text-decoration-color",
            "text-decoration-thickness",
        ],
        "text-decoration-line" => &["text-decoration-line"],
        "text-decoration-style" => &["text-decoration-style"],
        "text-decoration-color" => &["text-decoration-color"],
        "text-decoration-thickness" => &["text-decoration-thickness"],
        "text-decoration-inset" => &["text-decoration-inset"],
        "text-decoration-skip" => &[
            "text-decoration-skip-ink",
            "text-decoration-skip-self",
            "text-decoration-skip-box",
            "text-decoration-skip-spaces",
        ],
        "text-decoration-skip-ink" => &["text-decoration-skip-ink"],
        "text-decoration-skip-self" => &["text-decoration-skip-self"],
        "text-decoration-skip-box" => &["text-decoration-skip-box"],
        "text-decoration-skip-spaces" => &["text-decoration-skip-spaces"],
        "text-underline-offset" => &["text-underline-offset"],
        "text-underline-position" => &["text-underline-position"],
        "text-emphasis" => &["text-emphasis-style", "text-emphasis-color"],
        "text-emphasis-style" => &["text-emphasis-style"],
        "text-emphasis-color" => &["text-emphasis-color"],
        "text-emphasis-position" => &["text-emphasis-position"],
        "text-emphasis-skip" => &["text-emphasis-skip"],
        "ruby-position" => &["ruby-position"],
        "text-shadow" => &["text-shadow"],
        "box-shadow" => &["box-shadow"],
        "overflow" => &["overflow-x", "overflow-y"],
        "overflow-x" => &["overflow-x"],
        "overflow-y" => &["overflow-y"],
        "scrollbar-gutter" => &["scrollbar-gutter"],
        "scrollbar-width" => &["scrollbar-width"],
        "scroll-snap-type" => &["scroll-snap-type"],
        "scroll-snap-align" => &["scroll-snap-align"],
        "scroll-snap-stop" => &["scroll-snap-stop"],
        "scroll-padding" => &[
            "scroll-padding-top",
            "scroll-padding-right",
            "scroll-padding-bottom",
            "scroll-padding-left",
        ],
        "scroll-padding-top" => &["scroll-padding-top"],
        "scroll-padding-right" => &["scroll-padding-right"],
        "scroll-padding-bottom" => &["scroll-padding-bottom"],
        "scroll-padding-left" => &["scroll-padding-left"],
        "scroll-margin" => &[
            "scroll-margin-top",
            "scroll-margin-right",
            "scroll-margin-bottom",
            "scroll-margin-left",
        ],
        "scroll-margin-top" => &["scroll-margin-top"],
        "scroll-margin-right" => &["scroll-margin-right"],
        "scroll-margin-bottom" => &["scroll-margin-bottom"],
        "scroll-margin-left" => &["scroll-margin-left"],
        "word-wrap" | "overflow-wrap" => &["overflow-wrap"],
        "font-variant" => &[
            "font-variant-ligatures",
            "font-variant-position",
            "font-variant-caps",
            "font-variant-numeric",
            "font-variant-alternates",
            "font-variant-east-asian",
            "font-variant-emoji",
        ],
        "font-variant-ligatures" => &["font-variant-ligatures"],
        "font-variant-position" => &["font-variant-position"],
        "font-variant-caps" => &["font-variant-caps"],
        "font-variant-numeric" => &["font-variant-numeric"],
        "font-variant-alternates" => &["font-variant-alternates"],
        "font-variant-east-asian" => &["font-variant-east-asian"],
        "font-variant-emoji" => &["font-variant-emoji"],
        "font-palette" => &["font-palette"],
        "page-break-before" | "break-before" => &["break-before"],
        "page-break-after" | "break-after" => &["break-after"],
        "page-break-inside" | "break-inside" => &["break-inside"],
        _ => return None,
    };
    Some(longhands.to_vec())
}

pub(in crate::css) fn logical_axis_side_names(name: &str) -> Option<[&'static str; 2]> {
    match name {
        "border-block" | "border-block-width" | "border-block-style" | "border-block-color" => {
            Some(["border-block-start", "border-block-end"])
        }
        "border-inline" | "border-inline-width" | "border-inline-style" | "border-inline-color" => {
            Some(["border-inline-start", "border-inline-end"])
        }
        _ => None,
    }
}

/// Returns the logical start/end side names for a box edge axis shorthand.
///
/// CSS Logical Properties defines `*-block` and `*-inline` margin/padding
/// shorthands as setting the start and end sides of that logical axis:
/// <https://www.w3.org/TR/css-logical-1/#box>.
pub(in crate::css) fn logical_box_axis_side_names(name: &str) -> Option<[&'static str; 2]> {
    match name {
        "margin-block" | "padding-block" | "inset-block" => Some(["block-start", "block-end"]),
        "margin-inline" | "padding-inline" | "inset-inline" => Some(["inline-start", "inline-end"]),
        _ => None,
    }
}

pub(in crate::css) fn logical_axis_sides(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<[BorderSide; 2]> {
    let [start, end] = logical_axis_side_names(name)?;
    Some([
        logical_border_side(start, direction, writing_mode)?,
        logical_border_side(end, direction, writing_mode)?,
    ])
}

pub(in crate::css) fn border_side_component_longhands(side: BorderSide) -> [&'static str; 3] {
    [
        physical_border_side_component_longhand(side, "width"),
        physical_border_side_component_longhand(side, "style"),
        physical_border_side_component_longhand(side, "color"),
    ]
}

pub(in crate::css) fn physical_border_side_component_longhand(
    side: BorderSide,
    component: &str,
) -> &'static str {
    match (side, component) {
        (BorderSide::Top, "width") => "border-top-width",
        (BorderSide::Right, "width") => "border-right-width",
        (BorderSide::Bottom, "width") => "border-bottom-width",
        (BorderSide::Left, "width") => "border-left-width",
        (BorderSide::Top, "style") => "border-top-style",
        (BorderSide::Right, "style") => "border-right-style",
        (BorderSide::Bottom, "style") => "border-bottom-style",
        (BorderSide::Left, "style") => "border-left-style",
        (BorderSide::Top, "color") => "border-top-color",
        (BorderSide::Right, "color") => "border-right-color",
        (BorderSide::Bottom, "color") => "border-bottom-color",
        (BorderSide::Left, "color") => "border-left-color",
        _ => unreachable!("invalid border side component"),
    }
}

pub(in crate::css) fn declaration_is_revert_layer(value: &str) -> bool {
    trim_css_value(value).eq_ignore_ascii_case("revert-layer")
}

pub(in crate::css) fn declaration_is_revert(value: &str) -> bool {
    trim_css_value(value).eq_ignore_ascii_case("revert")
}

/// Returns whether a prior declaration is erased by a later `revert`.
///
/// CSS Cascade Level 5 rolls author-origin `revert` back to user level,
/// user-origin `revert` back to UA level, and treats UA-origin `revert` like
/// `unset`:
/// <https://www.w3.org/TR/css-cascade-5/#revert>.
pub(in crate::css) fn same_or_stronger_reverted_origin(
    prior: &CascadedDeclaration<'_>,
    rollback: &CascadedDeclaration<'_>,
) -> bool {
    match rollback.origin {
        StylesheetOrigin::Author => prior.origin == StylesheetOrigin::Author,
        StylesheetOrigin::User => {
            matches!(
                prior.origin,
                StylesheetOrigin::User | StylesheetOrigin::Author
            )
        }
        StylesheetOrigin::UserAgent => prior.origin == StylesheetOrigin::UserAgent,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::css) enum CssWideDefaultKeyword {
    Initial,
    Inherit,
    Unset,
}

impl CssWideDefaultKeyword {
    pub(in crate::css) fn parse(value: &str) -> Option<Self> {
        match trim_css_value(value).to_ascii_lowercase().as_str() {
            "initial" => Some(Self::Initial),
            "inherit" => Some(Self::Inherit),
            "unset" => Some(Self::Unset),
            _ => None,
        }
    }
}

/// Applies CSS-wide defaulting keywords to modeled properties.
///
/// CSS Cascade Level 5 defines `initial`, `inherit`, and `unset` as defaulting
/// behaviors accepted by every property. Shorthands, including `all`, apply the
/// keyword to their longhands:
/// <https://www.w3.org/TR/css-cascade-5/#defaulting-keywords> and
/// <https://www.w3.org/TR/css-cascade-5/#all-shorthand>.
pub(in crate::css) fn apply_css_wide_default_keyword(
    style: &mut ComputedStyle,
    name: &str,
    keyword: CssWideDefaultKeyword,
    defaulted_style: &ComputedStyle,
) {
    if name.eq_ignore_ascii_case("all") {
        for longhand in ALL_MODELED_LONGHANDS {
            apply_css_wide_default_longhand(style, longhand, keyword, defaulted_style);
        }
        return;
    }
    if let Some(longhands) = affected_longhands(name, style.direction, style.writing_mode) {
        for longhand in longhands {
            apply_css_wide_default_longhand(style, longhand, keyword, defaulted_style);
        }
    } else {
        apply_css_wide_default_longhand(style, name, keyword, defaulted_style);
    }
}

pub(in crate::css) fn apply_css_wide_default_longhand(
    style: &mut ComputedStyle,
    name: &str,
    keyword: CssWideDefaultKeyword,
    defaulted_style: &ComputedStyle,
) {
    // `font-size` retains a deferred inheritance representation so a normal
    // inherited font can resolve against the immediate parent's used metric.
    // That representation is not correct for `initial`: CSS Cascade resets
    // an inherited property to its property's initial value, rather than
    // re-inheriting it after the defaulting pass:
    // <https://drafts.csswg.org/css-cascade-5/#initial> and
    // <https://drafts.csswg.org/css-fonts-4/#font-size-prop>.
    if name.eq_ignore_ascii_case("font-size") && keyword == CssWideDefaultKeyword::Initial {
        set_font_size(style, ComputedStyle::initial().font_size);
        return;
    }
    let initial;
    let source = match keyword {
        CssWideDefaultKeyword::Initial => {
            initial = ComputedStyle::initial();
            &initial
        }
        CssWideDefaultKeyword::Inherit => defaulted_style,
        CssWideDefaultKeyword::Unset if property_is_inherited(name) => defaulted_style,
        CssWideDefaultKeyword::Unset => {
            initial = ComputedStyle::initial();
            &initial
        }
    };
    copy_modeled_property(style, source, name);
    if name.eq_ignore_ascii_case("page") && matches!(style.page, PageAssignment::Unspecified) {
        style.page = PageAssignment::Auto;
    }
}

pub(in crate::css) fn same_cascade_layer(
    left: &CascadedDeclaration<'_>,
    right: &CascadedDeclaration<'_>,
) -> bool {
    left.origin == right.origin
        && left.important == right.important
        && left.layer_order == right.layer_order
}

/// Parses the CSS Box Model Level 4 `margin-trim` property.
///
/// The value is a set of trim-side keywords; `block` and `inline` expand to
/// both sides in that axis:
/// <https://drafts.csswg.org/css-box-4/#margin-trim>.
pub(in crate::css) fn parse_margin_trim(value: &str) -> Option<MarginTrim> {
    let mut trim = MarginTrim::NONE;
    let mut saw_token = false;
    let mut saw_none = false;
    for token in trim_css_value(value).split_whitespace() {
        saw_token = true;
        match token.to_ascii_lowercase().as_str() {
            "none" if !saw_none && trim == MarginTrim::NONE => saw_none = true,
            "none" => return None,
            "block" => {
                if saw_none {
                    return None;
                }
                trim.block_start = true;
                trim.block_end = true;
            }
            "block-start" => {
                if saw_none {
                    return None;
                }
                trim.block_start = true;
            }
            "block-end" => {
                if saw_none {
                    return None;
                }
                trim.block_end = true;
            }
            "inline" => {
                if saw_none {
                    return None;
                }
                trim.inline_start = true;
                trim.inline_end = true;
            }
            "inline-start" => {
                if saw_none {
                    return None;
                }
                trim.inline_start = true;
            }
            "inline-end" => {
                if saw_none {
                    return None;
                }
                trim.inline_end = true;
            }
            _ => return None,
        }
    }
    saw_token.then_some(trim)
}

pub(crate) fn apply_declarations(style: &mut ComputedStyle, declarations: &Declarations) {
    apply_declarations_with_origin(style, declarations, StylesheetOrigin::Author);
}

pub(crate) fn apply_declarations_with_origin(
    style: &mut ComputedStyle,
    declarations: &Declarations,
    origin: StylesheetOrigin,
) {
    let mut declarations = cascaded_declarations_from(declarations, origin);
    sort_cascaded_declarations(&mut declarations);
    apply_cascaded_declarations(style, &declarations);
}

pub(crate) fn apply_cascaded_declarations(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
) {
    let defaulted_style = style.clone();
    apply_cascaded_declarations_with_inheritance_source(style, declarations, &defaulted_style);
}

pub(crate) fn apply_cascaded_declarations_with_inheritance_source(
    style: &mut ComputedStyle,
    declarations: &[CascadedDeclaration<'_>],
    inheritance_source: &ComputedStyle,
) {
    let parent_ch_advance = fallback_ch_advance_for_style(inheritance_source);
    apply_cascaded_declarations_with_inheritance_source_and_parent_ch_advance(
        style,
        declarations,
        inheritance_source,
        parent_ch_advance,
        false,
        ColorSchemePreference::None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_cascade_keeps_local_values_non_inherited_and_composes_effective_scale() {
        let mut parent = ComputedStyle::initial();
        apply_declarations(&mut parent, &parse_declarations("zoom: 200%"));
        assert_eq!(parent.zoom.factor(), 2.0);
        assert_eq!(parent.effective_zoom.factor(), 2.0);

        let mut child = ComputedStyle::initial();
        let child_declarations = parse_declarations("zoom: inherit");
        let declarations =
            cascaded_declarations_from(&child_declarations, StylesheetOrigin::Author);
        apply_cascaded_declarations_with_inheritance_source(&mut child, &declarations, &parent);
        // Explicit inheritance copies the parent's local computed factor; the
        // child then composes that factor with the inherited effective scale.
        assert_eq!(child.zoom.factor(), 2.0);
        assert_eq!(child.effective_zoom.factor(), 4.0);

        let mut grandchild = ComputedStyle::initial();
        let empty_declarations = Declarations::new();
        let declarations =
            cascaded_declarations_from(&empty_declarations, StylesheetOrigin::Author);
        apply_cascaded_declarations_with_inheritance_source(&mut grandchild, &declarations, &child);
        assert_eq!(grandchild.zoom.factor(), 1.0);
        assert_eq!(grandchild.effective_zoom.factor(), 4.0);
    }
}
