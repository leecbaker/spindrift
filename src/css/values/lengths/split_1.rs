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
    root_font_size: f32,
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
    // `calc-size(any, <calc-sum>)` has no intrinsic sizing behavior: `any`
    // denotes an unspecified definite size, so a calculation that does not
    // reference the special `size` keyword resolves exactly as its ordinary
    // length-percentage calculation. Keep this reduction at the parsing
    // boundary; calc-size values that reference `size` must retain their
    // intrinsic basis for the layout-time sizing algorithm.
    // <https://drafts.csswg.org/css-values-5/#calc-size>
    if let Some((basis, calculation)) = calc_size_arguments(value)
        && basis.eq_ignore_ascii_case("any")
        && !calculation
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '-')
            .any(|token| token.eq_ignore_ascii_case("size"))
    {
        return parse_computed_length_percentage_with_root(calculation, font_size, root_font_size)
            .map(ComputedLengthPercentageOrAuto::LengthPercentage);
    }
    if let Some(calc_size) = parse_calc_size(value, font_size, root_font_size) {
        return Some(ComputedLengthPercentageOrAuto::CalcSize(calc_size));
    }
    if let Some(SpecifiedLength::FontRelativeEm(value)) = parse_specified_length(value) {
        return Some(ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_em(value),
        ));
    }
    if let Some(SpecifiedLength::RootFontRelativeRem(value)) = parse_specified_length(value) {
        return Some(ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_rem(value),
        ));
    }
    if let Some(SpecifiedLength::FontRelativeIc(value)) = parse_specified_length(value) {
        return Some(ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_ic(value),
        ));
    }
    if let Some(SpecifiedLength::FontRelativeEx(value)) = parse_specified_length(value) {
        return Some(ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_ex(value),
        ));
    }
    if let Some(SpecifiedLength::FontRelativeCap(value)) = parse_specified_length(value) {
        return Some(ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_cap(value),
        ));
    }
    if let Some(argument) = fit_content_argument(value)
        && let Some(length) =
            parse_computed_length_percentage_with_root(argument, font_size, root_font_size)
        && !length_percentage_is_definitely_negative(length.clone())
    {
        return Some(ComputedLengthPercentageOrAuto::FitContent(Some(length)));
    }
    parse_computed_length_percentage_with_root(value, font_size, root_font_size)
        .map(ComputedLengthPercentageOrAuto::LengthPercentage)
}

/// Splits the two top-level arguments of a `calc-size()` function.
fn calc_size_arguments(value: &str) -> Option<(&str, &str)> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    lower
        .strip_prefix("calc-size(")
        .and_then(|rest| rest.strip_suffix(')'))?;
    let start = "calc-size(".len();
    let arguments = &value[start..value.len().checked_sub(1)?];
    let mut depth = 0usize;
    for (index, character) in arguments.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                return Some((arguments[..index].trim(), arguments[index + 1..].trim()));
            }
            _ => {}
        }
    }
    None
}

