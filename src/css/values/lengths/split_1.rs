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
    } else if let Some(number) = lower.strip_suffix("rem") {
        (number, SpecifiedLength::RootFontRelativeRem(0.0))
    } else if let Some(number) = lower.strip_suffix("em") {
        (number, SpecifiedLength::FontRelativeEm(0.0))
    } else if let Some(number) = lower.strip_suffix("ch") {
        (number, SpecifiedLength::FontRelativeCh(0.0))
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
        SpecifiedLength::RootFontRelativeRem(_) => SpecifiedLength::RootFontRelativeRem(value),
    })
}

pub(crate) fn parse_computed_length_percentage(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    parse_math_length_percentage(value, font_size)
}

pub(crate) fn parse_computed_length_percentage_auto(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentageOrAuto> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(ComputedLengthPercentageOrAuto::Auto);
    }
    parse_computed_length_percentage(value, font_size)
        .map(ComputedLengthPercentageOrAuto::LengthPercentage)
}

/// Parses the computed CSS box-size grammar for width/height properties.
///
/// CSS Sizing defines the width/height/min/max-size value grammar as accepting
/// `auto`, intrinsic sizing keywords, `fit-content()`, `stretch`, and
/// `<length-percentage>` values. Margins and positioned insets intentionally
/// keep using `parse_computed_length_percentage_auto` because they do not
/// accept intrinsic size keywords:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
pub(crate) fn parse_computed_box_size(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentageOrAuto> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(ComputedLengthPercentageOrAuto::Auto);
    }
    if value.eq_ignore_ascii_case("min-content") {
        return Some(ComputedLengthPercentageOrAuto::MinContent);
    }
    if value.eq_ignore_ascii_case("max-content") {
        return Some(ComputedLengthPercentageOrAuto::MaxContent);
    }
    if value.eq_ignore_ascii_case("fit-content") {
        return Some(ComputedLengthPercentageOrAuto::FitContent(None));
    }
    if value.eq_ignore_ascii_case("stretch") {
        return Some(ComputedLengthPercentageOrAuto::Stretch);
    }
    if let Some(argument) = fit_content_argument(value)
        && let Some(length) = parse_computed_length_percentage(argument, font_size)
        && !length_percentage_is_definitely_negative(length)
    {
        return Some(ComputedLengthPercentageOrAuto::FitContent(Some(length)));
    }
    parse_computed_length_percentage(value, font_size)
        .map(ComputedLengthPercentageOrAuto::LengthPercentage)
}

/// Parses the `flex-basis` computed value grammar.
///
/// CSS Flexbox defines `flex-basis` as accepting `content`, `auto`, and
/// `<length-percentage>` values. Keep this parser separate from generic box
/// size parsing so `content` cannot be accepted by width, margin, or inset
/// longhands:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>.
pub(crate) fn parse_computed_flex_basis(value: &str, font_size: f32) -> Option<ComputedFlexBasis> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("content") {
        return Some(ComputedFlexBasis::Content);
    }
    if value.eq_ignore_ascii_case("min-content") {
        return Some(ComputedFlexBasis::MinContent);
    }
    if value.eq_ignore_ascii_case("max-content") {
        return Some(ComputedFlexBasis::MaxContent);
    }
    if value.eq_ignore_ascii_case("fit-content") {
        return Some(ComputedFlexBasis::FitContent(None));
    }
    if let Some(argument) = fit_content_argument(value)
        && let Some(length) = parse_computed_length_percentage(argument, font_size)
        && !length_percentage_is_definitely_negative(length)
    {
        return Some(ComputedFlexBasis::FitContent(Some(length)));
    }
    if value.eq_ignore_ascii_case("auto") {
        return Some(ComputedFlexBasis::Auto);
    }
    let has_percentage = value.contains('%');
    let length = parse_computed_length_percentage(value, font_size)?;
    (!length_percentage_is_definitely_negative(length)).then_some(
        ComputedFlexBasis::LengthPercentage(ComputedFlexBasisLength::new(length, has_percentage)),
    )
}

