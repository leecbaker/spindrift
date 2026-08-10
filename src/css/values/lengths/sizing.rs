use super::*;

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
    if let Some(SpecifiedLength::FontRelativeLh(value)) = parse_specified_length(value) {
        return Some(ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_lh(value),
        ));
    }
    if let Some(SpecifiedLength::RootFontRelativeRlh(value)) = parse_specified_length(value) {
        return Some(ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_rlh(value),
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
