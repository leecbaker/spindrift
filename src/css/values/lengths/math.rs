use super::*;

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
    parse_math_length_percentage_with_root(value, font_size, ROOT_FONT_SIZE_PT)
}

pub(super) fn parse_math_length_percentage_with_root(
    value: &str,
    font_size: f32,
    root_font_size: f32,
) -> Option<ComputedLengthPercentage> {
    match parse_math_value_with_root(value, font_size, root_font_size)? {
        MathValue::LengthPercentage(value) => Some(value),
        MathValue::Number(value) => Some(ComputedLengthPercentage::from_points(value)),
        MathValue::Resolution(_) => None,
    }
}

pub(in crate::css) fn parse_math_value(value: &str, font_size: f32) -> Option<MathValue> {
    parse_math_value_with_root(value, font_size, ROOT_FONT_SIZE_PT)
}

/// Parse a CSS Values `<resolution>` calculation and return its canonical
/// dots-per-CSS-pixel value.
///
/// This uses the same dimension algebra as other CSS math values, so `calc()`,
/// `min()`, `max()`, and `clamp()` reject incompatible dimensions instead of
/// relying on image-set-specific string arithmetic.
/// <https://drafts.csswg.org/css-values-4/#resolution>
pub(crate) fn parse_math_resolution(value: &str) -> Option<f32> {
    match parse_math_value_with_root(value, 0.0, ROOT_FONT_SIZE_PT)? {
        MathValue::Resolution(value) if value.is_finite() => Some(value),
        _ => None,
    }
}

fn parse_math_value_with_root(
    value: &str,
    font_size: f32,
    root_font_size: f32,
) -> Option<MathValue> {
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    let value = parse_math_sum(&mut parser, font_size, root_font_size)?;
    parser.is_exhausted().then_some(value)
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::css) enum MathValue {
    Number(f32),
    LengthPercentage(ComputedLengthPercentage),
    /// Canonical CSS dots-per-pixel. `dppx` is the canonical resolution unit.
    /// <https://drafts.csswg.org/css-values-4/#resolution>
    Resolution(f32),
}

impl MathValue {
    pub(in crate::css) fn add(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => Some(Self::Number(left + right)),
            (Self::Resolution(left), Self::Resolution(right)) => {
                Some(Self::Resolution(left + right))
            }
            (Self::LengthPercentage(left), Self::LengthPercentage(right)) => Some(
                Self::LengthPercentage(ComputedLengthPercentage::sum(left, right)),
            ),
            _ => None,
        }
    }

    pub(in crate::css) fn sub(self, other: Self) -> Option<Self> {
        self.add(other.negated()?)
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
                Self::LengthPercentage(ComputedLengthPercentage::product(value, number))
            }
            Self::Resolution(value) => Self::Resolution(value * number),
        })
    }

    fn negated(self) -> Option<Self> {
        Some(match self {
            Self::Number(value) => Self::Number(-value),
            Self::LengthPercentage(value) => Self::LengthPercentage(value.negated()),
            Self::Resolution(value) => Self::Resolution(-value),
        })
    }

    pub(in crate::css) fn ordering_against(self, other: Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.partial_cmp(&right),
            (Self::LengthPercentage(left), Self::LengthPercentage(right)) => {
                left.computed_ordering(&right)
            }
            (Self::Resolution(left), Self::Resolution(right)) => left.partial_cmp(&right),
            _ => None,
        }
    }
}

pub(in crate::css) fn parse_math_sum(
    input: &mut Parser<'_, '_>,
    font_size: f32,
    root_font_size: f32,
) -> Option<MathValue> {
    let mut value = parse_math_product(input, font_size, root_font_size)?;
    loop {
        if input.try_parse(|input| input.expect_delim('+')).is_ok() {
            value = value.add(parse_math_product(input, font_size, root_font_size)?)?;
        } else if input.try_parse(|input| input.expect_delim('-')).is_ok() {
            value = value.sub(parse_math_product(input, font_size, root_font_size)?)?;
        } else {
            return Some(value);
        }
    }
}

pub(in crate::css) fn parse_math_product(
    input: &mut Parser<'_, '_>,
    font_size: f32,
    root_font_size: f32,
) -> Option<MathValue> {
    let mut value = parse_math_primary(input, font_size, root_font_size)?;
    loop {
        if input.try_parse(|input| input.expect_delim('*')).is_ok() {
            value = value.mul(parse_math_primary(input, font_size, root_font_size)?)?;
        } else if input.try_parse(|input| input.expect_delim('/')).is_ok() {
            value = value.div(parse_math_primary(input, font_size, root_font_size)?)?;
        } else {
            return Some(value);
        }
    }
}