/// Returns whether a computed length-percentage cannot resolve to a non-negative value.
///
/// CSS Flexbox's `flex-basis` accepts the CSS Sizing `<width>` grammar, whose
/// length-percentage values have a non-negative range. Mixed values such as
/// `calc(50% - 10pt)` need a used percentage basis, but values with no positive
/// component are definitely negative and must be rejected:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property> and
/// <https://www.w3.org/TR/css-values-4/#calc-range>.
pub(in crate::css) fn length_percentage_is_definitely_negative(
    value: ComputedLengthPercentage,
) -> bool {
    let components = [
        value.length_points(),
        value.percent,
        value.ch,
        value.vw,
        value.vh,
        value.vmin,
        value.vmax,
        value.vi,
        value.vb,
    ];
    components.iter().any(|component| *component < 0.0)
        && components.iter().all(|component| *component <= 0.0)
}

/// Extracts the argument from a CSS `fit-content()` sizing function.
///
/// CSS Sizing defines `fit-content(<length-percentage>)` as an intrinsic size
/// clamp. This helper only recognizes the function wrapper; the argument is
/// parsed by the shared typed `<length-percentage>` parser:
/// <https://www.w3.org/TR/css-sizing-3/#funcdef-width-fit-content>.
pub(in crate::css) fn fit_content_argument(value: &str) -> Option<&str> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    let argument = lower
        .strip_prefix("fit-content(")
        .and_then(|rest| rest.strip_suffix(')'))?;
    let start = "fit-content(".len();
    let end = value.len().checked_sub(1)?;
    (argument.find('(').is_none() && argument.find(')').is_none()).then_some(&value[start..end])
}

pub(crate) fn parse_percentage(value: &str) -> Option<f32> {
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    parser.expect_percentage().ok()
}

/// Parses CSS math functions into a computed `<length-percentage>`.
///
/// CSS Values and Units Level 4 defines `calc()`, `min()`, `max()`, and
/// `clamp()` for math expressions. `calc()` preserves mixed
/// length-percentage values until used-value resolution; `min()`/`max()` and
/// `clamp()` are computed here only when their arguments are comparable without
/// a layout-time percentage basis:
/// <https://www.w3.org/TR/css-values-4/#math>.
pub(in crate::css) fn parse_math_length_percentage(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    match parse_math_value(value, font_size)? {
        MathValue::LengthPercentage(value) => Some(value),
        MathValue::Number(value) => Some(ComputedLengthPercentage::from_points(value)),
    }
}

pub(in crate::css) fn parse_math_value(value: &str, font_size: f32) -> Option<MathValue> {
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    let value = parse_math_sum(&mut parser, font_size)?;
    parser.is_exhausted().then_some(value)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::css) enum MathValue {
    Number(f32),
    LengthPercentage(ComputedLengthPercentage),
}

impl MathValue {
    pub(in crate::css) fn add(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => Some(Self::Number(left + right)),
            (Self::LengthPercentage(left), Self::LengthPercentage(right)) => {
                if let (Some(left), Some(right)) = (left.components(), right.components()) {
                    return Some(Self::LengthPercentage(ComputedLengthPercentage {
                        length: left.length + right.length,
                        percent: left.percent + right.percent,
                        has_percentage: left.has_percentage || right.has_percentage,
                        ch: left.ch + right.ch,
                        vw: left.vw + right.vw,
                        vh: left.vh + right.vh,
                        vmin: left.vmin + right.vmin,
                        vmax: left.vmax + right.vmax,
                        vi: left.vi + right.vi,
                        vb: left.vb + right.vb,
                        math: None,
                    }));
                }
                Some(Self::LengthPercentage(
                    ComputedLengthPercentage::from_deferred_math(
                        DeferredLengthPercentageMath::Sum(left.expression(), right.expression()),
                    ),
                ))
            }
            _ => None,
        }
    }

    pub(in crate::css) fn sub(self, other: Self) -> Option<Self> {
        self.add(other.mul_number(-1.0)?)
    }

    pub(in crate::css) fn mul(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => Some(Self::Number(left * right)),
            (Self::Number(number), value) | (value, Self::Number(number)) => {
                value.mul_number(number)
            }
            _ => None,
        }
    }

    pub(in crate::css) fn div(self, other: Self) -> Option<Self> {
        match other {
            Self::Number(number) if number != 0.0 => self.mul_number(1.0 / number),
            _ => None,
        }
    }

    pub(in crate::css) fn mul_number(self, number: f32) -> Option<Self> {
        Some(match self {
            Self::Number(value) => Self::Number(value * number),
            Self::LengthPercentage(value) => {
                if let Some(value) = value.components() {
                    return Some(Self::LengthPercentage(ComputedLengthPercentage {
                        length: value.length * number,
                        percent: value.percent * number,
                        has_percentage: value.has_percentage,
                        ch: value.ch * number,
                        vw: value.vw * number,
                        vh: value.vh * number,
                        vmin: value.vmin * number,
                        vmax: value.vmax * number,
                        vi: value.vi * number,
                        vb: value.vb * number,
                        math: None,
                    }));
                }
                Self::LengthPercentage(ComputedLengthPercentage::from_deferred_math(
                    DeferredLengthPercentageMath::Product(value.expression(), number),
                ))
            }
        })
    }

    pub(in crate::css) fn ordering_against(self, other: Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.partial_cmp(&right),
            (Self::LengthPercentage(left), Self::LengthPercentage(right)) => {
                length_percentage_component_ordering(left.components()?, right.components()?)
            }
            _ => None,
        }
    }
}

