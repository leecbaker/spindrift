use super::*;
use crate::units::SemanticLengthExt;

pub(super) fn fontique_weight(weight: FontWeight) -> FontiqueFontWeight {
    FontiqueFontWeight::new(weight.0 as f32)
}

pub(super) fn fontique_style(style: FontStyle) -> FontiqueFontStyle {
    match style {
        FontStyle::Normal => FontiqueFontStyle::Normal,
        FontStyle::Italic => FontiqueFontStyle::Italic,
        FontStyle::Oblique(angle) => FontiqueFontStyle::Oblique(Some(f32::from_bits(angle))),
    }
}

pub(super) fn fontique_width(width: FontWidth) -> FontiqueFontWidth {
    FontiqueFontWidth::from_ratio(width.0 as f32 / 1000.0)
}

/// Fontique cannot register or query an empty family name, while CSS Fonts
/// explicitly permits the quoted empty string as a font-family name.
/// Keep the sentinel entirely at the font-backend boundary; CSS-facing
/// metadata and matching keys retain the authored empty string.
pub(super) const EMPTY_CSS_FONT_FAMILY_ALIAS: &str = "__quire_empty_css_font_family__";

pub(super) fn fontique_family_name(name: &str) -> &str {
    if name.is_empty() {
        EMPTY_CSS_FONT_FAMILY_ALIAS
    } else {
        name
    }
}

/// Return fixed standard-axis defaults used while registering an `@font-face`.
///
/// A single-value CSS `font-weight` or `font-stretch` descriptor binds a
/// variable font face to that axis value. Fontique's attribute override makes
/// the face selectable at that value, while this companion axis override makes
/// HarfBuzz shape at the matching OpenType instance:
/// <https://www.w3.org/TR/css-fonts-4/#font-prop-desc>.
///
/// Ranged and `auto` descriptors leave the intrinsic axis defaults intact so
/// font matching can select an instance from the requested computed style.
///
/// The `font-variation-settings` descriptor is deliberately excluded: CSS
/// Fonts applies it after font selection as an initial setting on the selected
/// font object, and element-level settings may override it during shaping.
/// <https://www.w3.org/TR/css-fonts-4/#font-feature-variation-resolution>
pub(super) fn fontique_fixed_standard_axis_defaults(
    weight: FontWeight,
    weight_is_variable: bool,
    width: FontWidth,
    width_is_variable: bool,
) -> Vec<(OpenTypeTag, f32)> {
    let mut defaults = Vec::with_capacity(2);
    if !weight_is_variable {
        defaults.push((OpenTypeTag::new(b"wght"), weight.0 as f32));
    }
    if !width_is_variable {
        defaults.push((OpenTypeTag::new(b"wdth"), width.0 as f32 / 10.0));
    }
    defaults
}

