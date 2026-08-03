use super::*;

pub(crate) fn parse_length(value: &str) -> Option<f32> {
    parse_length_with_font_size(value, ROOT_FONT_SIZE_PT)
}

pub(crate) fn parse_length_with_font_size(value: &str, font_size: f32) -> Option<f32> {
    if let Some(length) = parse_specified_length(value) {
        if matches!(length, SpecifiedLength::FontRelativeCh(_)) {
            return None;
        }
        return Some(layout_points(
            length.to_computed(font_size, ROOT_FONT_SIZE_PT).length,
        ));
    }
    parse_math_length_percentage(value, font_size)?.length_if_no_percent()
}

pub(crate) fn parse_specified_length(value: &str) -> Option<SpecifiedLength> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") || value.ends_with('%') {
        return None;
    }

    let lower = value.to_ascii_lowercase();
    let (number, length) = if let Some(number) = lower.strip_suffix("px") {
        (
            number,
            SpecifiedLength::Absolute {
                value: 0.0,
                unit: AbsoluteLengthUnit::Px,
            },
        )
    } else if let Some(number) = lower.strip_suffix("pt") {
        (
            number,
            SpecifiedLength::Absolute {
                value: 0.0,
                unit: AbsoluteLengthUnit::Pt,
            },
        )
    } else if let Some(number) = lower.strip_suffix("in") {
        (
            number,
            SpecifiedLength::Absolute {
                value: 0.0,
                unit: AbsoluteLengthUnit::In,
            },
        )
    } else if let Some(number) = lower.strip_suffix("cm") {
        (
            number,
            SpecifiedLength::Absolute {
                value: 0.0,
                unit: AbsoluteLengthUnit::Cm,
            },
        )
    } else if let Some(number) = lower.strip_suffix("mm") {
        (
            number,
            SpecifiedLength::Absolute {
                value: 0.0,
                unit: AbsoluteLengthUnit::Mm,
            },
        )
    } else if let Some(number) = lower.strip_suffix("pc") {
        (
            number,
            SpecifiedLength::Absolute {
                value: 0.0,
                unit: AbsoluteLengthUnit::Pc,
            },
        )
    } else if let Some(number) = lower.strip_suffix("rlh") {
        (number, SpecifiedLength::RootFontRelativeRlh(0.0))
    } else if let Some(number) = lower.strip_suffix("rex") {
        (number, SpecifiedLength::RootFontRelativeRex(0.0))
    } else if let Some(number) = lower.strip_suffix("rcap") {
        (number, SpecifiedLength::RootFontRelativeRcap(0.0))
    } else if let Some(number) = lower.strip_suffix("rch") {
        (number, SpecifiedLength::RootFontRelativeRch(0.0))
    } else if let Some(number) = lower.strip_suffix("ric") {
        (number, SpecifiedLength::RootFontRelativeRic(0.0))
    } else if let Some(number) = lower.strip_suffix("rem") {
        (number, SpecifiedLength::RootFontRelativeRem(0.0))
    } else if let Some(number) = lower.strip_suffix("em") {
        (number, SpecifiedLength::FontRelativeEm(0.0))
    } else if let Some(number) = lower.strip_suffix("ch") {
        (number, SpecifiedLength::FontRelativeCh(0.0))
    } else if let Some(number) = lower.strip_suffix("ex") {
        (number, SpecifiedLength::FontRelativeEx(0.0))
    } else if let Some(number) = lower.strip_suffix("cap") {
        (number, SpecifiedLength::FontRelativeCap(0.0))
    } else if let Some(number) = lower.strip_suffix("ic") {
        (number, SpecifiedLength::FontRelativeIc(0.0))
    } else if let Some(number) = lower.strip_suffix("lh") {
        (number, SpecifiedLength::FontRelativeLh(0.0))
    } else {
        (
            lower.as_str(),
            SpecifiedLength::Absolute {
                value: 0.0,
                unit: AbsoluteLengthUnit::NumberPt,
            },
        )
    };
    let value = number.trim().parse::<f32>().ok()?;
    Some(match length {
        SpecifiedLength::Absolute { unit, .. } => SpecifiedLength::Absolute { value, unit },
        SpecifiedLength::FontRelativeEm(_) => SpecifiedLength::FontRelativeEm(value),
        SpecifiedLength::FontRelativeCh(_) => SpecifiedLength::FontRelativeCh(value),
        SpecifiedLength::FontRelativeEx(_) => SpecifiedLength::FontRelativeEx(value),
        SpecifiedLength::FontRelativeCap(_) => SpecifiedLength::FontRelativeCap(value),
        SpecifiedLength::FontRelativeIc(_) => SpecifiedLength::FontRelativeIc(value),
        SpecifiedLength::FontRelativeLh(_) => SpecifiedLength::FontRelativeLh(value),
        SpecifiedLength::RootFontRelativeRem(_) => SpecifiedLength::RootFontRelativeRem(value),
        SpecifiedLength::RootFontRelativeRex(_) => SpecifiedLength::RootFontRelativeRex(value),
        SpecifiedLength::RootFontRelativeRcap(_) => SpecifiedLength::RootFontRelativeRcap(value),
        SpecifiedLength::RootFontRelativeRch(_) => SpecifiedLength::RootFontRelativeRch(value),
        SpecifiedLength::RootFontRelativeRic(_) => SpecifiedLength::RootFontRelativeRic(value),
        SpecifiedLength::RootFontRelativeRlh(_) => SpecifiedLength::RootFontRelativeRlh(value),
    })
}

pub(crate) fn parse_computed_length_percentage(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    parse_computed_length_percentage_with_root(value, font_size, ROOT_FONT_SIZE_PT)
}

/// Parses a `<length-percentage>` while retaining font-relative components
/// for a property whose computed value is finalized only after its element's
/// winning font has been selected.
///
/// Generated images are parsed independently of their owning property, but
/// CSS Images defines their embedded lengths relative to the element on which
/// the image is used.  Resolving `em` at parser time would therefore bind a
/// gradient stop to a parser fallback rather than the owning style:
/// <https://www.w3.org/TR/css-images-3/#gradients> and
/// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
pub(in crate::css) fn parse_deferred_length_percentage(
    value: &str,
) -> Option<ComputedLengthPercentage> {
    parse_math_length_percentage(value, ROOT_FONT_SIZE_PT)
}

/// Parses a computed `<length-percentage>` using the document root's used font
/// size as the `rem` basis.
/// <https://www.w3.org/TR/css-values-4/#rem>
pub(crate) fn parse_computed_length_percentage_with_root(
    value: &str,
    font_size: f32,
    root_font_size: f32,
) -> Option<ComputedLengthPercentage> {
    let mut value = parse_math_length_percentage_with_root(value, font_size, root_font_size)?;
    value.resolve_em_relative_lengths(layout_pt(font_size));
    value.resolve_root_font_relative_lengths(root_font_size);
    Some(value)
}

pub(crate) fn parse_percentage(value: &str) -> Option<f32> {
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    parser.expect_percentage().ok()
}