pub(in crate::css) fn parse_math_sum(
    input: &mut Parser<'_, '_>,
    font_size: f32,
) -> Option<MathValue> {
    let mut value = parse_math_product(input, font_size)?;
    loop {
        if input.try_parse(|input| input.expect_delim('+')).is_ok() {
            value = value.add(parse_math_product(input, font_size)?)?;
        } else if input.try_parse(|input| input.expect_delim('-')).is_ok() {
            value = value.sub(parse_math_product(input, font_size)?)?;
        } else {
            return Some(value);
        }
    }
}

pub(in crate::css) fn parse_math_product(
    input: &mut Parser<'_, '_>,
    font_size: f32,
) -> Option<MathValue> {
    let mut value = parse_math_primary(input, font_size)?;
    loop {
        if input.try_parse(|input| input.expect_delim('*')).is_ok() {
            value = value.mul(parse_math_primary(input, font_size)?)?;
        } else if input.try_parse(|input| input.expect_delim('/')).is_ok() {
            value = value.div(parse_math_primary(input, font_size)?)?;
        } else {
            return Some(value);
        }
    }
}

pub(in crate::css) fn parse_math_primary(
    input: &mut Parser<'_, '_>,
    font_size: f32,
) -> Option<MathValue> {
    let token = input.next().ok()?.clone();
    match token {
        Token::Number { value, .. } => Some(MathValue::Number(value)),
        Token::Percentage { unit_value, .. } => Some(MathValue::LengthPercentage(
            ComputedLengthPercentage::from_percent(unit_value),
        )),
        Token::Dimension { value, unit, .. } => {
            parse_math_dimension(value, &unit, font_size).map(MathValue::LengthPercentage)
        }
        Token::Function(name) => {
            let name = name.to_string();
            input
                .parse_nested_block(
                    |input| -> Result<MathValue, cssparser::ParseError<'_, ()>> {
                        parse_math_function(&name, input, font_size)
                            .ok_or_else(|| input.new_custom_error(()))
                    },
                )
                .ok()
        }
        Token::ParenthesisBlock => input
            .parse_nested_block(
                |input| -> Result<MathValue, cssparser::ParseError<'_, ()>> {
                    let value = parse_math_sum(input, font_size)
                        .ok_or_else(|| input.new_custom_error(()))?;
                    input
                        .is_exhausted()
                        .then_some(value)
                        .ok_or_else(|| input.new_custom_error(()))
                },
            )
            .ok(),
        _ => None,
    }
}