/// Parses a retained `calc-size()` calculation.
///
/// A `calc-size()` calculation is evaluated after its sizing basis is known.
/// The arithmetic branches are affine in `size`; CSS Math min/max/clamp
/// comparisons retain their bounds until layout supplies that basis:
/// <https://drafts.csswg.org/css-values-5/#calc-size>.
fn parse_calc_size(value: &str, font_size: f32, root_font_size: f32) -> Option<CalcSize> {
    let (basis, calculation) = calc_size_arguments(value)?;
    let basis = parse_calc_size_basis(basis, font_size, root_font_size)?;
    let calculation = trim_css_value(calculation);
    let calculation = function_arguments(calculation, "calc")
        .filter(|arguments| arguments.len() == 1)
        .map(|arguments| arguments[0])
        .unwrap_or(calculation);
    let (primary, lower_bound, upper_bound) =
        if let Some(arguments) = function_arguments(calculation, "min") {
            if arguments.len() != 2 {
                return None;
            }
            (
                parse_calc_size_affine(arguments[0], font_size, root_font_size)?,
                None,
                Some(parse_calc_size_affine(
                    arguments[1],
                    font_size,
                    root_font_size,
                )?),
            )
        } else if let Some(arguments) = function_arguments(calculation, "max") {
            if arguments.len() != 2 {
                return None;
            }
            (
                parse_calc_size_affine(arguments[0], font_size, root_font_size)?,
                Some(parse_calc_size_affine(
                    arguments[1],
                    font_size,
                    root_font_size,
                )?),
                None,
            )
        } else if let Some(arguments) = function_arguments(calculation, "clamp") {
            if arguments.len() != 3 {
                return None;
            }
            (
                parse_calc_size_affine(arguments[1], font_size, root_font_size)?,
                Some(parse_calc_size_affine(
                    arguments[0],
                    font_size,
                    root_font_size,
                )?),
                Some(parse_calc_size_affine(
                    arguments[2],
                    font_size,
                    root_font_size,
                )?),
            )
        } else {
            (
                parse_calc_size_affine(calculation, font_size, root_font_size)?,
                None,
                None,
            )
        };
    Some(CalcSize {
        basis,
        size_multiplier: primary.size_multiplier,
        additive: primary.additive,
        lower_bound,
        upper_bound,
    })
}

fn parse_calc_size_affine(
    value: &str,
    font_size: f32,
    root_font_size: f32,
) -> Option<CalcSizeAffine> {
    let without_size = replace_calc_size_keyword(value, "0px").unwrap_or_else(|| value.to_owned());
    let unit_size = replace_calc_size_keyword(value, "1px").unwrap_or_else(|| value.to_owned());
    let additive =
        parse_computed_length_percentage_with_root(&without_size, font_size, root_font_size)?;
    let unit_value =
        parse_computed_length_percentage_with_root(&unit_size, font_size, root_font_size)?;
    let multiplier = layout_points(unit_value.difference_if_absolute(&additive)?) / CSS_PX_TO_PT;
    multiplier.is_finite().then_some(CalcSizeAffine {
        size_multiplier: multiplier,
        additive,
    })
}

/// Splits a CSS function's comma-separated top-level arguments.
fn function_arguments<'a>(value: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let value = trim_css_value(value);
    let prefix = format!("{name}(");
    if !value.get(..prefix.len())?.eq_ignore_ascii_case(&prefix) || !value.ends_with(')') {
        return None;
    }
    let inner = &value[prefix.len()..value.len().checked_sub(1)?];
    let mut arguments = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (index, character) in inner.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                arguments.push(inner[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    (depth == 0).then(|| {
        arguments.push(inner[start..].trim());
        arguments
    })
}

fn parse_calc_size_basis(
    value: &str,
    font_size: f32,
    root_font_size: f32,
) -> Option<CalcSizeBasis> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("auto") {
        return Some(CalcSizeBasis::Auto);
    }
    if value.eq_ignore_ascii_case("min-content") {
        return Some(CalcSizeBasis::MinContent);
    }
    if value.eq_ignore_ascii_case("max-content") {
        return Some(CalcSizeBasis::MaxContent);
    }
    if value.eq_ignore_ascii_case("fit-content") {
        return Some(CalcSizeBasis::FitContent);
    }
    if value.eq_ignore_ascii_case("stretch") {
        return Some(CalcSizeBasis::Stretch);
    }
    parse_computed_length_percentage_with_root(value, font_size, root_font_size)
        .map(CalcSizeBasis::LengthPercentage)
}

