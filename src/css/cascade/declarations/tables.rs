use super::*;

pub(in crate::css) fn apply_cascaded_table_declaration(
    style: &mut ComputedStyle,
    name: &str,
    value: &str,
    declaration: &CascadedDeclaration<'_>,
    _inheritance_source: &ComputedStyle,
    _parent_ch_advance: LayoutLength,
) -> bool {
    match name {
        "border-collapse" => {
            style.border_collapse = match value.to_ascii_lowercase().as_str() {
                "collapse" => BorderCollapse::Collapse,
                "separate" => BorderCollapse::Separate,
                _ => style.border_collapse,
            };
        }
        "caption-side" => {
            style.caption_side = match value.to_ascii_lowercase().as_str() {
                "top" => CaptionSide::Top,
                "bottom" => CaptionSide::Bottom,
                _ => style.caption_side,
            };
        }
        "table-layout" => {
            style.table_layout = match value.to_ascii_lowercase().as_str() {
                "auto" => TableLayout::Auto,
                "fixed" => TableLayout::Fixed,
                _ => style.table_layout,
            };
        }
        "empty-cells" => {
            style.empty_cells = match value.to_ascii_lowercase().as_str() {
                "show" => EmptyCells::Show,
                "hide" => EmptyCells::Hide,
                _ => style.empty_cells,
            };
        }
        "border-spacing" => {
            if let Some(spacing) = parse_border_spacing(value, style.font_size) {
                style.border_spacing = TableBorderSpacing::from_declaration(
                    spacing,
                    declaration.origin == StylesheetOrigin::Author,
                );
            }
        }
        _ => return false,
    }
    true
}