pub(in crate::css) fn parse_math_dimension(
    value: f32,
    unit: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    let unit = if unit.eq_ignore_ascii_case("rem") {
        return Some(ComputedLengthPercentage::from_points(
            value * ROOT_FONT_SIZE_PT,
        ));
    } else if unit.eq_ignore_ascii_case("em") {
        return Some(ComputedLengthPercentage::from_points(value * font_size));
    } else if unit.eq_ignore_ascii_case("ch") {
        return Some(ComputedLengthPercentage::from_ch(value));
    } else if unit.eq_ignore_ascii_case("vw")
        || unit.eq_ignore_ascii_case("svw")
        || unit.eq_ignore_ascii_case("lvw")
        || unit.eq_ignore_ascii_case("dvw")
    {
        return Some(ComputedLengthPercentage::from_vw(value));
    } else if unit.eq_ignore_ascii_case("vh")
        || unit.eq_ignore_ascii_case("svh")
        || unit.eq_ignore_ascii_case("lvh")
        || unit.eq_ignore_ascii_case("dvh")
    {
        return Some(ComputedLengthPercentage::from_vh(value));
    } else if unit.eq_ignore_ascii_case("vmin")
        || unit.eq_ignore_ascii_case("svmin")
        || unit.eq_ignore_ascii_case("lvmin")
        || unit.eq_ignore_ascii_case("dvmin")
    {
        return Some(ComputedLengthPercentage::from_vmin(value));
    } else if unit.eq_ignore_ascii_case("vmax")
        || unit.eq_ignore_ascii_case("svmax")
        || unit.eq_ignore_ascii_case("lvmax")
        || unit.eq_ignore_ascii_case("dvmax")
    {
        return Some(ComputedLengthPercentage::from_vmax(value));
    } else if unit.eq_ignore_ascii_case("vi")
        || unit.eq_ignore_ascii_case("svi")
        || unit.eq_ignore_ascii_case("lvi")
        || unit.eq_ignore_ascii_case("dvi")
    {
        return Some(ComputedLengthPercentage::from_vi(value));
    } else if unit.eq_ignore_ascii_case("vb")
        || unit.eq_ignore_ascii_case("svb")
        || unit.eq_ignore_ascii_case("lvb")
        || unit.eq_ignore_ascii_case("dvb")
    {
        return Some(ComputedLengthPercentage::from_vb(value));
    } else if unit.eq_ignore_ascii_case("px") {
        AbsoluteLengthUnit::Px
    } else if unit.eq_ignore_ascii_case("pt") {
        AbsoluteLengthUnit::Pt
    } else if unit.eq_ignore_ascii_case("in") {
        AbsoluteLengthUnit::In
    } else if unit.eq_ignore_ascii_case("cm") {
        AbsoluteLengthUnit::Cm
    } else if unit.eq_ignore_ascii_case("mm") {
        AbsoluteLengthUnit::Mm
    } else if unit.eq_ignore_ascii_case("q") {
        AbsoluteLengthUnit::Q
    } else if unit.eq_ignore_ascii_case("pc") {
        AbsoluteLengthUnit::Pc
    } else {
        return None;
    };
    Some(ComputedLengthPercentage::from_layout_length(
        unit.length_for_value(value),
    ))
}

pub(in crate::css) fn parse_math_function(
    name: &str,
    input: &mut Parser<'_, '_>,
    font_size: f32,
) -> Option<MathValue> {
    if name.eq_ignore_ascii_case("calc") {
        let value = parse_math_sum(input, font_size)?;
        return input.is_exhausted().then_some(value);
    }
    if name.eq_ignore_ascii_case("min") || name.eq_ignore_ascii_case("max") {
        let values = parse_comma_separated_math_values(input, font_size)?;
        let choose_max = name.eq_ignore_ascii_case("max");
        return compare_math_values(&values, choose_max)
            .or_else(|| defer_min_max_math_values(&values, choose_max));
    }
    if name.eq_ignore_ascii_case("clamp") {
        let values = parse_comma_separated_math_values(input, font_size)?;
        let [min, center, max] = values.as_slice() else {
            return None;
        };
        let min = *min;
        let center = *center;
        let max = *max;
        if let Some(below_max) = compare_math_values(&[center, max], false)
            && let Some(value) = compare_math_values(&[below_max, min], true)
        {
            return Some(value);
        }
        return defer_clamp_math_values(min, center, max);
    }
    None
}

pub(in crate::css) fn parse_comma_separated_math_values(
    input: &mut Parser<'_, '_>,
    font_size: f32,
) -> Option<Vec<MathValue>> {
    let mut values = Vec::new();
    loop {
        values.push(parse_math_sum(input, font_size)?);
        if input.is_exhausted() {
            break;
        }
        input.expect_comma().ok()?;
    }
    (!values.is_empty()).then_some(values)
}

pub(in crate::css) fn compare_math_values(
    values: &[MathValue],
    choose_max: bool,
) -> Option<MathValue> {
    let mut best = *values.first()?;
    for candidate in &values[1..] {
        let ordering = candidate.ordering_against(best)?;
        if (choose_max && ordering.is_gt()) || (!choose_max && ordering.is_lt()) {
            best = *candidate;
        }
    }
    Some(best)
}