/// Replaces standalone occurrences of calc-size's special `size` keyword.
///
/// CSS identifiers may contain hyphens and ASCII letters, so textual
/// replacement must distinguish `size` from identifiers such as `resize`.
fn replace_calc_size_keyword(value: &str, replacement: &str) -> Option<String> {
    let mut result = String::with_capacity(value.len());
    let mut replaced = false;
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let rest = &value[index..];
        if rest.len() >= 4
            && rest[..4].eq_ignore_ascii_case("size")
            && !rest
                .as_bytes()
                .get(4)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_')
            && (index == 0
                || !bytes[index - 1].is_ascii_alphanumeric()
                    && bytes[index - 1] != b'-'
                    && bytes[index - 1] != b'_')
        {
            result.push_str(replacement);
            index += 4;
            replaced = true;
        } else {
            let character = rest.chars().next()?;
            result.push(character);
            index += character.len_utf8();
        }
    }
    replaced.then_some(result)
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
        && !length_percentage_is_definitely_negative(length.clone())
    {
        return Some(ComputedFlexBasis::FitContent(Some(length)));
    }
    if value.eq_ignore_ascii_case("auto") {
        return Some(ComputedFlexBasis::Auto);
    }
    let length = parse_computed_length_percentage(value, font_size)?;
    (!length_percentage_is_definitely_negative(length.clone())).then_some(
        ComputedFlexBasis::LengthPercentage(ComputedFlexBasisLength::new(length)),
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
    value.is_definitely_negative()
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
    parse_math_length_percentage_with_root(value, font_size, ROOT_FONT_SIZE_PT)
}

fn parse_math_length_percentage_with_root(
    value: &str,
    font_size: f32,
    root_font_size: f32,
) -> Option<ComputedLengthPercentage> {
    match parse_math_value_with_root(value, font_size, root_font_size)? {
        MathValue::LengthPercentage(value) => Some(value),
        MathValue::Number(value) => Some(ComputedLengthPercentage::from_points(value)),
    }
}

pub(in crate::css) fn parse_math_value(value: &str, font_size: f32) -> Option<MathValue> {
    parse_math_value_with_root(value, font_size, ROOT_FONT_SIZE_PT)
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
}

