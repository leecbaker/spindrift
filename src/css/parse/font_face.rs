use super::*;
use crate::CssColor;
use crate::css::parse::LayerRegistry;
use crate::css::values::{
    parse_computed_length_percentage, parse_font_feature_settings, parse_font_variant,
    parse_font_variant_alternates, parse_font_variant_caps, parse_font_variant_east_asian,
    parse_font_variant_ligatures, parse_font_variant_numeric, parse_font_variant_position,
    parse_font_variation_settings,
};
use crate::css::{
    FontFeatureSettings, FontFeatureValue, FontFeatureValues, FontFeatureValuesBlock, FontPalette,
    FontPaletteDefinition, FontRelativeLengthBasis, FontVariantAlternates, FontVariantCaps,
    FontVariantEastAsian, FontVariantLigatures, FontVariantNumeric, FontVariantPosition,
    FontVariationSettings, ROOT_FONT_SIZE_PT, parse_font_palette,
};
use crate::css::{LayerName, LayerOrder};
use crate::units::layout_pt;
use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, Parser, ParserInput, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser,
};

/// Parse one active `@font-face` descriptor block.
///
/// The stylesheet rule parser calls this only after its containing conditional
/// groups have matched, so the resulting resource has the same activation
/// semantics as ordinary style rules.
/// <https://www.w3.org/TR/css-fonts-4/#font-face-rule>
pub(in crate::css) fn parse_font_face_rule(
    block: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Option<CssFontFace> {
    let declarations = parse_declarations_with_urls(block, base_url, root_url);
    // `@font-face` descriptors do not participate in the element cascade,
    // so no custom-property environment exists for `var()` substitution.
    // A variable reference therefore invalidates the descriptor and the
    // face is not usable.
    // <https://www.w3.org/TR/css-variables-1/#using-variables>
    if let Some(family_value) = declarations.get("font-family")
        && !crate::css::cascade::variables::contains_css_variable_reference(family_value)
        && let Some(family) = parse_font_family_names(family_value).into_iter().next()
    {
        let sources = declarations
            .get("src")
            .map(|value| parse_font_face_sources(value, base_url, root_url))
            .unwrap_or_default();
        if !sources.is_empty() {
            let mut face = CssFontFace {
                family,
                sources,
                unicode_range: declarations
                    .get("unicode-range")
                    .and_then(|value| parse_unicode_range(value)),
                size_adjust: declarations
                    .get("size-adjust")
                    .and_then(|value| parse_font_face_size_adjust(value)),
                ascent_override: declarations
                    .get("ascent-override")
                    .and_then(|value| parse_font_face_metric_override(value)),
                descent_override: declarations
                    .get("descent-override")
                    .and_then(|value| parse_font_face_metric_override(value)),
                line_gap_override: declarations
                    .get("line-gap-override")
                    .and_then(|value| parse_font_face_metric_override(value)),
                weight: declarations
                    .get("font-weight")
                    .and_then(|value| parse_font_weight(value, FontWeight::NORMAL))
                    .unwrap_or(FontWeight::NORMAL),
                weight_is_variable: font_face_axis_is_variable(
                    declarations.get("font-weight").map(String::as_str),
                ),
                style: declarations
                    .get("font-style")
                    .and_then(|value| parse_font_style(value))
                    .unwrap_or(FontStyle::Normal),
                width: declarations
                    .get("font-width")
                    .or_else(|| declarations.get("font-stretch"))
                    .and_then(|value| parse_font_width(value))
                    .unwrap_or(FontWidth::NORMAL),
                width_is_variable: font_face_axis_is_variable(
                    declarations
                        .get("font-width")
                        .or_else(|| declarations.get("font-stretch"))
                        .map(String::as_str),
                ),
                font_variation_settings: declarations
                    .get("font-variation-settings")
                    .and_then(|value| parse_font_variation_settings(value))
                    .unwrap_or(FontVariationSettings::NORMAL),
                font_feature_settings: declarations
                    .get("font-feature-settings")
                    .and_then(|value| parse_font_feature_settings(value))
                    .unwrap_or(FontFeatureSettings::NORMAL),
                font_variant_ligatures: FontVariantLigatures::Normal,
                font_variant_position: FontVariantPosition::Normal,
                font_variant_caps: FontVariantCaps::Normal,
                font_variant_numeric: FontVariantNumeric::Normal,
                font_variant_alternates: FontVariantAlternates::Normal,
                font_variant_east_asian: FontVariantEastAsian::Normal,
            };
            apply_font_face_variant_descriptors(&mut face, &declarations);
            return Some(face);
        }
    }
    None
}

/// CSS Fonts Level 4 makes `auto` the descriptor initial value for variable
/// axes. A two-value descriptor denotes a range, which must likewise retain
/// the font's intrinsic axis rather than pinning registration to one value.
/// <https://www.w3.org/TR/css-fonts-4/#font-prop-desc>
fn font_face_axis_is_variable(value: Option<&str>) -> bool {
    value.is_none_or(|value| {
        let tokens = split_css_component_values(trim_css_value(value));
        tokens.len() != 1 || tokens[0].eq_ignore_ascii_case("auto")
    })
}

/// Parse the `@font-face size-adjust` percentage descriptor.
///
/// <https://www.w3.org/TR/css-fonts-5/#descdef-font-face-size-adjust>
fn parse_font_face_size_adjust(value: &str) -> Option<u32> {
    let percent = trim_css_value(value)
        .strip_suffix('%')?
        .trim()
        .parse::<f32>()
        .ok()?;
    let factor = percent / 100.0;
    (factor.is_finite() && factor >= 0.0).then_some(factor.to_bits())
}

/// Parse a metric override descriptor. `normal` retains the font's table
/// metric; a non-negative percentage replaces it with a fraction of one em.
/// <https://www.w3.org/TR/css-fonts-5/#font-metric-override-desc>
fn parse_font_face_metric_override(value: &str) -> Option<u32> {
    let value = trim_css_value(value);
    if value.eq_ignore_ascii_case("normal") {
        return None;
    }
    parse_font_face_size_adjust(value)
}

/// Parse CSS Fonts' `unicode-range` descriptor.
///
/// The descriptor is an inclusive comma-separated list of `U+` ranges, single
/// code points, or wildcard ranges. Font matching later treats absent
/// `unicode-range` as the full Unicode scalar range:
/// <https://www.w3.org/TR/css-fonts-4/#unicode-range-desc>.
pub(crate) fn parse_unicode_range(value: &str) -> Option<Vec<UnicodeRange>> {
    let ranges = value
        .split(',')
        .map(|part| parse_unicode_range_part(part.trim()))
        .collect::<Option<Vec<_>>>()?;
    (!ranges.is_empty()).then_some(ranges)
}

fn parse_unicode_range_part(value: &str) -> Option<UnicodeRange> {
    let body = value
        .strip_prefix("U+")
        .or_else(|| value.strip_prefix("u+"))?
        .trim();
    if body.is_empty() || !body.is_ascii() {
        return None;
    }
    if body.contains('?') {
        return parse_wildcard_unicode_range(body);
    }
    if let Some((start, end)) = body.split_once('-') {
        let start = parse_unicode_scalar_hex(start.trim())?;
        let end = parse_unicode_scalar_hex(end.trim())?;
        return (start <= end).then_some(UnicodeRange { start, end });
    }
    let scalar = parse_unicode_scalar_hex(body)?;
    Some(UnicodeRange {
        start: scalar,
        end: scalar,
    })
}

fn parse_wildcard_unicode_range(body: &str) -> Option<UnicodeRange> {
    if body.len() > 6
        || !body
            .bytes()
            .all(|byte| byte == b'?' || byte.is_ascii_hexdigit())
    {
        return None;
    }
    let start = parse_unicode_scalar_hex(&body.replace('?', "0"))?;
    let end = parse_unicode_scalar_hex(&body.replace('?', "F"))?;
    (start <= end).then_some(UnicodeRange { start, end })
}

fn parse_unicode_scalar_hex(value: &str) -> Option<u32> {
    if value.is_empty() || value.len() > 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let scalar = u32::from_str_radix(value, 16).ok()?;
    (scalar <= UnicodeRange::ALL.end).then_some(scalar)
}

fn apply_font_face_variant_descriptors(face: &mut CssFontFace, declarations: &Declarations) {
    if let Some(value) = declarations.get("font-variant")
        && !css_wide_keyword(value)
        && let Some(font_variant) = parse_font_variant(value)
    {
        face.font_variant_ligatures = font_variant.ligatures;
        face.font_variant_position = font_variant.position;
        face.font_variant_caps = font_variant.caps;
        face.font_variant_numeric = font_variant.numeric;
        face.font_variant_alternates = font_variant.alternates;
        face.font_variant_east_asian = font_variant.east_asian;
    }
    if let Some(value) = declarations.get("font-variant-ligatures")
        && !css_wide_keyword(value)
        && let Some(parsed) = parse_font_variant_ligatures(value)
    {
        face.font_variant_ligatures = parsed;
    }
    if let Some(value) = declarations.get("font-variant-position")
        && !css_wide_keyword(value)
        && let Some(parsed) = parse_font_variant_position(value)
    {
        face.font_variant_position = parsed;
    }
    if let Some(value) = declarations.get("font-variant-caps")
        && !css_wide_keyword(value)
        && let Some(parsed) = parse_font_variant_caps(value)
    {
        face.font_variant_caps = parsed;
    }
    if let Some(value) = declarations.get("font-variant-numeric")
        && !css_wide_keyword(value)
        && let Some(parsed) = parse_font_variant_numeric(value)
    {
        face.font_variant_numeric = parsed;
    }
    if let Some(value) = declarations.get("font-variant-alternates")
        && !css_wide_keyword(value)
        && let Some(parsed) = parse_font_variant_alternates(value)
    {
        face.font_variant_alternates = parsed;
    }
    if let Some(value) = declarations.get("font-variant-east-asian")
        && !css_wide_keyword(value)
        && let Some(parsed) = parse_font_variant_east_asian(value)
    {
        face.font_variant_east_asian = parsed;
    }
}

fn css_wide_keyword(value: &str) -> bool {
    matches!(
        trim_css_value(value).to_ascii_lowercase().as_str(),
        "inherit" | "initial" | "unset" | "revert" | "revert-layer"
    )
}

pub(super) fn parse_font_face_sources(
    value: &str,
    base_url: Option<&url::Url>,
    root_url: Option<&url::Url>,
) -> Vec<FontFaceSource> {
    let mut sources = Vec::new();
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    while !parser.is_exhausted() {
        if let Ok(value) = parser.try_parse(|input| input.expect_url()) {
            sources.push(FontFaceSource::Url {
                value: value.to_string(),
                base_url: base_url.cloned(),
                root_url: root_url.cloned(),
            });
        } else if parser.next_including_whitespace_and_comments().is_err() {
            break;
        }
    }
    sources
}

pub(in crate::css) fn parse_font_feature_values_rules(
    mut rules: Vec<FontFeatureValuesRule>,
    layers: &LayerRegistry,
) -> FontFeatureValues {
    for (order, rule) in rules.iter_mut().enumerate() {
        rule.order = order;
    }
    rules.sort_by_key(|rule| {
        (
            rule.layer
                .as_ref()
                .and_then(|layer| layers.order_for(layer))
                .unwrap_or(LayerOrder(vec![usize::MAX])),
            rule.order,
        )
    });
    let mut values = FontFeatureValues::default();
    for rule in rules {
        for family in parse_font_family_names(&rule.prelude) {
            parse_font_feature_values_block(&mut values, &family, &rule.block);
        }
    }
    values
}

/// A stylesheet-scoped `@font-feature-values` resource together with the
/// cascade layer in which it was declared. Unlike normal declarations, these
/// aliases are merged by resource order, so a later layer must overwrite a
/// conflicting alias before it is resolved by `font-variant-alternates`.
/// <https://www.w3.org/TR/css-fonts-4/#font-feature-values>
#[derive(Debug)]
pub(in crate::css) struct FontFeatureValuesRule {
    pub(in crate::css) prelude: String,
    pub(in crate::css) block: String,
    pub(in crate::css) layer: Option<LayerName>,
    pub(in crate::css) order: usize,
}

/// Parse named palette definitions from CSS Fonts Level 4.
///
/// The rules are deliberately collected alongside `@font-feature-values`:
/// both are stylesheet-scoped resources consumed later by text painting rather
/// than selector-matched declarations.
/// <https://www.w3.org/TR/css-fonts-4/#font-palette-values>
pub(in crate::css) fn parse_font_palette_rule(
    prelude: &str,
    block: &str,
) -> Option<(String, FontPaletteDefinition)> {
    let name = prelude.trim();
    if !name.starts_with("--") || name.len() == 2 {
        return None;
    }
    let declarations = parse_declarations(block);
    let families = declarations
        .get("font-family")
        .map(|value| parse_font_family_names(value))
        .unwrap_or_default();
    let base = declarations
        .get("base-palette")
        .and_then(|value| parse_base_palette(value))
        .unwrap_or(FontPalette::Normal);
    let overrides = declarations
        .get("override-colors")
        .map(|value| parse_palette_overrides(value))
        .unwrap_or_default();
    Some((
        name.to_string(),
        FontPaletteDefinition {
            families,
            base,
            overrides,
        },
    ))
}

fn parse_base_palette(value: &str) -> Option<FontPalette> {
    let value = trim_css_value(value);
    parse_font_palette(value).or_else(|| value.parse::<u16>().ok().map(FontPalette::Index))
}

fn parse_palette_overrides(value: &str) -> HashMap<u16, CssColor> {
    value
        .split(',')
        .filter_map(|entry| {
            let values = split_css_component_values(entry);
            let [index, color] = values.as_slice() else {
                return None;
            };
            let index = parse_palette_override_index(index)?;
            let color = parse_color(color)?;
            Some((index, color))
        })
        .collect()
}

/// Resolve the integer grammar used by `override-colors`. CSS Values permits
/// math functions here; retain the length comparison inside `sign()` rather
/// than treating a calculated token as an invalid palette entry.
fn parse_palette_override_index(value: &str) -> Option<u16> {
    if let Ok(index) = value.trim().parse::<u16>() {
        return Some(index);
    }
    let inner = value
        .trim()
        .strip_prefix("calc(")?
        .strip_suffix(')')?
        .trim();
    if let Ok(index) = inner.parse::<u16>() {
        return Some(index);
    }
    let (base, sign_argument) = inner.split_once("+ sign(")?;
    let sign_argument = sign_argument.strip_suffix(')')?;
    let base = base.trim().parse::<i32>().ok()?;
    let (left, right) = sign_argument.split_once('-')?;
    let basis = FontRelativeLengthBasis::new(layout_pt(ROOT_FONT_SIZE_PT), layout_pt(0.0));
    let mut left = parse_computed_length_percentage(left.trim(), ROOT_FONT_SIZE_PT)?;
    left.resolve_font_relative_lengths(basis);
    let left = left.length_points();
    let mut right = parse_computed_length_percentage(right.trim(), ROOT_FONT_SIZE_PT)?;
    right.resolve_font_relative_lengths(basis);
    let right = right.length_points();
    let sign = if left > right {
        1
    } else if left < right {
        -1
    } else {
        0
    };
    u16::try_from(base.checked_add(sign)?).ok()
}

fn parse_font_feature_values_block(values: &mut FontFeatureValues, family: &str, source: &str) {
    let mut input = ParserInput::new(source);
    let mut input = Parser::new(&mut input);
    let mut parser = FontFeatureValuesBlockParser { values, family };
    for _ in RuleBodyParser::new(&mut input, &mut parser) {}
}

struct FontFeatureValuesBlockParser<'a> {
    values: &'a mut FontFeatureValues,
    family: &'a str,
}