pub(in crate::css) fn defer_min_max_math_values(
    values: &[MathValue],
    choose_max: bool,
) -> Option<MathValue> {
    let expressions = values
        .iter()
        .map(|value| match value {
            MathValue::LengthPercentage(value) => Some(value.expression()),
            MathValue::Number(_) => None,
        })
        .collect::<Option<Vec<_>>>()?;
    can_defer_until_used_resolution(&expressions)?;
    let mut expressions = expressions.into_iter();
    let mut value = expressions.next()?;
    for next in expressions {
        value = ComputedLengthPercentage::from_deferred_math(if choose_max {
            DeferredLengthPercentageMath::Max(value, next)
        } else {
            DeferredLengthPercentageMath::Min(value, next)
        })
        .expression();
    }
    Some(MathValue::LengthPercentage(
        ComputedLengthPercentage::from_expression(value),
    ))
}

pub(in crate::css) fn defer_clamp_math_values(
    min: MathValue,
    center: MathValue,
    max: MathValue,
) -> Option<MathValue> {
    let MathValue::LengthPercentage(min) = min else {
        return None;
    };
    let MathValue::LengthPercentage(center) = center else {
        return None;
    };
    let MathValue::LengthPercentage(max) = max else {
        return None;
    };
    let min = min.expression();
    let center = center.expression();
    let max = max.expression();
    can_defer_until_used_resolution(&[min, center, max])?;
    Some(MathValue::LengthPercentage(
        ComputedLengthPercentage::from_deferred_math(DeferredLengthPercentageMath::Clamp {
            min,
            center,
            max,
        }),
    ))
}

pub(in crate::css) fn can_defer_until_used_resolution(
    values: &[LengthPercentageExpression],
) -> Option<()> {
    let mut has_deferred_basis = false;
    for value in values {
        has_deferred_basis |= value.depends_on_metric_or_percent()?;
    }
    has_deferred_basis.then_some(())
}

pub(crate) fn set_font_size(style: &mut ComputedStyle, font_size: f32) {
    style.font_size = font_size;
    project_line_height(style);
}

pub(crate) fn fallback_ch_advance_for_style(style: &ComputedStyle) -> f32 {
    fallback_ch_advance_for_font_metrics(
        style.font_size,
        style.writing_mode,
        style.text_orientation,
    )
}

pub(crate) fn fallback_ch_advance_for_font_metrics(
    font_size: f32,
    writing_mode: WritingMode,
    text_orientation: TextOrientation,
) -> f32 {
    if writing_mode != WritingMode::HorizontalTb && text_orientation == TextOrientation::Upright {
        font_size
    } else {
        font_size * 0.5
    }
}

pub(crate) fn parse_font_size(value: &str, parent_font_size: f32) -> Option<f32> {
    parse_font_size_with_parent_ch_advance(value, parent_font_size, parent_font_size * 0.5)
}

pub(crate) fn parse_font_size_with_parent_ch_advance(
    value: &str,
    parent_font_size: f32,
    parent_ch_advance: f32,
) -> Option<f32> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "xx-small" => return Some(7.0),
        "x-small" => return Some(8.3),
        "small" => return Some(10.0),
        "medium" => return Some(12.0),
        "large" => return Some(14.4),
        "x-large" => return Some(17.3),
        "xx-large" => return Some(20.7),
        "xxx-large" => return Some(24.9),
        "smaller" => return Some(parent_font_size / 1.2),
        "larger" => return Some(parent_font_size * 1.2),
        _ => {}
    }

    if let Some(mut value) = parse_computed_length_percentage(value, parent_font_size) {
        value.resolve_font_metric_lengths(parent_ch_advance);
        return value.used_length_with_percentage_basis(parent_font_size);
    }
    parse_length(value)
}

/// Parses `line-height` into its computed CSS value.
///
/// CSS 2.2 computes `normal` and unitless numbers as keywords/numbers, while
/// lengths and percentages compute to absolute lengths:
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-line-height>.
pub(crate) fn parse_computed_line_height(
    value: &str,
    font_size: f32,
) -> Option<ComputedLineHeight> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(ComputedLineHeight::Normal);
    }
    if let Ok(multiplier) = value.parse::<f32>() {
        return Some(ComputedLineHeight::Number(multiplier));
    }
    if let Some(mut value) = parse_computed_length_percentage(value, font_size) {
        value.length += layout_pt(value.percent * font_size);
        value.percent = 0.0;
        return Some(ComputedLineHeight::Length(value));
    }
    parse_length(value).map(ComputedLineHeight::from_points)
}