pub(in crate::css) fn parse_math_primary(
    input: &mut Parser<'_, '_>,
    font_size: f32,
    root_font_size: f32,
) -> Option<MathValue> {
    let token = input.next().ok()?.clone();
    match token {
        Token::Number { value, .. } => Some(MathValue::Number(value)),
        Token::Percentage { unit_value, .. } => Some(MathValue::LengthPercentage(
            ComputedLengthPercentage::from_percent(unit_value),
        )),
        Token::Dimension { value, unit, .. } => parse_math_resolution_dimension(value, &unit)
            .map(MathValue::Resolution)
            .or_else(|| {
                parse_math_dimension(value, &unit, font_size, root_font_size)
                    .map(MathValue::LengthPercentage)
            }),
        Token::Function(name) => {
            let name = name.to_string();
            input
                .parse_nested_block(
                    |input| -> Result<MathValue, cssparser::ParseError<'_, ()>> {
                        parse_math_function(&name, input, font_size, root_font_size)
                            .ok_or_else(|| input.new_custom_error(()))
                    },
                )
                .ok()
        }
        Token::ParenthesisBlock => input
            .parse_nested_block(
                |input| -> Result<MathValue, cssparser::ParseError<'_, ()>> {
                    let value = parse_math_sum(input, font_size, root_font_size)
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

fn parse_math_resolution_dimension(value: f32, unit: &str) -> Option<f32> {
    let factor = if unit.eq_ignore_ascii_case("dppx") || unit.eq_ignore_ascii_case("x") {
        1.0
    } else if unit.eq_ignore_ascii_case("dpi") {
        1.0 / 96.0
    } else if unit.eq_ignore_ascii_case("dpcm") {
        2.54 / 96.0
    } else {
        return None;
    };
    Some(value * factor)
}

pub(in crate::css) fn parse_math_dimension(
    value: f32,
    unit: &str,
    _font_size: f32,
    _root_font_size: f32,
) -> Option<ComputedLengthPercentage> {
    if let Some(value) = ComputedLengthPercentage::from_container_unit(unit, value) {
        return Some(value);
    }
    let unit = if unit.eq_ignore_ascii_case("rlh") {
        return Some(ComputedLengthPercentage::from_rlh(value));
    } else if unit.eq_ignore_ascii_case("rex") {
        return Some(ComputedLengthPercentage::from_rex(value));
    } else if unit.eq_ignore_ascii_case("rcap") {
        return Some(ComputedLengthPercentage::from_rcap(value));
    } else if unit.eq_ignore_ascii_case("rch") {
        return Some(ComputedLengthPercentage::from_rch(value));
    } else if unit.eq_ignore_ascii_case("ric") {
        return Some(ComputedLengthPercentage::from_ric(value));
    } else if unit.eq_ignore_ascii_case("rem") {
        // `rem` computes against the used root font size. The root itself can
        // depend on viewport or font-metric units, so retaining this component
        // through the computed-value phase is required for descendants to see
        // the eventual root basis:
        // <https://www.w3.org/TR/css-values-4/#rem>
        return Some(ComputedLengthPercentage::from_rem(value));
    } else if unit.eq_ignore_ascii_case("em") {
        // Ordinary `em` likewise resolves after the element's winning font
        // size is known; do not fold it against a provisional cascade value.
        // <https://www.w3.org/TR/css-values-4/#em>
        return Some(ComputedLengthPercentage::from_em(value));
    } else if unit.eq_ignore_ascii_case("ch") {
        return Some(ComputedLengthPercentage::from_ch(value));
    } else if unit.eq_ignore_ascii_case("ex") {
        return Some(ComputedLengthPercentage::from_ex(value));
    } else if unit.eq_ignore_ascii_case("cap") {
        return Some(ComputedLengthPercentage::from_cap(value));
    } else if unit.eq_ignore_ascii_case("ic") {
        return Some(ComputedLengthPercentage::from_ic(value));
    } else if unit.eq_ignore_ascii_case("lh") {
        return Some(ComputedLengthPercentage::from_lh(value));
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
    root_font_size: f32,
) -> Option<MathValue> {
    if name.eq_ignore_ascii_case("calc") {
        let value = parse_math_sum(input, font_size, root_font_size)?;
        return input.is_exhausted().then_some(value);
    }
    if name.eq_ignore_ascii_case("min") || name.eq_ignore_ascii_case("max") {
        let values = parse_comma_separated_math_values(input, font_size, root_font_size)?;
        let choose_max = name.eq_ignore_ascii_case("max");
        return compare_math_values(&values, choose_max)
            .or_else(|| defer_min_max_math_values(&values, choose_max));
    }
    if name.eq_ignore_ascii_case("clamp") {
        let values = parse_comma_separated_math_values(input, font_size, root_font_size)?;
        let [min, center, max] = values.as_slice() else {
            return None;
        };
        let min = min.clone();
        let center = center.clone();
        let max = max.clone();
        if let Some(below_max) = compare_math_values(&[center.clone(), max.clone()], false)
            && let Some(value) = compare_math_values(&[below_max, min.clone()], true)
        {
            return Some(value);
        }
        return defer_clamp_math_values(min, center, max);
    }
    if name.eq_ignore_ascii_case("sign") {
        let value = parse_math_sum(input, font_size, root_font_size)?;
        input.is_exhausted().then_some(())?;
        return match value {
            MathValue::Number(value) | MathValue::Resolution(value) => {
                Some(MathValue::Number(value.signum()))
            }
            MathValue::LengthPercentage(_) => None,
        };
    }
    None
}

pub(in crate::css) fn parse_comma_separated_math_values(
    input: &mut Parser<'_, '_>,
    font_size: f32,
    root_font_size: f32,
) -> Option<Vec<MathValue>> {
    let mut values = Vec::new();
    loop {
        values.push(parse_math_sum(input, font_size, root_font_size)?);
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
    let mut best = values.first()?.clone();
    for candidate in &values[1..] {
        let ordering = candidate.clone().ordering_against(best.clone())?;
        if (choose_max && ordering.is_gt()) || (!choose_max && ordering.is_lt()) {
            best = candidate.clone();
        }
    }
    Some(best)
}

pub(in crate::css) fn defer_min_max_math_values(
    values: &[MathValue],
    choose_max: bool,
) -> Option<MathValue> {
    let mut values = values.iter().cloned();
    let MathValue::LengthPercentage(mut value) = values.next()? else {
        return None;
    };
    for next in values {
        let MathValue::LengthPercentage(next) = next else {
            return None;
        };
        value = if choose_max {
            ComputedLengthPercentage::max(value, next)
        } else {
            ComputedLengthPercentage::min(value, next)
        };
    }
    Some(MathValue::LengthPercentage(value))
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
    Some(MathValue::LengthPercentage(
        ComputedLengthPercentage::clamp(min, center, max),
    ))
}
