use super::*;

pub(super) fn fontique_weight(weight: FontWeight) -> FontiqueFontWeight {
    FontiqueFontWeight::new(weight.0 as f32)
}

pub(super) fn fontique_style(style: FontStyle) -> FontiqueFontStyle {
    match style {
        FontStyle::Normal => FontiqueFontStyle::Normal,
        FontStyle::Italic => FontiqueFontStyle::Italic,
        FontStyle::Oblique => FontiqueFontStyle::Oblique(Some(14.0)),
    }
}

pub(super) fn fontique_width(width: FontWidth) -> FontiqueFontWidth {
    FontiqueFontWidth::from_ratio(width.0 as f32 / 1000.0)
}

pub(super) fn fontique_attributes(
    weight: FontWeight,
    style: FontStyle,
    width: FontWidth,
) -> FontiqueAttributes {
    FontiqueAttributes::new(
        fontique_width(width),
        fontique_style(style),
        fontique_weight(weight),
    )
}

pub(super) fn parley_font_family_source(family: &FontFamily) -> String {
    match family {
        FontFamily::SansSerif => "sans-serif".to_string(),
        FontFamily::Serif => "serif".to_string(),
        FontFamily::Monospace => "monospace".to_string(),
        FontFamily::Names(names) => names
            .iter()
            .map(|name| {
                let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

pub(super) fn push_parley_default_style(
    builder: &mut parley::RangedBuilder<'_, [u8; 4]>,
    style: &ComputedStyle,
) {
    push_parley_default_style_with_font_size(builder, style, style.font_size);
}

pub(super) fn push_parley_default_style_with_font_size(
    builder: &mut parley::RangedBuilder<'_, [u8; 4]>,
    style: &ComputedStyle,
    font_size: f32,
) {
    let font_family_source = parley_font_family_source(&style.font_family);
    builder.push_default(StyleProperty::FontFamily(ParleyFontFamily::from(
        font_family_source.as_str(),
    )));
    builder.push_default(StyleProperty::FontSize(font_size));
    builder.push_default(StyleProperty::LineHeight(ParleyLineHeight::Absolute(
        style.line_height,
    )));
    builder.push_default(StyleProperty::FontWeight(ParleyFontWeight::new(
        style.font_weight.0 as f32,
    )));
    builder.push_default(StyleProperty::FontStyle(parley_font_style(
        style.font_style,
    )));
    builder.push_default(StyleProperty::FontWidth(ParleyFontWidth::from_ratio(
        style.font_width.0 as f32 / 1000.0,
    )));
    builder.push_default(StyleProperty::WordBreak(parley_word_break(
        style.word_break,
    )));
    builder.push_default(StyleProperty::OverflowWrap(parley_overflow_wrap(
        style.overflow_wrap,
    )));
    builder.push_default(StyleProperty::TextWrapMode(parley_text_wrap_mode(
        style.white_space,
    )));
    builder.push_default(StyleProperty::WordSpacing(style.used_word_spacing()));
    builder.push_default(StyleProperty::Locale(parley_language(style)));
}

pub(super) fn push_parley_text_spacing_default_with_context(
    builder: &mut parley::RangedBuilder<'_, [u8; 4]>,
    text: &str,
    style: &ComputedStyle,
    context: Option<&FontFeatureContext>,
) {
    let used_letter_spacing = used_letter_spacing_for_text(text, style.used_letter_spacing());
    builder.push_default(StyleProperty::LetterSpacing(used_letter_spacing));
    builder.push_default(StyleProperty::FontFeatures(parley_font_features(
        style,
        used_letter_spacing,
        context,
    )));
}

pub(super) fn push_parley_text_spacing_range_with_context(
    builder: &mut parley::RangedBuilder<'_, [u8; 4]>,
    text: &str,
    style: &ComputedStyle,
    range: Range<usize>,
    context: Option<&FontFeatureContext>,
) {
    let used_letter_spacing = used_letter_spacing_for_text(text, style.used_letter_spacing());
    builder.push(
        StyleProperty::LetterSpacing(used_letter_spacing),
        range.clone(),
    );
    builder.push(
        StyleProperty::FontFeatures(parley_font_features(style, used_letter_spacing, context)),
        range,
    );
}

pub(super) fn push_parley_style_range(
    builder: &mut parley::RangedBuilder<'_, [u8; 4]>,
    style: &ComputedStyle,
    range: Range<usize>,
) {
    push_parley_style_range_with_font_size(builder, style, range, style.font_size);
}

pub(super) fn push_parley_style_range_with_font_size(
    builder: &mut parley::RangedBuilder<'_, [u8; 4]>,
    style: &ComputedStyle,
    range: Range<usize>,
    font_size: f32,
) {
    let font_family_source = parley_font_family_source(&style.font_family);
    builder.push(
        StyleProperty::FontFamily(ParleyFontFamily::from(font_family_source.as_str())),
        range.clone(),
    );
    builder.push(StyleProperty::FontSize(font_size), range.clone());
    builder.push(
        StyleProperty::LineHeight(ParleyLineHeight::Absolute(style.line_height)),
        range.clone(),
    );
    builder.push(
        StyleProperty::FontWeight(ParleyFontWeight::new(style.font_weight.0 as f32)),
        range.clone(),
    );
    builder.push(
        StyleProperty::FontStyle(parley_font_style(style.font_style)),
        range.clone(),
    );
    builder.push(
        StyleProperty::FontWidth(ParleyFontWidth::from_ratio(
            style.font_width.0 as f32 / 1000.0,
        )),
        range.clone(),
    );
    builder.push(
        StyleProperty::WordBreak(parley_word_break(style.word_break)),
        range.clone(),
    );
    builder.push(
        StyleProperty::OverflowWrap(parley_overflow_wrap(style.overflow_wrap)),
        range.clone(),
    );
    builder.push(
        StyleProperty::TextWrapMode(parley_text_wrap_mode(style.white_space)),
        range.clone(),
    );
    builder.push(
        StyleProperty::WordSpacing(style.used_word_spacing()),
        range.clone(),
    );
    builder.push(StyleProperty::Locale(parley_language(style)), range);
}

/// Build the OpenType feature list for one CSS shaping context.
///
/// CSS Fonts defines feature precedence as variant and kerning features first,
/// then non-font-feature CSS effects such as nonzero tracking, and finally
/// low-level `font-feature-settings`:
/// <https://www.w3.org/TR/css-fonts-4/#feature-precedence>.
fn parley_font_features(
    style: &ComputedStyle,
    used_letter_spacing: f32,
    context: Option<&FontFeatureContext>,
) -> ParleyFontFeatures<'static> {
    let mut features = Vec::<ParleyFontFeature>::new();
    if let Some(defaults) = context.and_then(|context| context.face_defaults.as_ref()) {
        let resolver = context.and_then(FontFeatureContext::resolver);
        push_font_variant_ligature_features(&mut features, defaults.font_variant_ligatures);
        push_font_variant_position_features(&mut features, defaults.font_variant_position);
        push_font_variant_caps_features(&mut features, defaults.font_variant_caps);
        push_font_variant_numeric_features(&mut features, &defaults.font_variant_numeric);
        push_font_variant_alternates_features(
            &mut features,
            &defaults.font_variant_alternates,
            resolver.as_ref(),
        );
        push_font_variant_east_asian_features(&mut features, &defaults.font_variant_east_asian);
        for setting in &defaults.font_feature_settings.0 {
            push_parley_font_feature(&mut features, setting.tag, setting.value);
        }
    }
    let resolver = context.and_then(FontFeatureContext::resolver);
    push_font_kerning_features(&mut features, style);
    push_font_variant_ligature_features(&mut features, style.font_variant_ligatures);
    push_font_variant_position_features(&mut features, style.font_variant_position);
    push_font_variant_caps_features(&mut features, style.font_variant_caps);
    push_font_variant_numeric_features(&mut features, &style.font_variant_numeric);
    push_font_variant_alternates_features(
        &mut features,
        &style.font_variant_alternates,
        resolver.as_ref(),
    );
    push_font_variant_east_asian_features(&mut features, &style.font_variant_east_asian);
    if used_letter_spacing != 0.0 {
        push_parley_font_feature(&mut features, *b"liga", 0);
        push_parley_font_feature(&mut features, *b"clig", 0);
    }
    for setting in &style.font_feature_settings.0 {
        push_parley_font_feature(&mut features, setting.tag, setting.value);
    }
    if features.is_empty() {
        ParleyFontFeatures::empty()
    } else {
        features.sort_by_key(|feature| feature.tag);
        ParleyFontFeatures::List(Cow::Owned(features))
    }
}

#[derive(Debug, Clone)]
pub(super) struct FontFeatureContext {
    pub(super) family: Option<String>,
    pub(super) face_defaults: Option<FontFaceFeatureDefaults>,
    pub(super) font_feature_values: FontFeatureValues,
}

impl FontFeatureContext {
    fn resolver(&self) -> Option<FontFeatureValueResolver<'_>> {
        Some(FontFeatureValueResolver {
            family: self.family.as_deref()?,
            values: &self.font_feature_values,
        })
    }
}

fn push_font_kerning_features(features: &mut Vec<ParleyFontFeature>, style: &ComputedStyle) {
    let value = match style.font_kerning {
        FontKerning::Auto => return,
        FontKerning::Normal => 1,
        FontKerning::None => 0,
    };
    let tag = match style.writing_mode {
        WritingMode::HorizontalTb => *b"kern",
        WritingMode::VerticalRl | WritingMode::VerticalLr => *b"vkrn",
    };
    push_parley_font_feature(features, tag, value);
}

fn push_font_variant_ligature_features(
    features: &mut Vec<ParleyFontFeature>,
    ligatures: FontVariantLigatures,
) {
    match ligatures {
        FontVariantLigatures::Normal => {}
        FontVariantLigatures::None => {
            for tag in [*b"liga", *b"clig", *b"dlig", *b"hlig", *b"calt"] {
                push_parley_font_feature(features, tag, 0);
            }
        }
        FontVariantLigatures::Values {
            common,
            discretionary,
            historical,
            contextual,
        } => {
            if let Some(enabled) = common {
                let value = u16::from(enabled);
                push_parley_font_feature(features, *b"liga", value);
                push_parley_font_feature(features, *b"clig", value);
            }
            if let Some(enabled) = discretionary {
                push_parley_font_feature(features, *b"dlig", u16::from(enabled));
            }
            if let Some(enabled) = historical {
                push_parley_font_feature(features, *b"hlig", u16::from(enabled));
            }
            if let Some(enabled) = contextual {
                push_parley_font_feature(features, *b"calt", u16::from(enabled));
            }
        }
    }
}

fn push_font_variant_position_features(
    features: &mut Vec<ParleyFontFeature>,
    position: FontVariantPosition,
) {
    match position {
        FontVariantPosition::Normal => {}
        FontVariantPosition::Sub => push_parley_font_feature(features, *b"subs", 1),
        FontVariantPosition::Super => push_parley_font_feature(features, *b"sups", 1),
    }
}

fn push_font_variant_caps_features(features: &mut Vec<ParleyFontFeature>, caps: FontVariantCaps) {
    match caps {
        FontVariantCaps::Normal => {}
        FontVariantCaps::SmallCaps => push_parley_font_feature(features, *b"smcp", 1),
        FontVariantCaps::AllSmallCaps => {
            push_parley_font_feature(features, *b"c2sc", 1);
            push_parley_font_feature(features, *b"smcp", 1);
        }
        FontVariantCaps::PetiteCaps => push_parley_font_feature(features, *b"pcap", 1),
        FontVariantCaps::AllPetiteCaps => {
            push_parley_font_feature(features, *b"c2pc", 1);
            push_parley_font_feature(features, *b"pcap", 1);
        }
        FontVariantCaps::Unicase => push_parley_font_feature(features, *b"unic", 1),
        FontVariantCaps::TitlingCaps => push_parley_font_feature(features, *b"titl", 1),
    }
}

fn push_font_variant_numeric_features(
    features: &mut Vec<ParleyFontFeature>,
    numeric: &FontVariantNumeric,
) {
    let FontVariantNumeric::Values(values) = numeric else {
        return;
    };
    for value in values {
        let tag = match value {
            FontVariantNumericValue::LiningNums => *b"lnum",
            FontVariantNumericValue::OldstyleNums => *b"onum",
            FontVariantNumericValue::ProportionalNums => *b"pnum",
            FontVariantNumericValue::TabularNums => *b"tnum",
            FontVariantNumericValue::DiagonalFractions => *b"frac",
            FontVariantNumericValue::StackedFractions => *b"afrc",
            FontVariantNumericValue::Ordinal => *b"ordn",
            FontVariantNumericValue::SlashedZero => *b"zero",
        };
        push_parley_font_feature(features, tag, 1);
    }
}

fn push_font_variant_alternates_features(
    features: &mut Vec<ParleyFontFeature>,
    alternates: &FontVariantAlternates,
    resolver: Option<&FontFeatureValueResolver<'_>>,
) {
    match alternates {
        FontVariantAlternates::Normal => {}
        FontVariantAlternates::Values {
            historical_forms,
            stylistic,
            styleset,
            character_variant,
            swash,
            ornaments,
            annotation,
        } => {
            if *historical_forms {
                push_parley_font_feature(features, *b"hist", 1);
            }
            let Some(resolver) = resolver else {
                return;
            };
            for name in stylistic {
                if resolver
                    .get(FontFeatureValuesBlock::Stylistic, name)
                    .is_some()
                {
                    push_parley_font_feature(features, *b"salt", 1);
                }
            }
            for name in styleset {
                if let Some(value) = resolver.get(FontFeatureValuesBlock::Styleset, name) {
                    push_numbered_feature(features, b"ss", value.feature_index, 20, 1);
                }
            }
            for name in character_variant {
                if let Some(value) = resolver.get(FontFeatureValuesBlock::CharacterVariant, name) {
                    push_numbered_feature(
                        features,
                        b"cv",
                        value.feature_index,
                        99,
                        value.selector.unwrap_or(1),
                    );
                }
            }
            for name in swash {
                if resolver.get(FontFeatureValuesBlock::Swash, name).is_some() {
                    push_parley_font_feature(features, *b"swsh", 1);
                    push_parley_font_feature(features, *b"cswh", 1);
                }
            }
            for name in ornaments {
                if resolver
                    .get(FontFeatureValuesBlock::Ornaments, name)
                    .is_some()
                {
                    push_parley_font_feature(features, *b"ornm", 1);
                }
            }
            for name in annotation {
                if resolver
                    .get(FontFeatureValuesBlock::Annotation, name)
                    .is_some()
                {
                    push_parley_font_feature(features, *b"nalt", 1);
                }
            }
        }
    }
}

struct FontFeatureValueResolver<'a> {
    family: &'a str,
    values: &'a FontFeatureValues,
}

impl FontFeatureValueResolver<'_> {
    fn get(&self, block: FontFeatureValuesBlock, name: &str) -> Option<&FontFeatureValue> {
        self.values.get(self.family, block, name)
    }
}