/// Translate CSS's registered standard variation axes into Parley's shaping
/// settings. Fontique's face metadata selects the correct `@font-face`, but a
/// face whose fixed descriptor equals the requested CSS value has no synthesis
/// delta for Fontique to forward. Passing the standard coordinates explicitly
/// ensures that both fixed and ranged variable faces shape at their used CSS
/// `font-weight` and `font-stretch` values:
/// <https://www.w3.org/TR/css-fonts-4/#font-prop-desc>.
fn parley_standard_font_variations(style: &ComputedStyle) -> ParleyFontVariations<'static> {
    let mut variations = vec![
        ParleyFontVariation::new(ParleyTag::new(b"wdth"), style.font_width.0 as f32 / 10.0),
        ParleyFontVariation::new(ParleyTag::new(b"wght"), style.font_weight.0 as f32),
    ];
    let slant_angle =
        style
            .font_style
            .oblique_angle()
            .or(matches!(style.font_style, FontStyle::Italic).then_some(14.0));
    if let Some(angle) = slant_angle {
        // CSS positive oblique angles slant glyphs forward, while OpenType's
        // registered `slnt` axis uses the opposite sign.
        variations.push(ParleyFontVariation::new(ParleyTag::new(b"slnt"), -angle));
    }
    // The low-level property takes precedence over the registered CSS axis
    // properties for the same tag.
    for setting in &style.font_variation_settings.0 {
        let tag = ParleyTag::from_bytes(setting.tag);
        let value = f32::from_bits(setting.value);
        if let Some(existing) = variations.iter_mut().find(|axis| axis.tag == tag) {
            *existing = ParleyFontVariation::new(tag, value);
        } else {
            variations.push(ParleyFontVariation::new(tag, value));
        }
    }
    ParleyFontVariations::List(Cow::Owned(variations))
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
        FontFamily::SystemUi => "system-ui".to_string(),
        FontFamily::UiSerif => "ui-serif".to_string(),
        FontFamily::UiSansSerif => "ui-sans-serif".to_string(),
        FontFamily::UiMonospace => "ui-monospace".to_string(),
        FontFamily::UiRounded => "ui-rounded".to_string(),
        FontFamily::List(families) => families
            .iter()
            .map(parley_font_family_source)
            .collect::<Vec<_>>()
            .join(", "),
        FontFamily::Names(names) => names
            .iter()
            .map(|name| {
                let escaped = fontique_family_name(name)
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"");
                format!("\"{escaped}\"")
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

pub(super) fn push_parley_default_style(
    builder: &mut parley::RangedBuilder<'_, FontPalette>,
    style: &ComputedStyle,
    font_family_source: &str,
) {
    push_parley_default_style_with_font_size(builder, style, font_family_source, style.font_size);
}

pub(super) fn push_parley_default_style_with_font_size(
    builder: &mut parley::RangedBuilder<'_, FontPalette>,
    style: &ComputedStyle,
    font_family_source: &str,
    font_size: f32,
) {
    builder.push_default(StyleProperty::FontFamily(ParleyFontFamily::from(
        font_family_source,
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
    builder.push_default(StyleProperty::FontVariations(
        parley_standard_font_variations(style),
    ));
    builder.push_default(StyleProperty::WordBreak(parley_word_break(
        style.word_break,
    )));
    builder.push_default(StyleProperty::OverflowWrap(parley_overflow_wrap(
        style.overflow_wrap,
    )));
    builder.push_default(StyleProperty::TextWrapMode(parley_text_wrap_mode(style)));
    builder.push_default(StyleProperty::WordSpacing(
        style.used_word_spacing().points(),
    ));
    builder.push_default(StyleProperty::Locale(parley_language(style)));
    // Palette choice is a paint-only CSS Fonts property. Carry it in Parley's
    // brush so otherwise identical adjacent style ranges remain distinct
    // glyph runs without affecting font selection or shaping.
    // <https://drafts.csswg.org/css-fonts-4/#font-palette-prop>
    builder.push_default(StyleProperty::Brush(style.font_palette.clone()));
}

pub(super) fn push_parley_text_spacing_default_with_context(
    builder: &mut parley::RangedBuilder<'_, FontPalette>,
    text: &str,
    style: &ComputedStyle,
    context: Option<&FontFeatureContext>,
) {
    let used_letter_spacing =
        used_letter_spacing_for_text(text, style.used_letter_spacing().points());
    let vertical_form_ranges = vertical_form_feature_ranges(text, style);
    let default_feature_policy = FontFeaturePolicy::for_text(text.len(), &vertical_form_ranges);
    builder.push_default(StyleProperty::LetterSpacing(used_letter_spacing));
    builder.push_default(StyleProperty::FontFeatures(parley_font_features(
        style,
        used_letter_spacing,
        context,
        default_feature_policy,
    )));
    if !default_feature_policy.vertical_forms {
        push_vertical_form_feature_ranges(
            builder,
            style,
            0..text.len(),
            used_letter_spacing,
            context,
            &vertical_form_ranges,
        );
    }
}

pub(super) fn push_parley_text_spacing_range_with_context(
    builder: &mut parley::RangedBuilder<'_, FontPalette>,
    text: &str,
    style: &ComputedStyle,
    range: Range<usize>,
    context: Option<&FontFeatureContext>,
) {
    let used_letter_spacing =
        used_letter_spacing_for_text(text, style.used_letter_spacing().points());
    let vertical_form_ranges = vertical_form_feature_ranges(text, style);
    let default_feature_policy = FontFeaturePolicy::for_text(text.len(), &vertical_form_ranges);
    builder.push(
        StyleProperty::LetterSpacing(used_letter_spacing),
        range.clone(),
    );
    builder.push(
        StyleProperty::FontFeatures(parley_font_features(
            style,
            used_letter_spacing,
            context,
            default_feature_policy,
        )),
        range.clone(),
    );
    if !default_feature_policy.vertical_forms {
        push_vertical_form_feature_ranges(
            builder,
            style,
            range,
            used_letter_spacing,
            context,
            &vertical_form_ranges,
        );
    }
}

pub(super) fn push_parley_style_range(
    builder: &mut parley::RangedBuilder<'_, FontPalette>,
    style: &ComputedStyle,
    font_family_source: &str,
    range: Range<usize>,
) {
    push_parley_style_range_with_font_size(
        builder,
        style,
        font_family_source,
        range,
        style.font_size,
    );
}

pub(super) fn push_parley_style_range_with_font_size(
    builder: &mut parley::RangedBuilder<'_, FontPalette>,
    style: &ComputedStyle,
    font_family_source: &str,
    range: Range<usize>,
    font_size: f32,
) {
    builder.push(
        StyleProperty::FontFamily(ParleyFontFamily::from(font_family_source)),
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
        StyleProperty::FontVariations(parley_standard_font_variations(style)),
        range.clone(),
    );
    builder.push(
        StyleProperty::Brush(style.font_palette.clone()),
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
        StyleProperty::TextWrapMode(parley_text_wrap_mode(style)),
        range.clone(),
    );
    builder.push(
        StyleProperty::WordSpacing(style.used_word_spacing().points()),
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
    policy: FontFeaturePolicy,
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
    push_font_kerning_features(&mut features, style.font_kerning, policy.kerning_mode);
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
    push_vertical_form_features(&mut features, policy);
    if used_letter_spacing != 0.0 {
        // Tracking is applied after font-variant controls. It disables every
        // optional ligature or contextual substitution, while the later
        // low-level `font-feature-settings` layer may explicitly re-enable
        // one of them.
        // <https://www.w3.org/TR/css-fonts-4/#feature-precedence>
        for tag in [*b"liga", *b"clig", *b"dlig", *b"hlig", *b"calt"] {
            push_parley_font_feature(&mut features, tag, 0);
        }
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

/// The OpenType kerning feature selected for one shaped range.
///
/// CSS Fonts selects `kern` for horizontal and sideways typography and `vkrn`
/// for upright vertical typography. The inactive feature must be disabled at
/// this boundary because HarfBuzz enables `kern` by default even while shaping
/// an upright vertical run.
/// <https://drafts.csswg.org/css-fonts-4/#font-kerning-prop>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum KerningFeatureMode {
    #[default]
    Horizontal,
    UprightVertical,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FontFeaturePolicy {
    vertical_forms: bool,
    kerning_mode: KerningFeatureMode,
}

impl FontFeaturePolicy {
    const UPRIGHT_VERTICAL: Self = Self {
        vertical_forms: true,
        kerning_mode: KerningFeatureMode::UprightVertical,
    };

    /// Derive the default policy for a text range before range-local upright
    /// overrides are layered on top. A mixed-orientation run defaults to
    /// horizontal shaping and its upright typographic units override both
    /// vertical glyph-form and kerning features together.
    fn for_text(text_len: usize, vertical_form_ranges: &[Range<usize>]) -> Self {
        if vertical_form_ranges
            .first()
            .is_some_and(|range| range.start == 0 && range.end == text_len)
        {
            Self::UPRIGHT_VERTICAL
        } else {
            Self::default()
        }
    }
}

/// Return byte ranges that should shape with OpenType vertical glyph forms.
///
/// CSS Writing Modes orients vertical typographic character units according to
/// `text-orientation`; transformed vertical-orientation units select vertical
/// glyph forms even when their final presentation remains sideways.
/// The shaping feature policy applies `vert`/`vrt2` before PDF placement so
/// glyph selection, measurement, and ToUnicode output remain one artifact:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation> and
/// <https://learn.microsoft.com/en-us/typography/opentype/spec/features_uz#tag-vert>.
fn vertical_form_feature_ranges(text: &str, style: &ComputedStyle) -> Vec<Range<usize>> {
    let TextLayoutPolicy::Vertical(text_orientation) = style.text_layout_policy() else {
        return Vec::new();
    };
    if text.is_empty() {
        return Vec::new();
    }
    match text_orientation {
        TextOrientation::Sideways => Vec::new(),
        TextOrientation::Upright => std::iter::once(0..text.len()).collect(),
        TextOrientation::Mixed => {
            let mut ranges = Vec::new();
            for range in typographic_unit_ranges(text) {
                if typographic_unit_uses_vertical_forms_in_mixed_orientation(&text[range.clone()]) {
                    push_vertical_form_range_with_inherited_characters(text, range, &mut ranges);
                }
            }
            ranges
        }
    }
}

fn push_vertical_form_range_with_inherited_characters(
    text: &str,
    range: Range<usize>,
    ranges: &mut Vec<Range<usize>>,
) {
    let mut start = range.start;
    while let Some((previous_start, character)) = text[..start].char_indices().next_back()
        && character_inherits_vertical_orientation(character)
    {
        start = previous_start;
    }
    let mut end = range.end;
    while let Some((offset, character)) = text[end..].char_indices().next()
        && character_inherits_vertical_orientation(character)
    {
        end += offset + character.len_utf8();
    }
    if let Some(previous) = ranges.last_mut()
        && start <= previous.end
    {
        previous.end = previous.end.max(end);
        return;
    }
    ranges.push(start..end);
}

fn push_vertical_form_feature_ranges(
    builder: &mut parley::RangedBuilder<'_, FontPalette>,
    style: &ComputedStyle,
    base_range: Range<usize>,
    used_letter_spacing: f32,
    context: Option<&FontFeatureContext>,
    vertical_form_ranges: &[Range<usize>],
) {
    for vertical_range in vertical_form_ranges {
        let range =
            (base_range.start + vertical_range.start)..(base_range.start + vertical_range.end);
        builder.push(
            StyleProperty::FontFeatures(parley_font_features(
                style,
                used_letter_spacing,
                context,
                FontFeaturePolicy::UPRIGHT_VERTICAL,
            )),
            range,
        );
    }
}

fn push_vertical_form_features(features: &mut Vec<ParleyFontFeature>, policy: FontFeaturePolicy) {
    if policy.vertical_forms {
        push_parley_font_feature(features, *b"vert", 1);
        push_parley_font_feature(features, *b"vrt2", 1);
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

fn push_font_kerning_features(
    features: &mut Vec<ParleyFontFeature>,
    font_kerning: FontKerning,
    kerning_mode: KerningFeatureMode,
) {
    let active_value = match font_kerning {
        // CSS permits user agents to decide whether `auto` enables kerning.
        // Quire consistently enables the relevant OpenType feature, matching
        // the OpenType recommendation and the existing horizontal behavior.
        FontKerning::Auto | FontKerning::Normal => 1,
        FontKerning::None => 0,
    };
    match kerning_mode {
        KerningFeatureMode::Horizontal => {
            push_parley_font_feature(features, *b"kern", active_value);
            push_parley_font_feature(features, *b"vkrn", 0);
        }
        KerningFeatureMode::UprightVertical => {
            push_parley_font_feature(features, *b"kern", 0);
            push_parley_font_feature(features, *b"vkrn", active_value);
        }
    }
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
                if let Some(value) = resolver.get(FontFeatureValuesBlock::Stylistic, name) {
                    // `stylistic()` selects the named `@stylistic` value as
                    // the OpenType `salt` feature parameter, rather than
                    // merely enabling the feature. CSS Fonts defines the
                    // alias number as the selector supplied to `salt`.
                    // <https://www.w3.org/TR/css-fonts-4/#font-variant-alternates-prop>
                    push_parley_font_feature(features, *b"salt", value.feature_index);
                }
            }
            let styleset_indices = styleset
                .iter()
                .filter_map(|name| {
                    resolver
                        .get(FontFeatureValuesBlock::Styleset, name)
                        .map(|value| value.feature_index)
                })
                .collect::<Vec<_>>();
            if !styleset_indices.is_empty() {
                // `styleset()` establishes the selected set of numbered
                // stylistic sets. Disable every unselected `ss01`…`ss20`
                // feature so an OpenType default does not leak through a CSS
                // `font-variant-alternates` selection.
                // <https://www.w3.org/TR/css-fonts-4/#font-variant-alternates-prop>
                for index in 1..=20 {
                    push_numbered_feature(
                        features,
                        b"ss",
                        index,
                        20,
                        u16::from(styleset_indices.contains(&index)),
                    );
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
                if let Some(value) = resolver.get(FontFeatureValuesBlock::Swash, name) {
                    push_parley_font_feature(features, *b"swsh", value.feature_index);
                    push_parley_font_feature(features, *b"cswh", value.feature_index);
                }
            }
            for name in ornaments {
                if let Some(value) = resolver.get(FontFeatureValuesBlock::Ornaments, name) {
                    push_parley_font_feature(features, *b"ornm", value.feature_index);
                }
            }
            for name in annotation {
                if let Some(value) = resolver.get(FontFeatureValuesBlock::Annotation, name) {
                    push_parley_font_feature(features, *b"nalt", value.feature_index);
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
        CssWordBreak::Manual => ParleyWordBreak::Normal,
        CssWordBreak::BreakWord => ParleyWordBreak::Normal,
    }
}

pub(super) fn parley_overflow_wrap(overflow_wrap: CssOverflowWrap) -> ParleyOverflowWrap {
    match overflow_wrap {
        CssOverflowWrap::Normal => ParleyOverflowWrap::Normal,
        CssOverflowWrap::Anywhere => ParleyOverflowWrap::Anywhere,
        CssOverflowWrap::BreakWord => ParleyOverflowWrap::BreakWord,
    }
}

pub(super) fn parley_text_wrap_mode(style: &ComputedStyle) -> ParleyTextWrapMode {
    if style.allows_soft_wrap() {
        ParleyTextWrapMode::Wrap
    } else {
        ParleyTextWrapMode::NoWrap
    }
}

pub(super) fn parley_font_style(style: FontStyle) -> ParleyFontStyle {
    match style {
        FontStyle::Normal => ParleyFontStyle::Normal,
        FontStyle::Italic => ParleyFontStyle::Italic,
        FontStyle::Oblique(angle) => ParleyFontStyle::Oblique(Some(f32::from_bits(angle))),
    }
}

/// The CSS Fonts standard UI family names also identify platform font
/// families when written as a quoted name. Unlike generic keywords such as
/// `serif`, these names are specified aliases for the platform UI design
/// system.
/// <https://www.w3.org/TR/css-fonts-4/#standard-font-families>
pub(super) fn standard_ui_family_alias(name: &str) -> Option<FontFamily> {
    match name.trim().to_ascii_lowercase().as_str() {
        "system-ui" => Some(FontFamily::SystemUi),
        "ui-serif" => Some(FontFamily::UiSerif),
        "ui-sans-serif" => Some(FontFamily::UiSansSerif),
        "ui-monospace" => Some(FontFamily::UiMonospace),
        "ui-rounded" => Some(FontFamily::UiRounded),
        _ => None,
    }
}

/// Platform-private implementation names must not escape as author-visible
/// family names. CSS exposes the UI design system only through the standard
/// `system-ui` and `ui-*` aliases above.
/// <https://www.w3.org/TR/css-fonts-4/#standard-font-families>
pub(super) fn is_private_standard_ui_family_name(name: &str) -> bool {
    matches!(
        name.trim(),
        ".AppleSystemUIFontSerif"
            | ".AppleSystemUIFont"
            | ".AppleSystemUIFontRounded"
            | ".SF NS Mono"
            | ".SF UI Mono"
    )
}

pub(super) fn generic_query_families(
    family: &FontFamily,
    weight: FontWeight,
) -> Option<&'static [FontiqueQueryFamily<'static>]> {
    const SANS_SERIF_BOLD: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Named("Arial"),
        FontiqueQueryFamily::Named("Helvetica Neue"),
        FontiqueQueryFamily::Named("Helvetica"),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::SansSerif),
    ];
    const SANS_SERIF: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Named("Helvetica"),
        FontiqueQueryFamily::Named("Helvetica Neue"),
        FontiqueQueryFamily::Named("Arial"),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::SansSerif),
    ];
    const SERIF: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Named("Times New Roman"),
        FontiqueQueryFamily::Named("Times"),
        FontiqueQueryFamily::Named("Georgia"),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::Serif),
    ];
    const MONOSPACE_BOLD: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Named("Courier New"),
        FontiqueQueryFamily::Named("Courier"),
        FontiqueQueryFamily::Named("PT Mono"),
        FontiqueQueryFamily::Named("Andale Mono"),
        FontiqueQueryFamily::Named("Menlo"),
        FontiqueQueryFamily::Named(".SF NS Mono"),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::Monospace),
    ];
    const MONOSPACE: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Named("Menlo"),
        FontiqueQueryFamily::Named("Andale Mono"),
        FontiqueQueryFamily::Named("PT Mono"),
        FontiqueQueryFamily::Named(".SF NS Mono"),
        FontiqueQueryFamily::Named("Courier New"),
        FontiqueQueryFamily::Named("Courier"),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::Monospace),
    ];
    const SYSTEM_UI: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Named(".SF NS"),
        FontiqueQueryFamily::Named("System Font"),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::SystemUi),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::SansSerif),
    ];
    const UI_SERIF: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::UiSerif),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::Serif),
    ];
    const UI_SANS_SERIF: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::UiSansSerif),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::SystemUi),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::SansSerif),
    ];
    const UI_MONOSPACE: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::UiMonospace),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::Monospace),
    ];
    const UI_ROUNDED: &[FontiqueQueryFamily<'static>] = &[
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::UiRounded),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::SystemUi),
        FontiqueQueryFamily::Generic(FontiqueGenericFamily::SansSerif),
    ];

    match family {
        FontFamily::SansSerif if weight.0 >= FontWeight::BOLD.0 => Some(SANS_SERIF_BOLD),
        FontFamily::SansSerif => Some(SANS_SERIF),
        FontFamily::Serif => Some(SERIF),
        FontFamily::Monospace if weight.0 >= FontWeight::BOLD.0 => Some(MONOSPACE_BOLD),
        FontFamily::Monospace => Some(MONOSPACE),
        FontFamily::SystemUi => Some(SYSTEM_UI),
        FontFamily::UiSerif => Some(UI_SERIF),
        FontFamily::UiSansSerif => Some(UI_SANS_SERIF),
        FontFamily::UiMonospace => Some(UI_MONOSPACE),
        FontFamily::UiRounded => Some(UI_ROUNDED),
        FontFamily::List(_) => None,
        FontFamily::Names(_) => None,
    }
}