/// Parses `letter-spacing` into a computed length projection.
///
/// CSS Text defines `letter-spacing` as `normal | <length>` and makes the
/// property inherited. The renderer stores `normal` as zero additional spacing
/// until justification-driven spacing is modeled separately:
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
pub(crate) fn parse_letter_spacing(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(ComputedLengthPercentage::ZERO);
    }
    if let Some(value) = parse_computed_length_percentage(value, font_size)
        && value.percent == 0.0
    {
        return Some(value);
    }
    parse_length(value).map(ComputedLengthPercentage::from_points)
}

/// Parses `word-spacing` into a computed length projection.
///
/// CSS Text defines `word-spacing` as `normal | <length>` and makes the
/// property inherited. The renderer stores `normal` as zero additional spacing
/// and keeps font-relative components typed until used-value resolution:
/// <https://www.w3.org/TR/css-text-3/#word-spacing-property>.
pub(crate) fn parse_word_spacing(value: &str, font_size: f32) -> Option<ComputedLengthPercentage> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(ComputedLengthPercentage::ZERO);
    }
    if let Some(value) = parse_computed_length_percentage(value, font_size)
        && value.percent == 0.0
    {
        return Some(value);
    }
    parse_length(value).map(ComputedLengthPercentage::from_points)
}

/// Parses CSS Text's `tab-size` property.
///
/// CSS Text Level 3 defines `tab-size` as a non-negative number of spaces or
/// a non-negative length, with an initial value of 8:
/// <https://www.w3.org/TR/css-text-3/#tab-size-property>.
pub(crate) fn parse_tab_size(value: &str, font_size: f32) -> Option<TabSize> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(TabSize::INITIAL);
    }
    if let Ok(number) = value.parse::<f32>()
        && number.is_finite()
        && number >= 0.0
    {
        return Some(TabSize::Spaces(number));
    }
    if let Some(length) = parse_computed_length_percentage(value, font_size)
        && length.percent == 0.0
        && length.length_points() >= 0.0
    {
        return Some(TabSize::Length(length));
    }
    None
}

/// Parses the computed CSS `text-indent` value.
///
/// CSS Text defines the grammar as `<length-percentage> && hanging? &&
/// each-line?`; the length-percentage is computed now, while percentages
/// remain unresolved until layout has the containing block inline size:
/// <https://www.w3.org/TR/css-text-3/#text-indent-property>.
pub(crate) fn parse_text_indent(value: &str, font_size: f32) -> Option<ComputedTextIndent> {
    let value = trim_css_value(value);
    let (value, hanging) = remove_unordered_text_indent_keyword(value, "hanging");
    let (value, each_line) = remove_unordered_text_indent_keyword(&value, "each-line");
    let amount = parse_computed_length_percentage(&value, font_size)?;
    Some(ComputedTextIndent {
        amount,
        hanging,
        each_line,
    })
}

/// Parses CSS Inline Layout `dominant-baseline`.
///
/// `dominant-baseline` is the inherited baseline-table selection used when
/// `alignment-baseline: baseline` resolves against the parent:
/// <https://drafts.csswg.org/css-inline-3/#dominant-baseline-property>.
pub(crate) fn parse_dominant_baseline(value: &str) -> Option<DominantBaseline> {
    Some(match parse_baseline_metric(value)? {
        BaselineMetricParseResult::Auto => DominantBaseline::Auto,
        BaselineMetricParseResult::Metric(metric) => DominantBaseline::Metric(metric),
        BaselineMetricParseResult::Baseline => return None,
    })
}

/// Parses CSS Inline Layout `alignment-baseline`.
///
/// The `baseline` keyword resolves to the parent's dominant baseline during
/// layout:
/// <https://drafts.csswg.org/css-inline-3/#alignment-baseline-property>.
pub(crate) fn parse_alignment_baseline(value: &str) -> Option<AlignmentBaseline> {
    Some(match parse_baseline_metric(value)? {
        BaselineMetricParseResult::Baseline => AlignmentBaseline::Baseline,
        BaselineMetricParseResult::Metric(metric) => AlignmentBaseline::Metric(metric),
        BaselineMetricParseResult::Auto => return None,
    })
}

pub(crate) fn parse_baseline_source(value: &str) -> Option<BaselineSource> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => Some(BaselineSource::Auto),
        "first" => Some(BaselineSource::First),
        "last" => Some(BaselineSource::Last),
        _ => None,
    }
}