fn push_numbered_feature(
    features: &mut Vec<ParleyFontFeature>,
    prefix: &[u8; 2],
    number: u16,
    max: u16,
    value: u16,
) {
    if !(1..=max).contains(&number) {
        return;
    }
    let tag = [
        prefix[0],
        prefix[1],
        b'0' + (number / 10) as u8,
        b'0' + (number % 10) as u8,
    ];
    push_parley_font_feature(features, tag, value);
}

fn push_font_variant_east_asian_features(
    features: &mut Vec<ParleyFontFeature>,
    east_asian: &FontVariantEastAsian,
) {
    let FontVariantEastAsian::Values(values) = east_asian else {
        return;
    };
    for value in values {
        let tag = match value {
            FontVariantEastAsianValue::Jis78 => *b"jp78",
            FontVariantEastAsianValue::Jis83 => *b"jp83",
            FontVariantEastAsianValue::Jis90 => *b"jp90",
            FontVariantEastAsianValue::Jis04 => *b"jp04",
            FontVariantEastAsianValue::Simplified => *b"smpl",
            FontVariantEastAsianValue::Traditional => *b"trad",
            FontVariantEastAsianValue::FullWidth => *b"fwid",
            FontVariantEastAsianValue::ProportionalWidth => *b"pwid",
            FontVariantEastAsianValue::Ruby => *b"ruby",
        };
        push_parley_font_feature(features, tag, 1);
    }
}