impl FontRequest {
    fn generic_family(family: &FontFamily) -> Option<GenericFontRequest> {
        match family {
            FontFamily::SansSerif => Some(GenericFontRequest::SansSerif),
            FontFamily::Serif => Some(GenericFontRequest::Serif),
            FontFamily::Monospace => Some(GenericFontRequest::Monospace),
            FontFamily::SystemUi => Some(GenericFontRequest::SystemUi),
            FontFamily::UiSerif => Some(GenericFontRequest::UiSerif),
            FontFamily::UiSansSerif => Some(GenericFontRequest::UiSansSerif),
            FontFamily::UiMonospace => Some(GenericFontRequest::UiMonospace),
            FontFamily::UiRounded => Some(GenericFontRequest::UiRounded),
            FontFamily::List(_) => None,
            FontFamily::Names(_) => None,
        }
    }

    fn normalized_names_key(names: &[String]) -> FontRequestFamily {
        let names = names
            .iter()
            .map(|name| normalize_family(name))
            .collect::<Vec<_>>();
        if names.len() == 1 {
            FontRequestFamily::Named(names.into_iter().next().unwrap())
        } else {
            FontRequestFamily::Names(names)
        }
    }

    fn family_key(family: &FontFamily) -> FontRequestFamily {
        if let Some(generic) = Self::generic_family(family) {
            return FontRequestFamily::Generic(generic);
        }
        match family {
            FontFamily::Names(names) => Self::normalized_names_key(names),
            FontFamily::List(families) => {
                FontRequestFamily::List(families.iter().map(Self::family_key).collect())
            }
            _ => unreachable!("generic font families returned above"),
        }
    }

