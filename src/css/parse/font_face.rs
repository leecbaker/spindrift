use super::*;
use crate::css::values::{
    parse_font_feature_settings, parse_font_variant, parse_font_variant_alternates,
    parse_font_variant_caps, parse_font_variant_east_asian, parse_font_variant_ligatures,
    parse_font_variant_numeric, parse_font_variant_position,
};
use crate::css::{
    FontFeatureSettings, FontFeatureValue, FontFeatureValues, FontFeatureValuesBlock,
    FontVariantAlternates, FontVariantCaps, FontVariantEastAsian, FontVariantLigatures,
    FontVariantNumeric, FontVariantPosition,
};

pub(super) fn parse_font_faces(css: &Css) -> Vec<CssFontFace> {
    let mut faces = Vec::new();
    let mut rest = css.source();
    while let Some(font_face_start) = find_ascii_case_insensitive(rest, "@font-face") {
        let font_face_rest = &rest[font_face_start + "@font-face".len()..];
        let Some(open_offset) = font_face_rest.find('{') else {
            break;
        };
        let open = font_face_start + "@font-face".len() + open_offset;
        let Some(close) = find_matching_brace(rest, open) else {
            break;
        };
        let declarations = parse_declarations(&rest[open + 1..close]);
        if let Some(family) = declarations
            .get("font-family")
            .and_then(|value| parse_font_family_names(value).into_iter().next())
        {
            let sources = declarations
                .get("src")
                .map(|value| parse_font_face_sources(value, css.base_url(), css.root_url()))
                .unwrap_or_default();
            if !sources.is_empty() {
                faces.push(CssFontFace {
                    family,
                    sources,
                    unicode_range: declarations
                        .get("unicode-range")
                        .and_then(|value| parse_unicode_range(value)),
                    weight: declarations
                        .get("font-weight")
                        .and_then(|value| parse_font_weight(value, FontWeight::NORMAL))
                        .unwrap_or(FontWeight::NORMAL),
                    style: declarations
                        .get("font-style")
                        .and_then(|value| parse_font_style(value))
                        .unwrap_or(FontStyle::Normal),
                    width: declarations
                        .get("font-width")
                        .or_else(|| declarations.get("font-stretch"))
                        .and_then(|value| parse_font_width(value))
                        .unwrap_or(FontWidth::NORMAL),
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
                });
                if let Some(face) = faces.last_mut() {
                    apply_font_face_variant_descriptors(face, &declarations);
                }
            }
        }
        rest = &rest[close + 1..];
    }
    faces
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
    base_url: Option<&Path>,
    root_url: Option<&Path>,
) -> Vec<FontFaceSource> {
    let mut sources = Vec::new();
    let mut input = ParserInput::new(trim_css_value(value));
    let mut parser = Parser::new(&mut input);
    while !parser.is_exhausted() {
        if let Ok(value) = parser.try_parse(|input| input.expect_url()) {
            sources.push(FontFaceSource::Url {
                value: value.to_string(),
                base_url: base_url.map(Path::to_path_buf),
                root_url: root_url.map(Path::to_path_buf),
            });
        } else if parser.next_including_whitespace_and_comments().is_err() {
            break;
        }
    }
    sources
}

pub(super) fn parse_font_feature_values(css: &Css) -> FontFeatureValues {
    let mut values = FontFeatureValues::default();
    let mut rest = css.source();
    while let Some(rule_start) = find_ascii_case_insensitive(rest, "@font-feature-values") {
        let after_name = &rest[rule_start + "@font-feature-values".len()..];
        let Some(open_offset) = after_name.find('{') else {
            break;
        };
        let open = rule_start + "@font-feature-values".len() + open_offset;
        let Some(close) = find_matching_brace(rest, open) else {
            break;
        };
        let prelude = &after_name[..open_offset];
        let families = parse_font_family_names(prelude);
        let block = &rest[open + 1..close];
        for family in families {
            parse_font_feature_values_block(&mut values, &family, block);
        }
        rest = &rest[close + 1..];
    }
    values
}

fn parse_font_feature_values_block(values: &mut FontFeatureValues, family: &str, mut source: &str) {
    while let Some(at_index) = source.find('@') {
        let after_at = &source[at_index + 1..];
        let name_end = after_at
            .char_indices()
            .find_map(|(index, character)| {
                (!matches!(character, '-' | '_' | 'a'..='z' | 'A'..='Z' | '0'..='9'))
                    .then_some(index)
            })
            .unwrap_or(after_at.len());
        let name = &after_at[..name_end];
        let Some(block) = font_feature_values_block_kind(name) else {
            source = &after_at[name_end..];
            continue;
        };
        let after_name = &after_at[name_end..];
        let Some(open_offset) = after_name.find('{') else {
            break;
        };
        let Some(close) = find_matching_brace(after_name, open_offset) else {
            break;
        };
        parse_font_feature_values_declarations(
            values,
            family,
            block,
            &after_name[open_offset + 1..close],
        );
        source = &after_name[close + 1..];
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

fn parse_font_feature_values_declarations(
    values: &mut FontFeatureValues,
    family: &str,
    block: FontFeatureValuesBlock,
    source: &str,
) {
    for declaration in source.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || !font_feature_value_name_is_valid(&name) {
            continue;
        }
        let mut numbers = Vec::new();
        let mut valid_numbers = true;
        for token in split_css_component_values(value) {
            if let Ok(number) = token.parse::<u16>() {
                numbers.push(number);
            } else {
                valid_numbers = false;
                break;
            }
        }
        if !valid_numbers {
            continue;
        }
        let Some(parsed) = parse_font_feature_value(block, &numbers) else {
            continue;
        };
        values.insert(family.to_string(), block, name, parsed);
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