fn push_parley_font_feature(features: &mut Vec<ParleyFontFeature>, tag: [u8; 4], value: u16) {
    let tag = ParleyTag::from_bytes(tag);
    if let Some(existing) = features.iter_mut().find(|feature| feature.tag == tag) {
        existing.value = value;
    } else {
        features.push(ParleyFontFeature::new(tag, value));
    }
}

fn parley_language(style: &ComputedStyle) -> Option<ParleyLanguage> {
    style.language.as_deref().and_then(|language| {
        let language = language.replace('_', "-");
        language.parse().ok()
    })
}

pub(super) fn parley_word_break(word_break: CssWordBreak) -> ParleyWordBreak {
    match word_break {
        CssWordBreak::Normal => ParleyWordBreak::Normal,
        CssWordBreak::BreakAll => ParleyWordBreak::BreakAll,
        CssWordBreak::KeepAll => ParleyWordBreak::KeepAll,
    }
}

pub(super) fn parley_overflow_wrap(overflow_wrap: CssOverflowWrap) -> ParleyOverflowWrap {
    match overflow_wrap {
        CssOverflowWrap::Normal => ParleyOverflowWrap::Normal,
        CssOverflowWrap::Anywhere => ParleyOverflowWrap::Anywhere,
        CssOverflowWrap::BreakWord => ParleyOverflowWrap::BreakWord,
    }
}

