use super::*;
use std::num::NonZeroUsize;

pub(in crate::css) fn apply_cascaded_layout_declaration(
    style: &mut ComputedStyle,
    name: &str,
    value: &str,
    declaration: &CascadedDeclaration<'_>,
    _inheritance_source: &ComputedStyle,
    _parent_ch_advance: LayoutLength,
) -> bool {
    match name {
        "direction" => {
            if let Some(direction) = parse_direction(value) {
                style.direction = direction;
            }
        }
        "unicode-bidi" => {
            style.unicode_bidi = match value.trim().to_ascii_lowercase().as_str() {
                "normal" => UnicodeBidi::Normal,
                "embed" => UnicodeBidi::Embed,
                "isolate" => UnicodeBidi::Isolate,
                "bidi-override" => UnicodeBidi::BidiOverride,
                "isolate-override" => UnicodeBidi::IsolateOverride,
                "plaintext" => UnicodeBidi::Plaintext,
                _ => style.unicode_bidi,
            };
        }
        "writing-mode" => {
            if let Some(writing_mode) = parse_writing_mode(value) {
                style.writing_mode = writing_mode;
            }
        }
        "text-orientation" => {
            if let Some(text_orientation) = parse_text_orientation(value) {
                style.text_orientation = text_orientation;
            }
        }
        "text-combine-upright" => {
            if let Some(text_combine_upright) = parse_text_combine_upright(value) {
                style.text_combine_upright = text_combine_upright;
            }
        }
        "line-fit-edge" => {
            if let Some(line_fit_edge) = parse_line_fit_edge(value) {
                style.line_fit_edge = line_fit_edge;
            }
        }
        "text-box-trim" => {
            if let Some(text_box_trim) = parse_text_box_trim(value) {
                style.text_box_trim = text_box_trim;
            }
        }
        "text-box-edge" => {
            if let Some(text_box_edge) = parse_text_box_edge(value) {
                style.text_box_edge = text_box_edge;
            }
        }
        "text-box" => {
            if let Some((text_box_trim, text_box_edge)) = parse_text_box(value) {
                style.text_box_trim = text_box_trim;
                style.text_box_edge = text_box_edge;
            }
        }
        "initial-letter" => {
            if let Some(initial_letter) = parse_initial_letter(value) {
                style.initial_letter = initial_letter;
            }
        }
        "initial-letter-align" => {
            if let Some(initial_letter_align) = parse_initial_letter_align(value) {
                style.initial_letter_align = initial_letter_align;
            }
        }
        "initial-letter-wrap" => {
            if let Some(initial_letter_wrap) = parse_initial_letter_wrap(value, style.font_size) {
                style.initial_letter_wrap = initial_letter_wrap;
            }
        }
        "box-decoration-break" => {
            if let Some(box_decoration_break) = parse_box_decoration_break(value) {
                style.box_decoration_break = box_decoration_break;
            }
        }
        "display" => {
            style.display = parse_display(value, style.display);
            style.legacy_webkit_box = LegacyWebkitBox::from_specified_display(value);
            if style.legacy_webkit_box.is_present() {
                style.flex_wrap = FlexWrap::NoWrap;
                style.flex_line_count = FlexLineCount::Auto;
            }
        }
        "-webkit-box-orient" => {
            style.webkit_box_orient = match value.trim().to_ascii_lowercase().as_str() {
                "horizontal" => WebkitBoxOrient::Horizontal,
                "vertical" => WebkitBoxOrient::Vertical,
                _ => style.webkit_box_orient,
            };
        }
        "flex-direction" => {
            style.flex_direction = match value.to_ascii_lowercase().as_str() {
                "column" => FlexDirection::Column,
                "column-reverse" => FlexDirection::ColumnReverse,
                "row" => FlexDirection::Row,
                "row-reverse" => FlexDirection::RowReverse,
                _ => style.flex_direction,
            };
        }
        "flex-flow" => {
            if let Some((direction, wrap)) = parse_flex_flow(value) {
                style.flex_direction = direction;
                if !style.legacy_webkit_box.is_present() {
                    style.flex_wrap = wrap;
                }
            }
        }
        "justify-content" => {
            style.justify_content = parse_justify_content(value, style.justify_content);
        }
        "justify-items" => {
            style.justify_items = parse_justify_items(value, style.justify_items);
        }
        "justify-self" => {
            style.justify_self = parse_justify_self(value, style.justify_self);
        }
        "align-content" => {
            style.align_content = parse_align_content(value, style.align_content);
        }
        "align-items" => {
            style.align_items = parse_align_items(value, style.align_items);
        }
        "align-self" => {
            style.align_self = parse_align_self(value, style.align_self);
        }
        "flex-wrap" => {
            let mut wrap = None;
            let mut balance = false;
            for token in value.split_ascii_whitespace() {
                match token.to_ascii_lowercase().as_str() {
                    "wrap" if wrap.is_none() => wrap = Some(FlexWrap::Wrap),
                    "wrap-reverse" if wrap.is_none() => wrap = Some(FlexWrap::WrapReverse),
                    "nowrap" if wrap.is_none() => wrap = Some(FlexWrap::NoWrap),
                    "balance" if !balance => balance = true,
                    _ => {
                        wrap = None;
                        balance = false;
                        break;
                    }
                }
            }
            if style.legacy_webkit_box.is_present() {
                style.flex_wrap = FlexWrap::NoWrap;
            } else if balance {
                style.flex_wrap = match wrap.unwrap_or(FlexWrap::Wrap) {
                    FlexWrap::WrapReverse => FlexWrap::BalanceReverse,
                    FlexWrap::Wrap | FlexWrap::Balance | FlexWrap::BalanceReverse => {
                        FlexWrap::Balance
                    }
                    FlexWrap::NoWrap => style.flex_wrap,
                };
            } else if let Some(wrap) = wrap {
                style.flex_wrap = wrap;
            }
        }
        "flex-line-count" => {
            if let Ok(value) = value.parse::<usize>()
                && value > 0
            {
                style.flex_line_count = FlexLineCount::Count(
                    NonZeroUsize::new(value).expect("positive flex line count"),
                );
            }
        }
        "flex-grow" => {
            if let Some(value) = parse_css_number(value)
                && value >= 0.0
            {
                style.flex_grow = value;
            }
        }
        "flex-shrink" => {
            if let Some(value) = parse_css_number(value)
                && value >= 0.0
            {
                style.flex_shrink = value;
            }
        }
        "flex-basis" => {
            if let Some(basis) = parse_computed_flex_basis(value, style.font_size) {
                style.flex_basis = basis;
            }
        }
        "order" => {
            if let Ok(value) = value.parse::<i32>() {
                style.order = value;
            }
        }
        "flex" => {
            if let Some((grow, shrink, basis)) = parse_flex_shorthand_components(value) {
                if let Some(grow) = parse_css_number(&grow) {
                    style.flex_grow = grow;
                }
                if let Some(shrink) = parse_css_number(&shrink) {
                    style.flex_shrink = shrink;
                }
                if let Some(basis) = parse_computed_flex_basis(&basis, style.font_size) {
                    style.flex_basis = basis;
                }
            }
        }
        "gap" | "grid-gap" => {
            let parts = split_css_component_values(value);
            if let Some((row_gap, column_gap)) =
                parse_gap_shorthand_components(&parts, style.font_size)
            {
                style.row_gap = row_gap;
                style.column_gap = column_gap;
            }
        }
        "row-gap" | "grid-row-gap" => {
            if let Some(gap) = parse_gap(value, style.font_size) {
                style.row_gap = gap;
            }
        }
        "column-count" => {
            if let Some(count) = parse_column_count(value) {
                style.column_count = count;
            }
        }
        "column-width" => {
            if let Some(width) = parse_column_width(value, style.font_size) {
                style.column_width = width;
            }
        }
        "column-height" => {
            if let Some(height) = parse_column_height(value, style.font_size) {
                style.column_height = height;
            }
        }
        "column-wrap" => {
            if let Some(wrap) = parse_column_wrap(value) {
                style.column_wrap = wrap;
            }
        }
        "column-fill" => {
            if let Some(fill) = parse_column_fill(value) {
                style.column_fill = fill;
            }
        }
        "column-span" => {
            if let Some(span) = parse_column_span(value) {
                style.column_span = span;
            }
        }
        "column-gap" | "grid-column-gap" => {
            if let Some(gap) = parse_column_gap(value, style.font_size) {
                style.column_gap = gap;
            }
        }
        "column-rule" => apply_gap_rule_shorthand(value, style, GapRuleDeclarationAxis::Column),
        "row-rule" => apply_gap_rule_shorthand(value, style, GapRuleDeclarationAxis::Row),
        "rule" => {
            apply_gap_rule_shorthand(value, style, GapRuleDeclarationAxis::Column);
            apply_gap_rule_shorthand(value, style, GapRuleDeclarationAxis::Row);
        }
        "column-rule-width" => apply_gap_rule_width(value, style, GapRuleDeclarationAxis::Column),
        "row-rule-width" => apply_gap_rule_width(value, style, GapRuleDeclarationAxis::Row),
        "rule-width" => {
            apply_gap_rule_width(value, style, GapRuleDeclarationAxis::Column);
            apply_gap_rule_width(value, style, GapRuleDeclarationAxis::Row);
        }
        "column-rule-style" => apply_gap_rule_style(value, style, GapRuleDeclarationAxis::Column),
        "row-rule-style" => apply_gap_rule_style(value, style, GapRuleDeclarationAxis::Row),
        "rule-style" => {
            apply_gap_rule_style(value, style, GapRuleDeclarationAxis::Column);
            apply_gap_rule_style(value, style, GapRuleDeclarationAxis::Row);
        }
        "column-rule-color" => apply_gap_rule_color(value, style, GapRuleDeclarationAxis::Column),
        "row-rule-color" => apply_gap_rule_color(value, style, GapRuleDeclarationAxis::Row),
        "rule-color" => {
            apply_gap_rule_color(value, style, GapRuleDeclarationAxis::Column);
            apply_gap_rule_color(value, style, GapRuleDeclarationAxis::Row);
        }
        "column-rule-break" => apply_gap_rule_break(value, style, GapRuleDeclarationAxis::Column),
        "row-rule-break" => apply_gap_rule_break(value, style, GapRuleDeclarationAxis::Row),
        "rule-break" => {
            apply_gap_rule_break(value, style, GapRuleDeclarationAxis::Column);
            apply_gap_rule_break(value, style, GapRuleDeclarationAxis::Row);
        }
        "column-rule-visibility-items" => {
            apply_gap_rule_visibility_items(value, style, GapRuleDeclarationAxis::Column)
        }
        "row-rule-visibility-items" => {
            apply_gap_rule_visibility_items(value, style, GapRuleDeclarationAxis::Row)
        }
        "rule-visibility-items" => {
            apply_gap_rule_visibility_items(value, style, GapRuleDeclarationAxis::Column);
            apply_gap_rule_visibility_items(value, style, GapRuleDeclarationAxis::Row);
        }
        "rule-overlap" => {
            if let Some(overlap) = parse_gap_rule_overlap(value) {
                style.rule_overlap = overlap;
            }
        }
        "column-rule-inset"
        | "row-rule-inset"
        | "rule-inset"
        | "column-rule-inset-start"
        | "column-rule-inset-end"
        | "row-rule-inset-start"
        | "row-rule-inset-end"
        | "rule-inset-start"
        | "rule-inset-end"
        | "column-rule-inset-cap"
        | "column-rule-inset-junction"
        | "row-rule-inset-cap"
        | "row-rule-inset-junction"
        | "rule-inset-cap"
        | "rule-inset-junction"
        | "column-rule-inset-cap-start"
        | "column-rule-inset-cap-end"
        | "column-rule-inset-junction-start"
        | "column-rule-inset-junction-end"
        | "row-rule-inset-cap-start"
        | "row-rule-inset-cap-end"
        | "row-rule-inset-junction-start"
        | "row-rule-inset-junction-end" => apply_gap_rule_inset_property(name, value, style),
        "grid-template-rows" => {
            if let Some(tracks) = parse_grid_track_list(value, style.font_size) {
                style.grid_template_rows = tracks;
            }
        }
        "grid-template-columns" => {
            if let Some(tracks) = parse_grid_track_list(value, style.font_size) {
                style.grid_template_columns = tracks;
            }
        }
        "grid-template-areas" => {
            if let Some(areas) = parse_grid_template_areas(value) {
                style.grid_template_areas = areas;
            }
        }
        "grid-auto-rows" => {
            if let Some(tracks) = parse_grid_auto_track_list(value, style.font_size) {
                style.grid_auto_rows = tracks;
            }
        }
        "grid-auto-columns" => {
            if let Some(tracks) = parse_grid_auto_track_list(value, style.font_size) {
                style.grid_auto_columns = tracks;
            }
        }
        "grid-auto-flow" => {
            if let Some(flow) = parse_grid_auto_flow(value) {
                style.grid_auto_flow = flow;
            }
        }
        "grid-lanes-direction" => {
            if let Some(direction) = parse_grid_lanes_direction(value) {
                style.grid_lanes_direction = direction;
            }
        }
        "flow-tolerance" => {
            if let Some(tolerance) = parse_grid_lanes_flow_tolerance(value, style.font_size) {
                style.grid_lanes_flow_tolerance = tolerance;
            }
        }
        "grid-row-start" => {
            if let Some(placement) = parse_grid_placement(value) {
                style.grid_row_start = placement;
            }
        }
        "grid-row-end" => {
            if let Some(placement) = parse_grid_placement(value) {
                style.grid_row_end = placement;
            }
        }
        "grid-column-start" => {
            if let Some(placement) = parse_grid_placement(value) {
                style.grid_column_start = placement;
            }
        }
        "grid-column-end" => {
            if let Some(placement) = parse_grid_placement(value) {
                style.grid_column_end = placement;
            }
        }
        "columns" => apply_columns(value, style),
        "margin-trim" => {
            if let Some(margin_trim) = parse_margin_trim(value) {
                style.margin_trim = margin_trim;
            }
        }
        "margin-block" | "margin-inline" => {
            apply_logical_margin_axis(value, style, name, declaration.origin)
        }
        "margin-block-start" | "margin-block-end" | "margin-inline-start" | "margin-inline-end" => {
            apply_logical_margin_side(value, style, name, declaration.origin)
        }
        "margin" => {
            if let Some(typed) = parse_margin_edge_values(value, style.font_size) {
                style.box_values.margin = typed.clone();
                style.margin = legacy_margin_edges(typed);
                style.ua_margin_em = if declaration.origin == StylesheetOrigin::UserAgent {
                    parse_margin_em_edges(value)
                } else {
                    OptionalEdges::NONE
                };
            }
        }
        "margin-top" => set_margin_side(value, style.font_size, |typed| {
            style.box_values.margin.top = typed.clone();
            style.margin.top = typed.length_if_no_percent().unwrap_or(0.0);
            style.ua_margin_em.top = if declaration.origin == StylesheetOrigin::UserAgent {
                parse_em_length_factor(value)
            } else {
                None
            };
        }),
        "margin-right" => set_margin_side(value, style.font_size, |typed| {
            style.box_values.margin.right = typed.clone();
            style.margin.right = typed.length_if_no_percent().unwrap_or(0.0);
            style.ua_margin_em.right = if declaration.origin == StylesheetOrigin::UserAgent {
                parse_em_length_factor(value)
            } else {
                None
            };
        }),
        "margin-bottom" => set_margin_side(value, style.font_size, |typed| {
            style.box_values.margin.bottom = typed.clone();
            style.margin.bottom = typed.length_if_no_percent().unwrap_or(0.0);
            style.ua_margin_em.bottom = if declaration.origin == StylesheetOrigin::UserAgent {
                parse_em_length_factor(value)
            } else {
                None
            };
        }),
        "margin-left" => set_margin_side(value, style.font_size, |typed| {
            style.box_values.margin.left = typed.clone();
            style.margin.left = typed.length_if_no_percent().unwrap_or(0.0);
            style.ua_margin_em.left = if declaration.origin == StylesheetOrigin::UserAgent {
                parse_em_length_factor(value)
            } else {
                None
            };
        }),
        "padding-block" | "padding-inline" => apply_logical_padding_axis(value, style, name),
        "padding-block-start"
        | "padding-block-end"
        | "padding-inline-start"
        | "padding-inline-end" => apply_logical_padding_side(value, style, name),
        "padding" => {
            if let Some(typed) = parse_edge_values(value, style.font_size) {
                style.box_values.padding = typed.clone();
                if let Some(edges) = legacy_edge_lengths(typed) {
                    style.padding = edges;
                }
            }
        }
        "padding-top" => set_computed_length_percentage(value, style.font_size, |typed| {
            style.box_values.padding.top = typed.clone();
            if let Some(length) = typed.length_if_no_percent() {
                style.padding.top = length;
            }
        }),
        "padding-right" => set_computed_length_percentage(value, style.font_size, |typed| {
            style.box_values.padding.right = typed.clone();
            if let Some(length) = typed.length_if_no_percent() {
                style.padding.right = length;
            }
        }),
        "padding-bottom" => set_computed_length_percentage(value, style.font_size, |typed| {
            style.box_values.padding.bottom = typed.clone();
            if let Some(length) = typed.length_if_no_percent() {
                style.padding.bottom = length;
            }
        }),
        "padding-left" => set_computed_length_percentage(value, style.font_size, |typed| {
            style.box_values.padding.left = typed.clone();
            if let Some(length) = typed.length_if_no_percent() {
                style.padding.left = length;
            }
        }),
        "border" => apply_border(value, style, None),
        "border-top" => apply_border(value, style, Some(BorderSide::Top)),
        "border-right" => apply_border(value, style, Some(BorderSide::Right)),
        "border-bottom" => apply_border(value, style, Some(BorderSide::Bottom)),
        "border-left" => apply_border(value, style, Some(BorderSide::Left)),
        "border-block" | "border-inline" => apply_logical_border_axis(value, style, name),
        "border-block-start" | "border-block-end" | "border-inline-start" | "border-inline-end" => {
            apply_logical_border(value, style, name)
        }
        "border-width" => {
            if let Some(edges) = parse_border_width_edges(value, style.font_size) {
                style.border_width_values = edges.clone();
                style.border_widths = Edges {
                    top: edges
                        .top
                        .length_if_no_percent()
                        .unwrap_or(edges.top.length_points()),
                    right: edges
                        .right
                        .length_if_no_percent()
                        .unwrap_or(edges.right.length_points()),
                    bottom: edges
                        .bottom
                        .length_if_no_percent()
                        .unwrap_or(edges.bottom.length_points()),
                    left: edges
                        .left
                        .length_if_no_percent()
                        .unwrap_or(edges.left.length_points()),
                };
                style.border_width = max_edge(style.border_widths);
            }
        }
        "border-block-width" => {
            if let Some([start, end]) = parse_logical_border_widths(value, style.font_size)
                && let Some([start_side, end_side]) =
                    logical_axis_sides(name, style.direction, style.writing_mode)
            {
                set_border_side_width(style, start_side, start);
                set_border_side_width(style, end_side, end);
            }
        }
        "border-inline-width" => {
            if let Some([start, end]) = parse_logical_border_widths(value, style.font_size)
                && let Some([start_side, end_side]) =
                    logical_axis_sides(name, style.direction, style.writing_mode)
            {
                set_border_side_width(style, start_side, start);
                set_border_side_width(style, end_side, end);
            }
        }
        "border-top-width" => {
            if let Some(length) = parse_computed_border_width(value, style.font_size) {
                set_border_side_width(style, BorderSide::Top, length);
            }
        }
        "border-right-width" => {
            if let Some(length) = parse_computed_border_width(value, style.font_size) {
                set_border_side_width(style, BorderSide::Right, length);
            }
        }
        "border-bottom-width" => {
            if let Some(length) = parse_computed_border_width(value, style.font_size) {
                set_border_side_width(style, BorderSide::Bottom, length);
            }
        }
        "border-left-width" => {
            if let Some(length) = parse_computed_border_width(value, style.font_size) {
                set_border_side_width(style, BorderSide::Left, length);
            }
        }
        "border-block-start-width"
        | "border-block-end-width"
        | "border-inline-start-width"
        | "border-inline-end-width" => {
            if let Some(side) = logical_border_side(name, style.direction, style.writing_mode)
                && let Some(length) = parse_computed_border_width(value, style.font_size)
            {
                set_border_side_width(style, side, length);
            }
        }
        "border-color" => {
            if let Some(colors) = parse_border_colors(value, style.color) {
                style.border_colors = colors;
                style.border_color = colors.top;
            } else if let Some(color) = parse_border_color(value, style.color) {
                style.border_color = color;
                style.border_colors = border_colors_all(color);
            }
        }
        "border-top-color" => {
            if let Some(color) = parse_border_color(value, style.color) {
                style.border_colors.top = color;
                style.border_color = color;
            }
        }
        "border-right-color" => {
            if let Some(color) = parse_border_color(value, style.color) {
                style.border_colors.right = color;
            }
        }
        "border-bottom-color" => {
            if let Some(color) = parse_border_color(value, style.color) {
                style.border_colors.bottom = color;
            }
        }
        "border-left-color" => {
            if let Some(color) = parse_border_color(value, style.color) {
                style.border_colors.left = color;
            }
        }
        "border-block-color" => {
            if let Some([start, end]) = parse_logical_border_colors(value, style.color)
                && let Some([start_side, end_side]) =
                    logical_axis_sides(name, style.direction, style.writing_mode)
            {
                set_border_side_color(style, start_side, start);
                set_border_side_color(style, end_side, end);
            }
        }
        "border-inline-color" => {
            if let Some([start, end]) = parse_logical_border_colors(value, style.color)
                && let Some([start_side, end_side]) =
                    logical_axis_sides(name, style.direction, style.writing_mode)
            {
                set_border_side_color(style, start_side, start);
                set_border_side_color(style, end_side, end);
            }
        }
        "border-block-start-color"
        | "border-block-end-color"
        | "border-inline-start-color"
        | "border-inline-end-color" => {
            if let Some(side) = logical_border_side(name, style.direction, style.writing_mode)
                && let Some(color) = parse_border_color(value, style.color)
            {
                set_border_side_color(style, side, color);
            }
        }
        "border-style" => {
            if let Some(styles) = parse_border_styles(value) {
                style.border_styles = styles;
                materialize_visible_border_widths(style);
            }
        }
        "outline-width" => {
            if let Some(length) = parse_computed_border_width(value, style.font_size) {
                style.outline_width_value = length.clone();
                style.outline_width = length
                    .length_if_no_percent()
                    .unwrap_or(length.length_points());
            }
        }
        "outline-style" => {
            if let Some(outline_style) = parse_border_style(value) {
                style.outline_style = outline_style;
            }
        }
        "outline-color" => {
            if let Some(color) = parse_border_color(value, style.color) {
                style.outline_color = color;
            }
        }
        _ => return false,
    }
    true
}

