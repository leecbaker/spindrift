use super::visual::{
    parse_aspect_ratio, parse_contain_intrinsic_size, parse_contain_intrinsic_size_component,
};
use super::*;

pub(in crate::css) fn apply_cascaded_sizing_declaration(
    style: &mut ComputedStyle,
    name: &str,
    value: &str,
    _declaration: &CascadedDeclaration<'_>,
    _inheritance_source: &ComputedStyle,
    _parent_ch_advance: LayoutLength,
) -> bool {
    match name {
        "width" => {
            style.box_values.width =
                parse_computed_box_size(value, style.font_size, style.root_font_size)
                    .unwrap_or(style.box_values.width.clone());
        }
        "height" => {
            if let Some(height) =
                parse_computed_box_size(value, style.font_size, style.root_font_size)
            {
                style.box_values.height = PhysicalHeight::from_computed(height);
            }
        }
        "aspect-ratio" => {
            if let Some(aspect_ratio) = parse_aspect_ratio(value) {
                style.aspect_ratio = aspect_ratio;
            }
        }
        "contain-intrinsic-size" => {
            if let Some(size) = parse_contain_intrinsic_size(value, style.font_size) {
                style.contain_intrinsic_size = size;
            }
        }
        "contain-intrinsic-width" => {
            if let Some(width) = parse_contain_intrinsic_size_component(value, style.font_size) {
                style.contain_intrinsic_size.width = width;
            }
        }
        "contain-intrinsic-height" => {
            if let Some(height) = parse_contain_intrinsic_size_component(value, style.font_size) {
                style.contain_intrinsic_size.height = height;
            }
        }
        "min-width" => {
            style.box_values.min_width =
                parse_computed_box_size(value, style.font_size, style.root_font_size)
                    .unwrap_or(style.box_values.min_width.clone());
        }
        "max-width" => {
            style.box_values.max_width = if value.trim().eq_ignore_ascii_case("none") {
                // `none` is the initial max-size value and removes the
                // constraint rather than preserving an inherited maximum.
                // <https://www.w3.org/TR/css-sizing-3/#preferred-size-properties>
                ComputedLengthPercentageOrAuto::Auto
            } else {
                parse_computed_box_size(value, style.font_size, style.root_font_size)
                    .unwrap_or(style.box_values.max_width.clone())
            };
        }
        "min-height" => {
            style.box_values.min_height =
                parse_computed_box_size(value, style.font_size, style.root_font_size)
                    .unwrap_or(style.box_values.min_height.clone());
        }
        "max-height" => {
            style.box_values.max_height = if value.trim().eq_ignore_ascii_case("none") {
                // See the matching `max-width` handling above.
                ComputedLengthPercentageOrAuto::Auto
            } else {
                parse_computed_box_size(value, style.font_size, style.root_font_size)
                    .unwrap_or(style.box_values.max_height.clone())
            };
        }
        "box-sizing" => {
            style.box_sizing = match value.to_ascii_lowercase().as_str() {
                "border-box" => BoxSizing::BorderBox,
                "content-box" => BoxSizing::ContentBox,
                _ => style.box_sizing,
            };
        }
        _ => return false,
    }
    true
}