impl MathValue {
    pub(in crate::css) fn add(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => Some(Self::Number(left + right)),
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
        })
    }

    fn negated(self) -> Option<Self> {
        Some(match self {
            Self::Number(value) => Self::Number(-value),
            Self::LengthPercentage(value) => Self::LengthPercentage(value.negated()),
        })
    }

    pub(in crate::css) fn ordering_against(self, other: Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.partial_cmp(&right),
            (Self::LengthPercentage(left), Self::LengthPercentage(right)) => {
                left.computed_ordering(&right)
            }
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
        Token::Dimension { value, unit, .. } => {
            parse_math_dimension(value, &unit, font_size, root_font_size)
                .map(MathValue::LengthPercentage)
        }
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

pub(crate) fn set_font_size(style: &mut ComputedStyle, font_size: f32) {
    style.font_size = clamp_used_layout_length(layout_pt(font_size)).points();
    style.deferred_font_size = DeferredFontSize::Absolute(style.font_size);
    project_line_height(style);
}

pub(crate) fn set_deferred_font_size(
    style: &mut ComputedStyle,
    font_size: DeferredFontSize,
    parent_font_size: f32,
    parent_ch_advance: LayoutLength,
) {
    let font_size = match font_size {
        DeferredFontSize::ParentLineHeight(multiplier) => {
            DeferredFontSize::Absolute(multiplier * style.line_height)
        }
        font_size => font_size,
    };
    style.font_size = clamp_used_layout_length(font_size.resolve(
        crate::css::FontRelativeLengthBasis::new(layout_pt(parent_font_size), parent_ch_advance),
    ))
    .points();
    style.deferred_font_size = font_size;
    project_line_height(style);
}

pub(crate) fn fallback_ch_advance_for_style(style: &ComputedStyle) -> LayoutLength {
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
) -> LayoutLength {
    if matches!(
        writing_mode.text_layout_policy(text_orientation),
        TextLayoutPolicy::Vertical(TextOrientation::Upright)
    ) {
        layout_pt(font_size)
    } else {
        layout_pt(font_size * 0.5)
    }
}

pub(crate) fn parse_font_size(value: &str, parent_font_size: f32) -> Option<f32> {
    parse_font_size_with_parent_ch_advance(
        value,
        parent_font_size,
        layout_pt(parent_font_size * 0.5),
    )
}

/// Parses `font-size` without requiring a parent font metric.
///
/// The returned representation is resolved only once the parent's selected
/// font is known. This is the font-specific counterpart to deferred
/// `<length-percentage>` used-value resolution.
pub(crate) fn parse_deferred_font_size(value: &str) -> Option<DeferredFontSize> {
    let value = trim_css_value(value);
    let lower = value.to_ascii_lowercase();
    let absolute = match lower.as_str() {
        "xx-small" => Some(7.0),
        "x-small" => Some(8.3),
        "small" => Some(10.0),
        "medium" => Some(12.0),
        "large" => Some(14.4),
        "x-large" => Some(17.3),
        "xx-large" => Some(20.7),
        "xxx-large" => Some(24.9),
        _ => None,
    };
    if let Some(value) = absolute {
        return Some(DeferredFontSize::Absolute(value));
    }
    if lower == "smaller" {
        return Some(DeferredFontSize::RelativeToParent(
            ComputedLengthPercentage::from_em(1.0 / 1.2),
        ));
    }
    if lower == "larger" {
        return Some(DeferredFontSize::RelativeToParent(
            ComputedLengthPercentage::from_em(1.2),
        ));
    }
    if let Some(em) = lower
        .strip_suffix("em")
        .and_then(|value| value.parse::<f32>().ok())
    {
        return Some(DeferredFontSize::RelativeToParent(
            ComputedLengthPercentage::from_em(em),
        ));
    }
    if let Some(lh) = lower
        .strip_suffix("lh")
        .and_then(|value| value.parse::<f32>().ok())
    {
        return Some(DeferredFontSize::ParentLineHeight(lh));
    }
    parse_math_length_percentage_with_root(value, 0.0, ROOT_FONT_SIZE_PT)
        .map(DeferredFontSize::RelativeToParent)
        .or_else(|| parse_length(value).map(DeferredFontSize::Absolute))
}

pub(crate) fn parse_font_size_with_parent_ch_advance(
    value: &str,
    parent_font_size: f32,
    parent_ch_advance: LayoutLength,
) -> Option<f32> {
    parse_deferred_font_size(value).map(|value| {
        value
            .resolve(crate::css::FontRelativeLengthBasis::new(
                layout_pt(parent_font_size),
                parent_ch_advance,
            ))
            .points()
    })
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
    if let Some(value) = parse_computed_length_percentage(value, font_size) {
        let value = value
            .clone()
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(font_size)))
            .map(ComputedLengthPercentage::from_layout_length)
            .unwrap_or(value);
        return Some(ComputedLineHeight::Length(value));
    }
    parse_length(value).map(ComputedLineHeight::from_points)
}

/// Parses `letter-spacing` into a computed length projection.
///
/// CSS Text Level 4 defines `letter-spacing` as `normal | <length-percentage>`
/// and makes the property inherited. The percentage remains unresolved until
/// the used font size is known; `normal` remains zero additional spacing until
/// justification-driven spacing is modeled separately:
/// <https://drafts.csswg.org/css-text-4/#letter-spacing-property>.
pub(crate) fn parse_letter_spacing(
    value: &str,
    font_size: f32,
) -> Option<ComputedLengthPercentage> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(ComputedLengthPercentage::ZERO);
    }
    if let Some(value) = parse_computed_length_percentage(value, font_size) {
        return Some(value);
    }
    parse_length(value).map(ComputedLengthPercentage::from_points)
}

/// Parses `word-spacing` into a computed length projection.
///
/// CSS Text Level 4 defines `word-spacing` as `normal | <length-percentage>`
/// and makes the property inherited. The renderer stores `normal` as zero
/// additional spacing and preserves percentages until used-value resolution
/// against the current element's used font size:
/// <https://www.w3.org/TR/css-text-4/#word-spacing-property>.
pub(crate) fn parse_word_spacing(value: &str, font_size: f32) -> Option<ComputedLengthPercentage> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return Some(ComputedLengthPercentage::ZERO);
    }
    parse_computed_length_percentage(value, font_size)
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
        && !length.contains_percentage()
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