pub(in crate::css) fn parse_text_box_trim(value: &str) -> Option<TextBoxTrim> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(TextBoxTrim::None),
        "trim-start" => Some(TextBoxTrim::TrimStart),
        "trim-end" => Some(TextBoxTrim::TrimEnd),
        "trim-both" => Some(TextBoxTrim::TrimBoth),
        _ => None,
    }
}

pub(in crate::css) fn parse_box_decoration_break(value: &str) -> Option<BoxDecorationBreak> {
    match value.trim().to_ascii_lowercase().as_str() {
        "slice" => Some(BoxDecorationBreak::Slice),
        "clone" => Some(BoxDecorationBreak::Clone),
        _ => None,
    }
}

pub(in crate::css) fn parse_initial_letter(value: &str) -> Option<InitialLetter> {
    let parts = split_css_component_values(trim_css_value(value));
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("normal") {
        return Some(InitialLetter::Normal);
    }
    let mut size = None;
    let mut sink = None;
    let mut keyword = None;
    for part in parts {
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "normal" => return None,
            "drop" | "raise" => {
                if keyword.replace(lower).is_some() {
                    return None;
                }
            }
            _ => {
                if part.contains('.') {
                    let parsed = part.parse::<f32>().ok()?;
                    if parsed < 1.0 || !parsed.is_finite() || size.replace(parsed).is_some() {
                        return None;
                    }
                } else if let Ok(integer) = part.parse::<u32>() {
                    if integer == 0 {
                        return None;
                    }
                    if size.is_none() {
                        size = Some(integer as f32);
                    } else if sink.replace(integer).is_some() {
                        return None;
                    }
                } else {
                    let parsed = part.parse::<f32>().ok()?;
                    if parsed < 1.0 || !parsed.is_finite() || size.replace(parsed).is_some() {
                        return None;
                    }
                }
            }
        }
    }
    let size = size?;
    let sink = match (sink, keyword.as_deref()) {
        (Some(sink), None) => sink,
        (Some(_), Some(_)) => return None,
        (None, Some("raise")) => 1,
        (None, Some("drop")) | (None, None) => size.floor().max(1.0) as u32,
        (None, Some(_)) => return None,
    };
    Some(InitialLetter::Specified { size, sink })
}