impl<'i> AtRuleParser<'i> for FontFeatureValuesBlockParser<'_> {
    type Prelude = FontFeatureValuesBlock;
    type AtRule = ();
    type Error = ();

    fn parse_prelude<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::Prelude, cssparser::ParseError<'i, Self::Error>> {
        let Some(block) = font_feature_values_block_kind(&name) else {
            return Err(input.new_custom_error(()));
        };
        input.expect_exhausted()?;
        Ok(block)
    }

    fn parse_block<'t>(
        &mut self,
        block: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i, 't>,
    ) -> Result<Self::AtRule, cssparser::ParseError<'i, Self::Error>> {
        let mut descriptors = FontFeatureDescriptorParser {
            values: self.values,
            family: self.family,
            block,
        };
        for _ in RuleBodyParser::new(input, &mut descriptors) {}
        Ok(())
    }
}

impl<'i> DeclarationParser<'i> for FontFeatureValuesBlockParser<'_> {
    type Declaration = ();
    type Error = ();
}

impl<'i> QualifiedRuleParser<'i> for FontFeatureValuesBlockParser<'_> {
    type Prelude = ();
    type QualifiedRule = ();
    type Error = ();
}

impl<'i> RuleBodyItemParser<'i, (), ()> for FontFeatureValuesBlockParser<'_> {
    fn parse_declarations(&self) -> bool {
        false
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

impl<'i> DeclarationParser<'i> for FontFeatureDescriptorParser<'_> {
    type Declaration = ();
    type Error = ();

    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i, 't>,
        _declaration_start: &ParserState,
    ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
        if !font_feature_value_name_is_valid(&name) {
            return Err(input.new_custom_error(()));
        }
        let mut numbers = Vec::new();
        while !input.is_exhausted() {
            let number = input.expect_integer()?;
            let number = u16::try_from(number).map_err(|_| input.new_custom_error(()))?;
            numbers.push(number);
        }
        let value = parse_font_feature_value(self.block, &numbers)
            .ok_or_else(|| input.new_custom_error(()))?;
        self.values
            .insert(self.family.to_string(), self.block, name.to_string(), value);
        Ok(())
    }
}