pub(super) fn parley_text_wrap_mode(white_space: crate::css::WhiteSpace) -> ParleyTextWrapMode {
    if white_space.allows_soft_wrap() {
        ParleyTextWrapMode::Wrap
    } else {
        ParleyTextWrapMode::NoWrap
    }
}

pub(super) fn parley_font_style(style: FontStyle) -> ParleyFontStyle {
    match style {
        FontStyle::Normal => ParleyFontStyle::Normal,
        FontStyle::Italic => ParleyFontStyle::Italic,
        FontStyle::Oblique => ParleyFontStyle::Oblique(Some(14.0)),
    }
}

pub(super) fn family_query(name: &str) -> FontiqueQueryFamily<'_> {
    match name.trim().to_ascii_lowercase().as_str() {
        "serif" => FontiqueQueryFamily::Generic(FontiqueGenericFamily::Serif),
        "sans-serif" | "sans serif" => {
            FontiqueQueryFamily::Generic(FontiqueGenericFamily::SansSerif)
        }
        "monospace" => FontiqueQueryFamily::Generic(FontiqueGenericFamily::Monospace),
        "cursive" => FontiqueQueryFamily::Generic(FontiqueGenericFamily::Cursive),
        "fantasy" => FontiqueQueryFamily::Generic(FontiqueGenericFamily::Fantasy),
        _ => FontiqueQueryFamily::Named(name),
    }
}