pub(in crate::css) fn parse_initial_letter_align(value: &str) -> Option<InitialLetterAlign> {
    let parts = split_css_component_values(trim_css_value(value));
    if parts.is_empty() || parts.len() > 2 {
        return None;
    }
    let mut border_box = false;
    let mut keyword = None;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "border-box" if !border_box => border_box = true,
            "alphabetic" if keyword.is_none() => {
                keyword = Some(InitialLetterAlignKeyword::Alphabetic);
            }
            "ideographic" if keyword.is_none() => {
                keyword = Some(InitialLetterAlignKeyword::Ideographic);
            }
            "hanging" if keyword.is_none() => keyword = Some(InitialLetterAlignKeyword::Hanging),
            "leading" if keyword.is_none() => keyword = Some(InitialLetterAlignKeyword::Leading),
            _ => return None,
        }
    }
    if !border_box && keyword.is_none() {
        return None;
    }
    Some(InitialLetterAlign {
        border_box,
        keyword: keyword.unwrap_or(InitialLetterAlignKeyword::Alphabetic),
    })
}

pub(in crate::css) fn parse_initial_letter_wrap(
    value: &str,
    font_size: f32,
) -> Option<InitialLetterWrap> {
    let value = trim_css_value(value);
    match value.to_ascii_lowercase().as_str() {
        "none" => Some(InitialLetterWrap::None),
        "first" => Some(InitialLetterWrap::First),
        "all" => Some(InitialLetterWrap::All),
        "grid" => Some(InitialLetterWrap::Grid),
        _ => parse_computed_length_percentage(value, font_size).map(InitialLetterWrap::Offset),
    }
}