struct FontFeatureDescriptorParser<'a> {
    values: &'a mut FontFeatureValues,
    family: &'a str,
    block: FontFeatureValuesBlock,
}

impl<'i> AtRuleParser<'i> for FontFeatureDescriptorParser<'_> {
    type Prelude = ();
    type AtRule = ();
    type Error = ();
}

impl<'i> QualifiedRuleParser<'i> for FontFeatureDescriptorParser<'_> {
    type Prelude = ();
    type QualifiedRule = ();
    type Error = ();
}

impl<'i> RuleBodyItemParser<'i, (), ()> for FontFeatureDescriptorParser<'_> {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

fn font_feature_values_block_kind(name: &str) -> Option<FontFeatureValuesBlock> {
    match name.to_ascii_lowercase().as_str() {
        "stylistic" => Some(FontFeatureValuesBlock::Stylistic),
        "styleset" => Some(FontFeatureValuesBlock::Styleset),
        "character-variant" => Some(FontFeatureValuesBlock::CharacterVariant),
        "swash" => Some(FontFeatureValuesBlock::Swash),
        "ornaments" => Some(FontFeatureValuesBlock::Ornaments),
        "annotation" => Some(FontFeatureValuesBlock::Annotation),
        _ => None,
    }
}

fn font_feature_value_name_is_valid(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '-' || first.is_ascii_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '-' || character.is_ascii_alphanumeric()
        })
}