pub(super) fn generic_query_families(
    family: &FontFamily,
    weight: FontWeight,
) -> Option<Vec<FontiqueQueryFamily<'static>>> {
    match family {
        FontFamily::SansSerif if weight.0 >= FontWeight::BOLD.0 => Some(vec![
            FontiqueQueryFamily::Named("Arial"),
            FontiqueQueryFamily::Named("Helvetica Neue"),
            FontiqueQueryFamily::Named("Helvetica"),
            FontiqueQueryFamily::Generic(FontiqueGenericFamily::SansSerif),
        ]),
        FontFamily::SansSerif => Some(vec![
            FontiqueQueryFamily::Named("Helvetica"),
            FontiqueQueryFamily::Named("Helvetica Neue"),
            FontiqueQueryFamily::Named("Arial"),
            FontiqueQueryFamily::Generic(FontiqueGenericFamily::SansSerif),
        ]),
        FontFamily::Serif => Some(vec![
            FontiqueQueryFamily::Named("Times New Roman"),
            FontiqueQueryFamily::Named("Times"),
            FontiqueQueryFamily::Named("Georgia"),
            FontiqueQueryFamily::Generic(FontiqueGenericFamily::Serif),
        ]),
        FontFamily::Monospace if weight.0 >= FontWeight::BOLD.0 => Some(vec![
            FontiqueQueryFamily::Named("Courier New"),
            FontiqueQueryFamily::Named("Courier"),
            FontiqueQueryFamily::Named("PT Mono"),
            FontiqueQueryFamily::Named("Andale Mono"),
            FontiqueQueryFamily::Named("Menlo"),
            FontiqueQueryFamily::Named(".SF NS Mono"),
            FontiqueQueryFamily::Generic(FontiqueGenericFamily::Monospace),
        ]),
        FontFamily::Monospace => Some(vec![
            FontiqueQueryFamily::Named("Menlo"),
            FontiqueQueryFamily::Named("Andale Mono"),
            FontiqueQueryFamily::Named("PT Mono"),
            FontiqueQueryFamily::Named(".SF NS Mono"),
            FontiqueQueryFamily::Named("Courier New"),
            FontiqueQueryFamily::Named("Courier"),
            FontiqueQueryFamily::Generic(FontiqueGenericFamily::Monospace),
        ]),
        FontFamily::Names(_) => None,
    }
}