pub(in crate::css) fn parse_text_box_edge(value: &str) -> Option<TextBoxEdge> {
    let parts = split_css_component_values(value);
    parse_text_box_edge_components(&parts)
}

pub(in crate::css) fn parse_line_fit_edge(value: &str) -> Option<LineFitEdge> {
    let parts = split_css_component_values(value);
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("leading") {
        return Some(LineFitEdge::Leading);
    }
    parse_text_edge_pair_components(&parts).map(LineFitEdge::Text)
}

pub(in crate::css) fn parse_text_box(value: &str) -> Option<(TextBoxTrim, TextBoxEdge)> {
    let parts = split_css_component_values(value);
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("normal") {
        return Some((TextBoxTrim::None, TextBoxEdge::Auto));
    }
    let mut trim = None;
    let mut edge_parts = Vec::new();
    for part in &parts {
        if part.eq_ignore_ascii_case("normal") {
            return None;
        }
        if let Some(parsed_trim) = parse_text_box_trim(part) {
            if trim.replace(parsed_trim).is_some() {
                return None;
            }
            continue;
        }
        edge_parts.push(*part);
    }
    let edge = if edge_parts.is_empty() {
        TextBoxEdge::Auto
    } else {
        parse_text_box_edge_components(&edge_parts)?
    };
    Some((trim.unwrap_or(TextBoxTrim::TrimBoth), edge))
}

