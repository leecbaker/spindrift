use super::visual::parse_z_index;
use super::*;

pub(in crate::css) fn apply_cascaded_positioning_declaration(
    style: &mut ComputedStyle,
    name: &str,
    value: &str,
    _declaration: &CascadedDeclaration<'_>,
    _inheritance_source: &ComputedStyle,
    _parent_ch_advance: LayoutLength,
) -> bool {
    match name {
        "left" => {
            style.box_values.inset_left =
                parse_computed_length_percentage_auto(value, style.font_size)
                    .unwrap_or(style.box_values.inset_left.clone());
        }
        "top" => {
            style.box_values.inset_top =
                parse_computed_length_percentage_auto(value, style.font_size)
                    .unwrap_or(style.box_values.inset_top.clone());
        }
        "right" => {
            style.box_values.inset_right =
                parse_computed_length_percentage_auto(value, style.font_size)
                    .unwrap_or(style.box_values.inset_right.clone());
        }
        "bottom" => {
            style.box_values.inset_bottom =
                parse_computed_length_percentage_auto(value, style.font_size)
                    .unwrap_or(style.box_values.inset_bottom.clone());
        }
        "position" => {
            if let Some(name) = parse_running_position(value) {
                // CSS GCPM running elements are removed from normal flow
                // and become available to page-margin `element()`.
                // https://www.w3.org/TR/css-gcpm-3/#running-elements
                style.position = Position::Running(RunningElementName::new(name));
            } else {
                style.position = match value.to_ascii_lowercase().as_str() {
                    "absolute" => Position::Absolute,
                    "fixed" => Position::Fixed,
                    "sticky" => Position::Sticky,
                    "relative" => Position::Relative,
                    "static" => Position::Static,
                    _ => style.position.clone(),
                };
            }
        }
        "float" => {
            style.float = match value.to_ascii_lowercase().as_str() {
                "left" => Float::Left,
                "right" => Float::Right,
                "inline-start" => Float::InlineStart,
                "inline-end" => Float::InlineEnd,
                "footnote" => Float::Footnote,
                "none" => Float::None,
                _ => style.float,
            };
        }
        "footnote-display" => {
            style.footnote_display = match value.to_ascii_lowercase().as_str() {
                "block" => FootnoteDisplay::Block,
                "inline" => FootnoteDisplay::Inline,
                "compact" => FootnoteDisplay::Compact,
                _ => style.footnote_display,
            };
        }
        "footnote-policy" => {
            style.footnote_policy = match value.to_ascii_lowercase().as_str() {
                "auto" => FootnotePolicy::Auto,
                "line" => FootnotePolicy::Line,
                "block" => FootnotePolicy::Block,
                _ => style.footnote_policy,
            };
        }
        "clear" => {
            style.clear = match value.to_ascii_lowercase().as_str() {
                "left" => Clear::Left,
                "right" => Clear::Right,
                "both" => Clear::Both,
                "inline-start" => Clear::InlineStart,
                "inline-end" => Clear::InlineEnd,
                "none" => Clear::None,
                _ => style.clear,
            };
        }
        "z-index" => {
            let value = value.trim();
            style.z_index = if value.eq_ignore_ascii_case("auto") {
                ZIndex::Auto
            } else {
                parse_z_index(value)
                    .map(ZIndex::StackLevel)
                    .unwrap_or(style.z_index)
            };
        }
        _ => return false,
    }
    true
}