    pub(super) fn from_family(
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Self {
        if let Some(generic) = Self::generic_family(family) {
            return Self {
                family: FontRequestFamily::Generic(generic),
                attributes: font_request_attributes(weight, style, width),
            };
        }

        Self {
            family: Self::family_key(family),
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
            family: FontRequestFamily::Named(normalize_family(name)),
            attributes: font_request_attributes(weight, style, width),
        }
    }

    pub(super) fn generic(
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        width: FontWidth,
    ) -> Self {
        let Some(generic) = Self::generic_family(family) else {
            return Self::from_family(family, weight, style, width);
        };
        Self {
            family: FontRequestFamily::Generic(generic),
            attributes: font_request_attributes(weight, style, width),
        }
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
            FontStyle::Oblique(_) => 2,
        },
        width: width.0,
    }
}

/// A cmap-resolved glyph that is safe to treat as character coverage.
///
/// OpenType glyph zero is `.notdef`, which is the missing-glyph fallback and
/// not evidence that the font covers the requested Unicode scalar. Keeping
/// this invariant in a distinct type prevents font matching and metric probes
/// from accidentally selecting `.notdef` as a real character glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::text) struct CoveredGlyphId(ttf_parser::GlyphId);

impl CoveredGlyphId {
    pub(in crate::text) fn from_face(face: &ttf_parser::Face<'_>, character: char) -> Option<Self> {
        face.glyph_index(character)
            .filter(|glyph| glyph.0 != 0)
            .map(Self)
    }