fn parse_text_box_edge_components(parts: &[&str]) -> Option<TextBoxEdge> {
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("auto") {
        return Some(TextBoxEdge::Auto);
    }
    parse_text_edge_pair_components(parts).map(TextBoxEdge::Text)
}

fn parse_text_edge_pair_components(parts: &[&str]) -> Option<TextEdgePair> {
    match parts {
        [single] => {
            let metric = parse_text_edge_metric(single)?;
            let over = if metric.can_resolve_over_edge() {
                metric
            } else {
                TextEdgeMetric::Text
            };
            let under = if metric.can_resolve_under_edge() {
                metric
            } else {
                TextEdgeMetric::Text
            };
            Some(TextEdgePair::new(over, under))
        }
        [over, under] => {
            let over = parse_text_edge_metric(over)?;
            let under = parse_text_edge_metric(under)?;
            if !over.can_resolve_over_edge() || !under.can_resolve_under_edge() {
                return None;
            }
            Some(TextEdgePair::new(over, under))
        }
        _ => None,
    }
}

fn parse_text_edge_metric(value: &str) -> Option<TextEdgeMetric> {
    match value.trim().to_ascii_lowercase().as_str() {
        "text" => Some(TextEdgeMetric::Text),
        "cap" => Some(TextEdgeMetric::Cap),
        "ex" => Some(TextEdgeMetric::Ex),
        "ideographic" => Some(TextEdgeMetric::Ideographic),
        "ideographic-ink" => Some(TextEdgeMetric::IdeographicInk),
        "alphabetic" => Some(TextEdgeMetric::Alphabetic),
        _ => None,
    }
}