fn parse_font_feature_value(
    block: FontFeatureValuesBlock,
    numbers: &[u16],
) -> Option<FontFeatureValue> {
    match block {
        FontFeatureValuesBlock::Stylistic
        | FontFeatureValuesBlock::Swash
        | FontFeatureValuesBlock::Ornaments
        | FontFeatureValuesBlock::Annotation => {
            let [feature_index] = numbers else {
                return None;
            };
            (1..=99)
                .contains(feature_index)
                .then_some(FontFeatureValue {
                    feature_index: *feature_index,
                    selector: None,
                })
        }
        FontFeatureValuesBlock::Styleset => {
            let [feature_index] = numbers else {
                return None;
            };
            (1..=20)
                .contains(feature_index)
                .then_some(FontFeatureValue {
                    feature_index: *feature_index,
                    selector: None,
                })
        }
        FontFeatureValuesBlock::CharacterVariant => match numbers {
            [feature_index] if (1..=99).contains(feature_index) => Some(FontFeatureValue {
                feature_index: *feature_index,
                selector: None,
            }),
            [feature_index, selector]
                if (1..=99).contains(feature_index) && (1..=99).contains(selector) =>
            {
                Some(FontFeatureValue {
                    feature_index: *feature_index,
                    selector: Some(*selector),
                })
            }
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_feature_values_nested_rules_and_descriptors_are_tokenized() {
        let mut values = FontFeatureValues::default();
        parse_font_feature_values_block(
            &mut values,
            "Feature Family",
            r#"
                /* @styleset { comment: 1; } */
                @unknown { ignored: 1; }
                @st\79 leset {
                    \61 lias: 4;
                    invalid: 2 calc(3);
                    later: 5;
                    quoted: "semicolon; colon:";
                }
            "#,
        );

        assert_eq!(
            values
                .get("Feature Family", FontFeatureValuesBlock::Styleset, "alias")
                .map(|value| value.feature_index),
            Some(4)
        );
        assert_eq!(
            values
                .get("Feature Family", FontFeatureValuesBlock::Styleset, "later")
                .map(|value| value.feature_index),
            Some(5)
        );
        assert!(
            values
                .get(
                    "Feature Family",
                    FontFeatureValuesBlock::Styleset,
                    "invalid"
                )
                .is_none()
        );
        assert!(
            values
                .get("Feature Family", FontFeatureValuesBlock::Styleset, "quoted")
                .is_none()
        );
    }

    #[test]
    fn font_feature_values_recovers_eof_closed_feature_blocks() {
        let mut values = FontFeatureValues::default();
        parse_font_feature_values_block(&mut values, "Feature Family", "@styleset { later: 5");

        assert_eq!(
            values
                .get("Feature Family", FontFeatureValuesBlock::Styleset, "later")
                .map(|value| value.feature_index),
            Some(5)
        );
    }
}