    pub(in crate::text) const fn raw(self) -> ttf_parser::GlyphId {
        self.0
    }
}

/// The selected document font and its verified cmap glyph for one scalar.
///
/// This is the boundary between CSS font fallback and consumers that need a
/// real glyph metric. In particular, numeric `tab-size` must measure U+0020
/// from this match rather than re-querying an unchecked glyph id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::text) struct CharacterFontMatch {
    pub(in crate::text) font_id: usize,
    pub(in crate::text) glyph_id: CoveredGlyphId,
}

pub(super) fn font_covered_glyph(font: &DocumentFont, character: char) -> Option<CoveredGlyphId> {
    ttf_parser::Face::parse(&font.data, font.face_index)
        .ok()
        .and_then(|face| CoveredGlyphId::from_face(&face, character))
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

pub(crate) fn opentype_name(face: &ttf_parser::Face<'_>, name_id: u16) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{FontFeatureSetting, FontFeatureSettings, WritingMode};

    fn vertical_range_texts<'a>(text: &'a str, style: &ComputedStyle) -> Vec<&'a str> {
        vertical_form_feature_ranges(text, style)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    fn feature_value(features: &ParleyFontFeatures<'static>, tag: [u8; 4]) -> Option<u16> {
        match features {
            ParleyFontFeatures::List(features) => features
                .iter()
                .find(|feature| feature.tag == ParleyTag::from_bytes(tag))
                .map(|feature| feature.value),
            ParleyFontFeatures::Source(_) => None,
        }
    }

    #[test]
    fn fixed_font_face_descriptors_pin_standard_variable_axes() {
        let defaults = fontique_fixed_standard_axis_defaults(
            FontWeight::BLACK,
            false,
            FontWidth::CONDENSED,
            false,
        );

        assert_eq!(
            defaults,
            vec![
                (OpenTypeTag::new(b"wght"), 900.0),
                (OpenTypeTag::new(b"wdth"), 75.0),
            ]
        );
    }

    #[test]
    fn variable_font_face_descriptors_keep_intrinsic_axis_defaults() {
        assert!(
            fontique_fixed_standard_axis_defaults(
                FontWeight::BLACK,
                true,
                FontWidth::EXPANDED,
                true,
            )
            .is_empty()
        );
    }

    #[test]
    fn css_standard_font_properties_become_parley_variations() {
        let mut style = ComputedStyle::initial();
        style.font_weight = FontWeight::BLACK;
        style.font_width = FontWidth::CONDENSED;

        let ParleyFontVariations::List(variations) = parley_standard_font_variations(&style) else {
            panic!("standard variations should use a concrete setting list");
        };
        assert_eq!(
            variations.as_ref(),
            [
                ParleyFontVariation::new(ParleyTag::new(b"wdth"), 75.0),
                ParleyFontVariation::new(ParleyTag::new(b"wght"), 900.0),
            ]
        );
    }

    #[test]
    fn font_feature_precedence_layers_face_defaults_variants_tracking_and_low_level_settings() {
        let defaults = FontFaceFeatureDefaults {
            font_feature_settings: FontFeatureSettings::NORMAL,
            font_variant_ligatures: FontVariantLigatures::Values {
                common: None,
                discretionary: Some(true),
                historical: None,
                contextual: None,
            },
            font_variant_position: FontVariantPosition::Normal,
            font_variant_caps: FontVariantCaps::Normal,
            font_variant_numeric: FontVariantNumeric::Normal,
            font_variant_alternates: FontVariantAlternates::Normal,
            font_variant_east_asian: FontVariantEastAsian::Normal,
        };
        let context = FontFeatureContext {
            family: Some("FeatureDefaults".to_string()),
            face_defaults: Some(defaults),
            font_feature_values: FontFeatureValues::default(),
        };
        let mut disabled_by_variant = ComputedStyle::initial();
        disabled_by_variant.font_variant_ligatures = FontVariantLigatures::Values {
            common: None,
            discretionary: Some(false),
            historical: None,
            contextual: None,
        };
        let mut reenabled_by_low_level = disabled_by_variant.clone();
        reenabled_by_low_level.font_feature_settings =
            FontFeatureSettings(vec![FontFeatureSetting::new(*b"dlig", 1)]);

        let cases = [
            ("font-face default", ComputedStyle::initial(), 0.0, Some(1)),
            ("element variant", disabled_by_variant.clone(), 0.0, Some(0)),
            ("letter spacing", disabled_by_variant, 1.0, Some(0)),
            ("low-level override", reenabled_by_low_level, 1.0, Some(1)),
        ];
        for (name, style, letter_spacing, expected_dlig) in cases {
            let features = parley_font_features(
                &style,
                letter_spacing,
                Some(&context),
                FontFeaturePolicy::default(),
            );
            assert_eq!(feature_value(&features, *b"dlig"), expected_dlig, "{name}");
        }

        let tracking_features = parley_font_features(
            &ComputedStyle::initial(),
            1.0,
            Some(&context),
            FontFeaturePolicy::default(),
        );
        for tag in [*b"liga", *b"clig", *b"dlig", *b"hlig", *b"calt"] {
            assert_eq!(feature_value(&tracking_features, tag), Some(0), "{tag:?}");
        }
    }

    #[test]
    fn vertical_form_policy_enables_features_for_upright_vertical_units_only() {
        let mut style = ComputedStyle::initial();
        assert!(vertical_form_feature_ranges("中文", &style).is_empty());

        style.writing_mode = WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Sideways;
        assert!(vertical_form_feature_ranges("中文", &style).is_empty());

        style.text_orientation = TextOrientation::Upright;
        assert_eq!(vertical_range_texts("a§、〈", &style), vec!["a§、〈"]);

        style.text_orientation = TextOrientation::Mixed;
        assert_eq!(vertical_range_texts("a§、〈", &style), vec!["§、〈"]);

        style.writing_mode = WritingMode::SidewaysLr;
        style.text_orientation = TextOrientation::Upright;
        assert!(vertical_form_feature_ranges("a中文", &style).is_empty());
    }

    #[test]
    fn vertical_form_policy_keeps_combining_units_with_their_base() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.text_orientation = TextOrientation::Mixed;

        assert_eq!(
            vertical_range_texts("\u{200d}中\u{301}", &style),
            vec!["\u{200d}中\u{301}"]
        );
        assert!(vertical_form_feature_ranges("a\u{301}", &style).is_empty());
    }

    #[test]
    fn vertical_form_features_are_css_implied_and_low_level_overridable() {
        let style = ComputedStyle::initial();
        let disabled = parley_font_features(&style, 0.0, None, FontFeaturePolicy::default());
        assert_eq!(feature_value(&disabled, *b"vert"), None);
        assert_eq!(feature_value(&disabled, *b"vrt2"), None);

        let enabled = parley_font_features(&style, 0.0, None, FontFeaturePolicy::UPRIGHT_VERTICAL);
        assert_eq!(feature_value(&enabled, *b"vert"), Some(1));
        assert_eq!(feature_value(&enabled, *b"vrt2"), Some(1));

        let mut overridden = style.clone();
        overridden.font_feature_settings = FontFeatureSettings(vec![
            FontFeatureSetting::new(*b"vert", 0),
            FontFeatureSetting::new(*b"vrt2", 0),
        ]);
        let features =
            parley_font_features(&overridden, 0.0, None, FontFeaturePolicy::UPRIGHT_VERTICAL);
        assert_eq!(feature_value(&features, *b"vert"), Some(0));
        assert_eq!(feature_value(&features, *b"vrt2"), Some(0));
    }

    #[test]
    fn kerning_feature_policy_tracks_typographic_orientation_per_range() {
        let horizontal = ComputedStyle::initial();

        let mut sideways = ComputedStyle::initial();
        sideways.writing_mode = WritingMode::SidewaysRl;

        let mut upright = ComputedStyle::initial();
        upright.writing_mode = WritingMode::VerticalRl;
        upright.text_orientation = TextOrientation::Upright;
        let upright_ranges = vertical_form_feature_ranges("AB", &upright);
        assert_eq!(
            FontFeaturePolicy::for_text("AB".len(), &upright_ranges),
            FontFeaturePolicy::UPRIGHT_VERTICAL
        );

        let mut mixed = upright.clone();
        mixed.text_orientation = TextOrientation::Mixed;
        let mixed_ranges = vertical_form_feature_ranges("A中", &mixed);
        assert_eq!(mixed_ranges, vec!["A".len().."A中".len()]);
        assert_eq!(
            FontFeaturePolicy::for_text("A中".len(), &mixed_ranges),
            FontFeaturePolicy::default(),
        );

        let cases = [
            (
                "horizontal",
                horizontal,
                FontFeaturePolicy::default(),
                (1, 0),
            ),
            ("sideways", sideways, FontFeaturePolicy::default(), (1, 0)),
            (
                "upright vertical",
                upright,
                FontFeaturePolicy::UPRIGHT_VERTICAL,
                (0, 1),
            ),
            (
                "mixed upright range",
                mixed,
                FontFeaturePolicy::UPRIGHT_VERTICAL,
                (0, 1),
            ),
        ];
        for (name, mut style, policy, normal_values) in cases {
            for (font_kerning, expected) in [
                (FontKerning::Auto, normal_values),
                (FontKerning::Normal, normal_values),
                (FontKerning::None, (0, 0)),
            ] {
                style.font_kerning = font_kerning;
                let features = parley_font_features(&style, 0.0, None, policy);
                assert_eq!(
                    feature_value(&features, *b"kern"),
                    Some(expected.0),
                    "{name}"
                );
                assert_eq!(
                    feature_value(&features, *b"vkrn"),
                    Some(expected.1),
                    "{name}"
                );
            }
        }

        let mut low_level_override = ComputedStyle::initial();
        low_level_override.font_kerning = FontKerning::None;
        low_level_override.font_feature_settings = FontFeatureSettings(vec![
            FontFeatureSetting::new(*b"kern", 1),
            FontFeatureSetting::new(*b"vkrn", 1),
        ]);
        let features = parley_font_features(
            &low_level_override,
            0.0,
            None,
            FontFeaturePolicy::UPRIGHT_VERTICAL,
        );
        assert_eq!(feature_value(&features, *b"kern"), Some(1));
        assert_eq!(feature_value(&features, *b"vkrn"), Some(1));
    }
}
