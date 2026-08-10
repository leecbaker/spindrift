use super::*;
use crate::css::component_values::{split_css_component_values, split_css_top_level_delimiter};
use crate::css::{ParsedImage, parse_css_image};

/// Parse `border-image-source`.
///
/// CSS Backgrounds and Borders defines the initial value as `none` and accepts
/// image values:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-source>.
pub(crate) fn parse_border_image_source(value: &str) -> Option<ComputedImage> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        Some(ComputedImage::None)
    } else {
        match parse_css_image(value, None, None) {
            ParsedImage::Image(image) => Some(image),
            ParsedImage::NotAnImage | ParsedImage::SyntaxError => None,
        }
    }
}

/// Parse the `border-image` shorthand.
///
/// CSS Backgrounds and Borders Level 3 defines `border-image` as a shorthand
/// for source, slice, width, outset, and repeat. Unspecified longhands reset to
/// their initial values, while slash-separated width/outset groups are only
/// valid when a slice group is present:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-border-image>.
pub(crate) fn parse_border_image(value: &str, font_size: f32) -> Option<BorderImage> {
    let groups = split_css_top_level_delimiter(value, '/');
    if groups.is_empty() || groups.len() > 3 {
        return None;
    }

    let mut image = BorderImage::initial();
    let mut source_seen = false;
    let mut slice_tokens = Vec::new();
    let mut repeat_tokens = Vec::new();
    let mut width_tokens = Vec::new();
    let mut outset_tokens = Vec::new();

    for part in split_css_component_values(groups[0]) {
        if parse_border_image_source_token(part, &mut image, &mut source_seen)? {
            continue;
        }
        if parse_border_image_repeat_keyword(part).is_some() {
            repeat_tokens.push(part);
            continue;
        }
        slice_tokens.push(part);
    }

    for part in groups
        .get(1)
        .into_iter()
        .flat_map(|group| split_css_component_values(group))
    {
        if parse_border_image_source_token(part, &mut image, &mut source_seen)? {
            continue;
        }
        if parse_border_image_repeat_keyword(part).is_some() {
            repeat_tokens.push(part);
            continue;
        }
        width_tokens.push(part);
    }

    for part in groups
        .get(2)
        .into_iter()
        .flat_map(|group| split_css_component_values(group))
    {
        if parse_border_image_source_token(part, &mut image, &mut source_seen)? {
            continue;
        }
        if parse_border_image_repeat_keyword(part).is_some() {
            repeat_tokens.push(part);
            continue;
        }
        outset_tokens.push(part);
    }

    if groups.len() > 1 && slice_tokens.is_empty() {
        return None;
    }
    if !slice_tokens.is_empty() {
        image.slice = parse_border_image_slice(&slice_tokens.join(" "))?;
    }
    if !width_tokens.is_empty() {
        image.width = parse_border_image_width(&width_tokens.join(" "), font_size)?;
    }
    if !outset_tokens.is_empty() {
        image.outset = parse_border_image_outset(&outset_tokens.join(" "), font_size)?;
    }
    if !repeat_tokens.is_empty() {
        image.repeat = parse_border_image_repeat(&repeat_tokens.join(" "))?;
    }

    Some(image)
}

/// Parse the source subproperty of the CSS Masking `mask-border` shorthand.
///
/// Its source/slice/width/outset/repeat grammar is the border-image grammar,
/// with an additional optional `alpha`/`luminance` mask-border mode. The mode
/// is irrelevant to CSS Transforms' grouping decision, so this adapter removes
/// it and delegates the remaining grammar to the typed border-image parser.
/// <https://drafts.csswg.org/css-masking/#propdef-mask-border>
pub(crate) fn parse_mask_border_source(value: &str, font_size: f32) -> Option<ComputedImage> {
    if trim_css_value(value).is_empty() {
        return None;
    }
    let mut saw_mode = false;
    let mut border_image_tokens = Vec::new();
    for token in split_css_component_values(value) {
        if matches!(token.to_ascii_lowercase().as_str(), "alpha" | "luminance") {
            if saw_mode {
                return None;
            }
            saw_mode = true;
        } else {
            border_image_tokens.push(token);
        }
    }
    Some(parse_border_image(&border_image_tokens.join(" "), font_size)?.source)
}

pub(in crate::css) fn parse_border_image_source_token(
    token: &str,
    image: &mut BorderImage,
    source_seen: &mut bool,
) -> Option<bool> {
    let source = parse_border_image_source(token);
    if let Some(source) = source {
        if *source_seen {
            return None;
        }
        image.source = source;
        *source_seen = true;
        Some(true)
    } else {
        Some(false)
    }
}

/// Parse `border-image-slice`.
///
/// Unitless numbers and percentages are stored until the source image size is
/// known at used-value time; an optional `fill` keyword is preserved:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>.
pub(crate) fn parse_border_image_slice(value: &str) -> Option<BorderImageSlice> {
    let mut fill = false;
    let mut offsets = Vec::new();
    for part in split_css_component_values(value) {
        if part.eq_ignore_ascii_case("fill") {
            if fill {
                return None;
            }
            fill = true;
            continue;
        }
        offsets.push(parse_border_image_slice_value(part)?);
    }
    Some(BorderImageSlice {
        offsets: expand_border_image_slice_offsets(&offsets)?,
        fill,
    })
}

