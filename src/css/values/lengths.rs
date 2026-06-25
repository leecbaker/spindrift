use super::*;
use cssparser::Token;

pub(crate) fn parse_length(value: &str) -> Option<f32> {
    parse_length_with_font_size(value, ROOT_FONT_SIZE_PT)
}

pub(crate) fn parse_length_with_font_size(value: &str, font_size: f32) -> Option<f32> {
    if let Some(length) = parse_specified_length(value) {
        if matches!(length, SpecifiedLength::FontRelativeCh(_)) {
            return None;
        }
        return Some(length.to_computed(font_size, ROOT_FONT_SIZE_PT).points);
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
/// `auto`, intrinsic sizing keywords, `fit-content()`, and
/// `<length-percentage>` values. Margins and positioned insets intentionally
/// keep using `parse_computed_length_percentage_auto` because they do not
/// accept intrinsic size keywords:
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>.
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
    let value = parse_computed_length_percentage(value, font_size)?;
    (!length_percentage_is_definitely_negative(value))
        .then_some(ComputedFlexBasis::LengthPercentage(value))
}

/// Returns whether a computed length-percentage cannot resolve to a non-negative value.
///
/// CSS Flexbox's `flex-basis` accepts the CSS Sizing `<width>` grammar, whose
/// length-percentage values have a non-negative range. Mixed values such as
/// `calc(50% - 10pt)` need a used percentage basis, but values with no positive
/// component are definitely negative and must be rejected:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property> and
/// <https://www.w3.org/TR/css-values-4/#calc-range>.
fn length_percentage_is_definitely_negative(value: ComputedLengthPercentage) -> bool {
    let components = [
        value.length,
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
fn fit_content_argument(value: &str) -> Option<&str> {
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
fn parse_math_length_percentage(value: &str, font_size: f32) -> Option<ComputedLengthPercentage> {
    match parse_math_value(value, font_size)? {
        MathValue::LengthPercentage(value) => Some(value),
        MathValue::Number(value) => Some(ComputedLengthPercentage::from_length(value)),
    }
}

fn parse_math_value(value: &str, font_size: f32) -> Option<MathValue> {
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    let value = parse_math_sum(&mut parser, font_size)?;
    parser.is_exhausted().then_some(value)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MathValue {
    Number(f32),
    LengthPercentage(ComputedLengthPercentage),
}

impl MathValue {
    fn add(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => Some(Self::Number(left + right)),
            (Self::LengthPercentage(left), Self::LengthPercentage(right)) => {
                Some(Self::LengthPercentage(ComputedLengthPercentage {
                    length: left.length + right.length,
                    percent: left.percent + right.percent,
                    ch: left.ch + right.ch,
                    vw: left.vw + right.vw,
                    vh: left.vh + right.vh,
                    vmin: left.vmin + right.vmin,
                    vmax: left.vmax + right.vmax,
                    vi: left.vi + right.vi,
                    vb: left.vb + right.vb,
                }))
            }
            _ => None,
        }
    }

    fn sub(self, other: Self) -> Option<Self> {
        self.add(other.mul_number(-1.0)?)
    }

    fn mul(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => Some(Self::Number(left * right)),
            (Self::Number(number), value) | (value, Self::Number(number)) => {
                value.mul_number(number)
            }
            _ => None,
        }
    }

    fn div(self, other: Self) -> Option<Self> {
        match other {
            Self::Number(number) if number != 0.0 => self.mul_number(1.0 / number),
            _ => None,
        }
    }

    fn mul_number(self, number: f32) -> Option<Self> {
        Some(match self {
            Self::Number(value) => Self::Number(value * number),
            Self::LengthPercentage(value) => Self::LengthPercentage(ComputedLengthPercentage {
                length: value.length * number,
                percent: value.percent * number,
                ch: value.ch * number,
                vw: value.vw * number,
                vh: value.vh * number,
                vmin: value.vmin * number,
                vmax: value.vmax * number,
                vi: value.vi * number,
                vb: value.vb * number,
            }),
        })
    }

    fn comparable_component(self) -> Option<(ComparableUnit, f32)> {
        match self {
            Self::Number(value) => Some((ComparableUnit::Number, value)),
            Self::LengthPercentage(value)
                if value.percent == 0.0
                    && value.vw == 0.0
                    && value.vh == 0.0
                    && value.vmin == 0.0
                    && value.vmax == 0.0
                    && value.vi == 0.0
                    && value.vb == 0.0 =>
            {
                if value.ch == 0.0 {
                    Some((ComparableUnit::Length, value.length))
                } else {
                    None
                }
            }
            Self::LengthPercentage(value)
                if value.length == 0.0
                    && value.ch == 0.0
                    && value.vw == 0.0
                    && value.vh == 0.0
                    && value.vmin == 0.0
                    && value.vmax == 0.0
                    && value.vi == 0.0
                    && value.vb == 0.0 =>
            {
                Some((ComparableUnit::Percent, value.percent))
            }
            Self::LengthPercentage(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparableUnit {
    Number,
    Length,
    Percent,
}

fn parse_math_sum(input: &mut Parser<'_, '_>, font_size: f32) -> Option<MathValue> {
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

fn parse_math_product(input: &mut Parser<'_, '_>, font_size: f32) -> Option<MathValue> {
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

fn parse_math_primary(input: &mut Parser<'_, '_>, font_size: f32) -> Option<MathValue> {
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

fn parse_math_dimension(
    value: f32,
    unit: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    let unit = if unit.eq_ignore_ascii_case("rem") {
        return Some(ComputedLengthPercentage::from_length(
            value * ROOT_FONT_SIZE_PT,
        ));
    } else if unit.eq_ignore_ascii_case("em") {
        return Some(ComputedLengthPercentage::from_length(value * font_size));
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
    Some(ComputedLengthPercentage::from_length(
        value * unit.points_per_unit(),
    ))
}

fn parse_math_function(
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
        return compare_math_values(&values, name.eq_ignore_ascii_case("max"));
    }
    if name.eq_ignore_ascii_case("clamp") {
        let values = parse_comma_separated_math_values(input, font_size)?;
        let [min, center, max] = values.as_slice() else {
            return None;
        };
        let min = *min;
        let center = *center;
        let max = *max;
        return compare_math_values(&[compare_math_values(&[center, max], false)?, min], true);
    }
    None
}

fn parse_comma_separated_math_values(
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

fn compare_math_values(values: &[MathValue], choose_max: bool) -> Option<MathValue> {
    let mut best = *values.first()?;
    let (unit, mut best_value) = best.comparable_component()?;
    for candidate in &values[1..] {
        let (candidate_unit, candidate_value) = candidate.comparable_component()?;
        if candidate_unit != unit {
            return None;
        }
        if (choose_max && candidate_value > best_value)
            || (!choose_max && candidate_value < best_value)
        {
            best = *candidate;
            best_value = candidate_value;
        }
    }
    Some(best)
}

pub(crate) fn set_font_size(style: &mut ComputedStyle, font_size: f32) {
    style.font_size = font_size;
    project_line_height(style);
}

pub(crate) fn parse_font_size(value: &str, parent_font_size: f32) -> Option<f32> {
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

    if let Some(value) = parse_computed_length_percentage(value, parent_font_size)
        && value.ch == 0.0
    {
        return Some(value.length + value.percent * parent_font_size);
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
    if let Some(value) = parse_computed_length_percentage(value, font_size)
        && value.ch == 0.0
    {
        return Some(ComputedLineHeight::Length(
            value.length + value.percent * font_size,
        ));
    }
    parse_length(value).map(ComputedLineHeight::Length)
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
    parse_length(value).map(ComputedLengthPercentage::from_length)
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
    parse_length(value).map(ComputedLengthPercentage::from_length)
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
        && length.length >= 0.0
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

/// Parses the computed CSS `hanging-punctuation` keyword set.
///
/// CSS Text defines the grammar as
/// `none | [ first || [ force-end | allow-end ] || last ]`, with the computed
/// value preserving the specified keywords:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(crate) fn parse_hanging_punctuation(value: &str) -> Option<HangingPunctuation> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("none") {
        return Some(HangingPunctuation::NONE);
    }

    let mut output = HangingPunctuation::NONE;
    let mut saw_keyword = false;
    for keyword in value.split_whitespace() {
        match keyword.to_ascii_lowercase().as_str() {
            "first" if !output.first => output.first = true,
            "last" if !output.last => output.last = true,
            "force-end" if !output.force_end && !output.allow_end => output.force_end = true,
            "allow-end" if !output.force_end && !output.allow_end => output.allow_end = true,
            _ => return None,
        }
        saw_keyword = true;
    }
    saw_keyword.then_some(output)
}

fn remove_unordered_text_indent_keyword(value: &str, keyword: &str) -> (String, bool) {
    let Some(range) = find_top_level_keyword(value, keyword) else {
        return (value.trim().to_string(), false);
    };
    let mut output = String::with_capacity(value.len().saturating_sub(range.len()));
    output.push_str(value[..range.start].trim_end());
    if !output.is_empty() && !value[range.end..].trim_start().is_empty() {
        output.push(' ');
    }
    output.push_str(value[range.end..].trim_start());
    (output.trim().to_string(), true)
}

fn find_top_level_keyword(value: &str, keyword: &str) -> Option<std::ops::Range<usize>> {
    let first = keyword.chars().next()?;
    let mut depth = 0usize;
    for (start, character) in value.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && character.eq_ignore_ascii_case(&first) => {
                let end = start + keyword.len();
                if value
                    .get(start..end)
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(keyword))
                    && keyword_boundary(value, start, end)
                {
                    return Some(start..end);
                }
            }
            _ => {}
        }
    }
    None
}

fn keyword_boundary(value: &str, start: usize, end: usize) -> bool {
    !value[..start]
        .chars()
        .next_back()
        .is_some_and(is_css_identifier_character)
        && !value[end..]
            .chars()
            .next()
            .is_some_and(is_css_identifier_character)
}

fn is_css_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '-'
}

/// Updates the temporary used line-height projection from the computed value.
///
/// CSS Cascade separates computed values from used values; this keeps the
/// legacy numeric layout fields derived from `ComputedLineHeight` until layout
/// can consume the typed value directly:
/// <https://www.w3.org/TR/css-cascade-5/#computed>.
pub(crate) fn project_line_height(style: &mut ComputedStyle) {
    let (line_height, multiplier, is_normal) = match style.line_height_value {
        ComputedLineHeight::Normal => (style.font_size * 1.2, Some(1.2), true),
        ComputedLineHeight::Number(multiplier) => {
            (style.font_size * multiplier, Some(multiplier), false)
        }
        ComputedLineHeight::Length(length) => (length, None, false),
    };
    style.line_height = line_height;
    style.line_height_multiplier = multiplier;
    style.line_height_is_normal = is_normal;
}

pub(crate) fn parse_line_height(value: &str, font_size: f32) -> Option<(f32, Option<f32>, bool)> {
    let mut style = ComputedStyle {
        font_size,
        ..ComputedStyle::initial()
    };
    style.line_height_value = parse_computed_line_height(value, font_size)?;
    project_line_height(&mut style);
    Some((
        style.line_height,
        style.line_height_multiplier,
        style.line_height_is_normal,
    ))
}

/// Removes CSS declaration priority syntax before property value parsing.
///
/// CSS Cascade Level 5 defines `!important` as declaration priority rather
/// than part of the property value:
/// <https://www.w3.org/TR/css-cascade-5/#importance>.
pub(crate) fn trim_css_value(value: &str) -> &str {
    let value = value.trim();
    let important = "!important";
    let suffix_start = value.len().saturating_sub(important.len());
    if let Some(suffix) = value.get(suffix_start..)
        && suffix.eq_ignore_ascii_case(important)
    {
        value.get(..suffix_start).unwrap_or(value).trim_end()
    } else {
        value
    }
}

pub(crate) fn parse_css_string_token(value: &str) -> Option<(String, &str)> {
    let mut chars = value.char_indices();
    let (_, quote) = chars.next()?;
    if !matches!(quote, '"' | '\'') {
        return None;
    }
    let mut output = String::new();
    while let Some((index, character)) = chars.next() {
        if character == '\\' {
            push_css_string_escape(&mut output, &mut chars);
        } else if character == quote {
            return Some((output, &value[index + character.len_utf8()..]));
        } else {
            output.push(character);
        }
    }
    None
}

/// Decodes one escaped CSS string component.
///
/// CSS Syntax defines string escapes as either up to six hexadecimal digits
/// plus optional trailing whitespace, or a single escaped code point:
/// <https://www.w3.org/TR/css-syntax-3/#consume-escaped-code-point>.
fn push_css_string_escape(output: &mut String, chars: &mut std::str::CharIndices<'_>) {
    let mut clone = chars.clone();
    let mut hex = String::new();
    while hex.len() < 6 {
        let Some((_, character)) = clone.next() else {
            break;
        };
        if character.is_ascii_hexdigit() {
            hex.push(character);
        } else {
            break;
        }
    }
    if !hex.is_empty() {
        for _ in 0..hex.len() {
            chars.next();
        }
        if let Ok(codepoint) = u32::from_str_radix(&hex, 16)
            && let Some(character) = char::from_u32(codepoint)
        {
            output.push(character);
        }
        if chars
            .clone()
            .next()
            .is_some_and(|(_, character)| character.is_whitespace())
        {
            chars.next();
        }
        return;
    }
    if let Some((_, character)) = chars.next()
        && !matches!(character, '\n' | '\r' | '\u{000c}')
    {
        output.push(character);
    }
}

pub(crate) fn strip_ascii_function<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let prefix_len = name.len();
    let prefix = value.get(..prefix_len)?;
    if !prefix.eq_ignore_ascii_case(name) {
        return None;
    }
    let after_name = value[prefix_len..].trim_start();
    after_name.strip_prefix('(')
}

pub(crate) fn split_function_argument(value_after_open: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in value_after_open.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() {
            if character == '\\' {
                escaped = true;
            } else if Some(character) == quote {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '(' => depth += 1,
            ')' if depth == 0 => {
                return Some((&value_after_open[..index], &value_after_open[index + 1..]));
            }
            ')' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

pub(crate) fn is_css_ident_continue(character: char) -> bool {
    character == '-'
        || character == '_'
        || character.is_ascii_alphanumeric()
        || !character.is_ascii()
}