pub(super) fn fallback_family_score(family_name: &str) -> (u32, String) {
    // CSS Fonts Level 4 fallback uses available fonts after failing the
    // requested family list. Hidden platform UI fonts are implementation
    // details and should not outrank normal document fonts just because their
    // PostScript names sort earlier.
    let normalized = family_name.to_ascii_lowercase();
    let hidden_system_font_score = if family_name.starts_with('.') {
        50_000
    } else if normalized.contains("arial unicode") {
        0
    } else {
        10_000
    };
    (hidden_system_font_score, normalized)
}

impl FontRequest {
    pub(super) fn from_family(
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Self {
        let family_list = match family {
            FontFamily::SansSerif => {
                vec![FontFamilyRequest::Generic(GenericFontRequest::SansSerif)]
            }
            FontFamily::Serif => vec![FontFamilyRequest::Generic(GenericFontRequest::Serif)],
            FontFamily::Monospace => {
                vec![FontFamilyRequest::Generic(GenericFontRequest::Monospace)]
            }
            FontFamily::Names(names) => names
                .iter()
                .map(|name| FontFamilyRequest::Named(normalize_family(name)))
                .collect(),
        };
        Self {
            family_list,
            attributes: font_request_attributes(weight, style, width),
        }
    }

    pub(super) fn single_name(
        name: &str,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Self {
        Self {
            family_list: vec![FontFamilyRequest::Named(normalize_family(name))],
            attributes: font_request_attributes(weight, style, width),
        }
    }

    pub(super) fn generic(
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Self {
        Self::from_family(family, weight, style, width)
    }
}

impl FallbackRequest {
    pub(super) fn new(
        character: char,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Self {
        Self {
            character,
            attributes: font_request_attributes(weight, style, width),
        }
    }
}

pub(super) fn font_request_attributes(
    weight: FontWeight,
    style: FontStyle,
    width: FontWidth,
) -> FontRequestAttributes {
    FontRequestAttributes {
        weight: weight.0,
        style: match style {
            FontStyle::Normal => 0,
            FontStyle::Italic => 1,
            FontStyle::Oblique => 2,
        },
        width: width.0,
    }
}

pub(super) fn font_has_character(font: &DocumentFont, character: char) -> bool {
    ttf_parser::Face::parse(&font.data, font.face_index)
        .ok()
        .and_then(|face| face.glyph_index(character))
        .is_some()
}

pub(super) fn decode_data_url(value: &str) -> Option<Vec<u8>> {
    let (metadata, payload) = value.split_once(',')?;
    if !metadata.to_ascii_lowercase().contains(";base64") {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .ok()
}

pub(super) fn normalize_family(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase()
}

pub(super) fn standalone_font_program_kind(data: &[u8]) -> Option<FontProgramKind> {
    match data.get(..4) {
        Some(b"\x00\x01\x00\x00") | Some(b"true") | Some(b"typ1") | Some(b"ttcf") => {
            Some(FontProgramKind::TrueType)
        }
        Some(b"OTTO") => Some(FontProgramKind::OpenTypeCff),
        _ => None,
    }
}

pub(super) fn opentype_name(face: &ttf_parser::Face<'_>, name_id: u16) -> Option<String> {
    face.names()
        .into_iter()
        .find(|name| name.name_id == name_id && name.is_unicode())
        .and_then(|name| name.to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            face.names()
                .into_iter()
                .find(|name| name.name_id == name_id)
                .and_then(|name| name.to_string())
                .filter(|name| !name.is_empty())
        })
}

pub(super) fn sanitize_pdf_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '+') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized.trim_matches('-').to_string()
}