pub(in crate::css) fn parse_border_image_slice_value(value: &str) -> Option<BorderImageSliceValue> {
    parse_percentage(value)
        .map(BorderImageSliceValue::Percent)
        .or_else(|| parse_non_negative_number(value).map(BorderImageSliceValue::Number))
}

pub(in crate::css) fn expand_border_image_slice_offsets(
    values: &[BorderImageSliceValue],
) -> Option<BorderImageSliceOffsets> {
    match values {
        [all] => Some(BorderImageSliceOffsets {
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [vertical, horizontal] => Some(BorderImageSliceOffsets {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(BorderImageSliceOffsets {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(BorderImageSliceOffsets {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

/// Parse `border-image-width`.
///
/// Numeric values multiply border widths; explicit lengths and percentages are
/// represented as computed length-percentage values:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-width>.
pub(crate) fn parse_border_image_width(value: &str, font_size: f32) -> Option<BorderImageWidth> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_border_image_width_value(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some(BorderImageWidth {
            top: all.clone(),
            right: all.clone(),
            bottom: all.clone(),
            left: all.clone(),
        }),
        [vertical, horizontal] => Some(BorderImageWidth {
            top: vertical.clone(),
            right: horizontal.clone(),
            bottom: vertical.clone(),
            left: horizontal.clone(),
        }),
        [top, horizontal, bottom] => Some(BorderImageWidth {
            top: top.clone(),
            right: horizontal.clone(),
            bottom: bottom.clone(),
            left: horizontal.clone(),
        }),
        [top, right, bottom, left] => Some(BorderImageWidth {
            top: top.clone(),
            right: right.clone(),
            bottom: bottom.clone(),
            left: left.clone(),
        }),
        _ => None,
    }
}

pub(in crate::css) fn parse_border_image_width_value(
    value: &str,
    font_size: f32,
) -> Option<BorderImageWidthValue> {
    if trim_css_value(value).eq_ignore_ascii_case("auto") {
        return Some(BorderImageWidthValue::Auto);
    }
    parse_non_negative_number(value)
        .map(BorderImageWidthValue::Number)
        .or_else(|| {
            parse_computed_length_percentage(value, font_size)
                .map(BorderImageWidthValue::LengthPercentage)
        })
}

/// Parse `border-image-outset`.
///
/// Numeric values multiply border widths; lengths are used directly:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-outset>.
pub(crate) fn parse_border_image_outset(value: &str, font_size: f32) -> Option<BorderImageOutset> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(|part| parse_border_image_outset_value(part, font_size))
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some(BorderImageOutset {
            top: all.clone(),
            right: all.clone(),
            bottom: all.clone(),
            left: all.clone(),
        }),
        [vertical, horizontal] => Some(BorderImageOutset {
            top: vertical.clone(),
            right: horizontal.clone(),
            bottom: vertical.clone(),
            left: horizontal.clone(),
        }),
        [top, horizontal, bottom] => Some(BorderImageOutset {
            top: top.clone(),
            right: horizontal.clone(),
            bottom: bottom.clone(),
            left: horizontal.clone(),
        }),
        [top, right, bottom, left] => Some(BorderImageOutset {
            top: top.clone(),
            right: right.clone(),
            bottom: bottom.clone(),
            left: left.clone(),
        }),
        _ => None,
    }
}

pub(in crate::css) fn parse_border_image_outset_value(
    value: &str,
    font_size: f32,
) -> Option<BorderImageOutsetValue> {
    parse_non_negative_number(value)
        .map(BorderImageOutsetValue::Number)
        .or_else(|| {
            parse_computed_length_percentage(value, font_size).and_then(|value| {
                (!value.needs_percentage_basis() && !value.is_definitely_negative())
                    .then_some(BorderImageOutsetValue::Length(value))
            })
        })
}

/// Parse `border-image-repeat`.
///
/// One keyword applies to both axes; two keywords are horizontal then vertical:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-repeat>.
pub(crate) fn parse_border_image_repeat(value: &str) -> Option<BorderImageRepeat> {
    let values = split_css_component_values(value)
        .into_iter()
        .map(parse_border_image_repeat_keyword)
        .collect::<Option<Vec<_>>>()?;
    match values.as_slice() {
        [all] => Some(BorderImageRepeat {
            horizontal: *all,
            vertical: *all,
        }),
        [horizontal, vertical] => Some(BorderImageRepeat {
            horizontal: *horizontal,
            vertical: *vertical,
        }),
        _ => None,
    }
}

pub(in crate::css) fn parse_border_image_repeat_keyword(
    value: &str,
) -> Option<BorderImageRepeatKeyword> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "stretch" => Some(BorderImageRepeatKeyword::Stretch),
        "repeat" => Some(BorderImageRepeatKeyword::Repeat),
        "round" => Some(BorderImageRepeatKeyword::Round),
        "space" => Some(BorderImageRepeatKeyword::Space),
        _ => None,
    }
}

pub(in crate::css) fn parse_non_negative_number(value: &str) -> Option<f32> {
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    let value = parser.expect_number().ok()?;
    (value >= 0.0 && parser.is_exhausted()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_border_shorthand_tracks_the_reset_only_source() {
        assert!(matches!(
            parse_mask_border_source("alpha", 12.0),
            Some(ComputedImage::None)
        ));
        assert!(matches!(
            parse_mask_border_source("none luminance", 12.0),
            Some(ComputedImage::None)
        ));
        assert!(parse_mask_border_source("alpha luminance", 12.0).is_none());
        assert!(parse_mask_border_source("", 12.0).is_none());
    }
}