/// Parses CSS Inline Layout `baseline-shift`.
///
/// The computed value keeps mixed length-percentages typed until layout can
/// resolve percentages against the aligned element's own line-height:
/// <https://drafts.csswg.org/css-inline-3/#baseline-shift-property>.
pub(crate) fn parse_baseline_shift(value: &str, font_size: f32) -> Option<BaselineShift> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "baseline" => Some(BaselineShift::ZERO),
        "sub" => Some(BaselineShift::Sub),
        "super" => Some(BaselineShift::Super),
        "top" => Some(BaselineShift::Top),
        "center" => Some(BaselineShift::Center),
        "bottom" => Some(BaselineShift::Bottom),
        _ => {
            parse_computed_length_percentage(value, font_size).map(BaselineShift::LengthPercentage)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::css) enum BaselineMetricParseResult {
    Auto,
    Baseline,
    Metric(BaselineMetric),
}

pub(in crate::css) fn parse_baseline_metric(value: &str) -> Option<BaselineMetricParseResult> {
    Some(match trim_css_value(value).to_ascii_lowercase().as_str() {
        "auto" => BaselineMetricParseResult::Auto,
        "baseline" => BaselineMetricParseResult::Baseline,
        "text-bottom" => BaselineMetricParseResult::Metric(BaselineMetric::TextBottom),
        "alphabetic" => BaselineMetricParseResult::Metric(BaselineMetric::Alphabetic),
        "ideographic" => BaselineMetricParseResult::Metric(BaselineMetric::Ideographic),
        "middle" => BaselineMetricParseResult::Metric(BaselineMetric::Middle),
        "central" => BaselineMetricParseResult::Metric(BaselineMetric::Central),
        "mathematical" => BaselineMetricParseResult::Metric(BaselineMetric::Mathematical),
        "hanging" => BaselineMetricParseResult::Metric(BaselineMetric::Hanging),
        "text-top" => BaselineMetricParseResult::Metric(BaselineMetric::TextTop),
        _ => return None,
    })
}

/// Parses CSS 2.2 `vertical-align` compatibility values as a CSS Inline
/// shorthand over `alignment-baseline`, `baseline-source`, and
/// `baseline-shift`.
///
/// Length and percentage values are computed as typed length-percentages;
/// layout resolves percentages against the element's own line-height:
/// <https://www.w3.org/TR/CSS22/visudet.html#propdef-vertical-align>.
pub(crate) fn parse_vertical_align(value: &str, font_size: f32) -> Option<VerticalAlign> {
    let trimmed = trim_css_value(value);
    let (baseline_source, remaining) = remove_vertical_align_baseline_source(trimmed)?;
    let vertical_align = VerticalAlign::BASELINE.with_baseline_source(baseline_source);
    if remaining.is_empty() {
        return Some(vertical_align);
    }
    let lower = remaining.to_ascii_lowercase();
    match lower.as_str() {
        "baseline" => Some(vertical_align),
        "sub" => Some(vertical_align.with_baseline_shift(BaselineShift::Sub)),
        "super" => Some(vertical_align.with_baseline_shift(BaselineShift::Super)),
        "top" => Some(
            vertical_align
                .with_baseline_shift(BaselineShift::Top)
                .with_table_cell_align(TableCellVerticalAlign::Top),
        ),
        "center" => Some(vertical_align.with_baseline_shift(BaselineShift::Center)),
        "middle" => Some(
            vertical_align
                .with_alignment_baseline(AlignmentBaseline::Metric(BaselineMetric::Middle))
                .with_table_cell_align(TableCellVerticalAlign::Middle),
        ),
        "bottom" => Some(
            vertical_align
                .with_baseline_shift(BaselineShift::Bottom)
                .with_table_cell_align(TableCellVerticalAlign::Bottom),
        ),
        "text-top" => Some(
            vertical_align
                .with_alignment_baseline(AlignmentBaseline::Metric(BaselineMetric::TextTop)),
        ),
        "text-bottom" => Some(
            vertical_align
                .with_alignment_baseline(AlignmentBaseline::Metric(BaselineMetric::TextBottom)),
        ),
        _ => {
            if let Some(alignment_baseline) = parse_alignment_baseline(remaining) {
                return Some(vertical_align.with_alignment_baseline(alignment_baseline));
            }
            parse_baseline_shift(remaining, font_size)
                .map(|baseline_shift| vertical_align.with_baseline_shift(baseline_shift))
        }
    }
}
