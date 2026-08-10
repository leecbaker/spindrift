use std::collections::HashMap;

use crate::CssColor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FontWeight(pub u16);

impl FontWeight {
    pub const THIN: Self = Self(100);
    pub const NORMAL: Self = Self(400);
    pub const BOLD: Self = Self(700);
    pub const BLACK: Self = Self(900);

    pub fn from_number(value: f32) -> Option<Self> {
        value
            .is_finite()
            .then(|| value.round())
            .filter(|value| (1.0..=1000.0).contains(value))
            .map(|value| Self(value as u16))
    }

    pub fn bolder(self) -> Self {
        match self.0 {
            0..=99 => Self::NORMAL,
            100..=349 => Self::NORMAL,
            350..=549 => Self::BOLD,
            550..=749 => Self::BLACK,
            750..=899 => Self::BLACK,
            _ => self,
        }
    }

    pub fn lighter(self) -> Self {
        match self.0 {
            0..=99 => self,
            100..=349 => Self::THIN,
            350..=549 => Self::THIN,
            550..=749 => Self::NORMAL,
            _ => Self::BOLD,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontStyle {
    Normal,
    Italic,
    /// CSS oblique angle in degrees, stored as IEEE-754 bits to keep
    /// computed styles hashable and comparable.
    Oblique(u32),
}

impl FontStyle {
    pub(crate) const DEFAULT_OBLIQUE: Self = Self::Oblique(14.0_f32.to_bits());

    pub(crate) const fn oblique_angle(self) -> Option<f32> {
        match self {
            Self::Oblique(angle) => Some(f32::from_bits(angle)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FontWidth(pub u16);

impl FontWidth {
    pub const ULTRA_CONDENSED: Self = Self(500);
    pub const EXTRA_CONDENSED: Self = Self(625);
    pub const CONDENSED: Self = Self(750);
    pub const SEMI_CONDENSED: Self = Self(875);
    pub const NORMAL: Self = Self(1000);
    pub const SEMI_EXPANDED: Self = Self(1125);
    pub const EXPANDED: Self = Self(1250);
    pub const EXTRA_EXPANDED: Self = Self(1500);
    pub const ULTRA_EXPANDED: Self = Self(2000);

    pub fn from_percent(percent: f32) -> Option<Self> {
        percent
            .is_finite()
            .then(|| (percent * 10.0).round())
            .filter(|value| *value >= 0.0 && *value <= u16::MAX as f32)
            .map(|value| Self(value as u16))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FontFamilyName(String);

impl FontFamilyName {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for FontFamilyName {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// A CSS font-family item or fallback list.
///
/// A named family is one CSS family name. Comma-separated fallback is modeled
/// only by [`Self::List`], so a caller cannot accidentally treat whitespace or
/// a backend candidate list as CSS fallback syntax.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum FontFamily {
    SansSerif,
    Serif,
    Monospace,
    SystemUi,
    UiSerif,
    UiSansSerif,
    UiMonospace,
    UiRounded,
    /// An ordered `font-family` fallback list. Each entry retains whether it
    /// was an unquoted generic keyword or a quoted family name.
    List(Vec<FontFamily>),
    Named(FontFamilyName),
}

impl FontFamily {
    pub(crate) fn named(value: impl Into<String>) -> Self {
        Self::Named(FontFamilyName::new(value))
    }

    #[cfg(test)]
    #[allow(non_snake_case)]
    pub(crate) fn Names(names: Vec<String>) -> Self {
        let mut names = names.into_iter().map(Self::named);
        let first = names
            .next()
            .expect("test font family list must be non-empty");
        match names.next() {
            None => first,
            Some(second) => Self::List(
                std::iter::once(first)
                    .chain(std::iter::once(second))
                    .chain(names)
                    .collect(),
            ),
        }
    }
}

/// Controls which missing typographic forms the UA may synthesize.
///
/// CSS Fonts defines these inherited permissions for face matching and
/// shaping; `font-synthesis: none` disables every form.
/// <https://www.w3.org/TR/css-fonts-4/#font-synthesis-intro>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FontSynthesis {
    pub(crate) weight: bool,
    pub(crate) style: bool,
    pub(crate) small_caps: bool,
    pub(crate) position: bool,
}

/// The OpenType language system selected by `font-language-override`.
///
/// CSS accepts a quoted OpenType language-system tag.  OpenType stores that
/// tag in a four-byte field, padding the common three-letter tags with a
/// trailing space.  Keeping the normalized tag rather than a BCP-47 locale is
/// important: this property changes only OpenType feature selection, not the
/// element language used by CSS Text.
/// <https://drafts.csswg.org/css-fonts-4/#font-language-override-prop>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontLanguageOverride {
    Normal,
    OpenType([u8; 4]),
}

impl FontLanguageOverride {
    pub(crate) const fn opentype_tag(self) -> Option<[u8; 4]> {
        match self {
            Self::Normal => None,
            Self::OpenType(tag) => Some(tag),
        }
    }
}

impl FontSynthesis {
    pub(crate) const ALL: Self = Self {
        weight: true,
        style: true,
        small_caps: true,
        position: true,
    };

    pub(crate) const NONE: Self = Self {
        weight: false,
        style: false,
        small_caps: false,
        position: false,
    };
}

/// Computed `font-size-adjust`.
///
/// CSS Fonts defines `font-size-adjust` as an inherited property that adjusts
/// the used font size to normalize a selected font metric while leaving the
/// computed `font-size` unchanged:
/// <https://www.w3.org/TR/css-fonts-5/#font-size-adjust-prop>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FontSizeAdjust {
    None,
    Value {
        metric: FontSizeAdjustMetric,
        value: FontSizeAdjustValue,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontSizeAdjustMetric {
    ExHeight,
    CapHeight,
    ChWidth,
    IcWidth,
    IcHeight,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FontSizeAdjustValue {
    Number(f32),
    FromFont,
}

/// A computed OpenType feature setting from CSS font feature controls.
///
/// CSS Fonts defines OpenType feature tags as four printable ASCII characters
/// with non-negative integer values, where 0 disables a feature and omitted or
/// `on` enables it:
/// <https://www.w3.org/TR/css-fonts-4/#font-feature-settings-prop>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FontFeatureSetting {
    pub(crate) tag: [u8; 4],
    pub(crate) value: u16,
}

impl FontFeatureSetting {
    pub(crate) const fn new(tag: [u8; 4], value: u16) -> Self {
        Self { tag, value }
    }
}

/// Computed `font-feature-settings`, stored as a de-duplicated feature map.
///
/// The computed value is sorted by tag, with duplicate specified tags resolved
/// by the last value in source order:
/// <https://www.w3.org/TR/css-fonts-4/#font-feature-settings-prop>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontFeatureSettings(pub(crate) Vec<FontFeatureSetting>);

impl FontFeatureSettings {
    pub(crate) const NORMAL: Self = Self(Vec::new());
}

/// A low-level OpenType variation-axis coordinate from
/// `font-variation-settings`.
/// <https://www.w3.org/TR/css-fonts-4/#font-variation-settings-def>
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FontVariationSetting {
    pub(crate) tag: [u8; 4],
    /// IEEE-754 bits retain CSS's computed numeric value while allowing the
    /// inherited computed style to remain comparable.
    pub(crate) value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontVariationSettings(pub(crate) Vec<FontVariationSetting>);

impl FontVariationSettings {
    pub(crate) const NORMAL: Self = Self(Vec::new());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontKerning {
    Auto,
    Normal,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontVariantLigatures {
    Normal,
    None,
    Values {
        common: Option<bool>,
        discretionary: Option<bool>,
        historical: Option<bool>,
        contextual: Option<bool>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontVariantPosition {
    Normal,
    Sub,
    Super,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontVariantCaps {
    Normal,
    SmallCaps,
    AllSmallCaps,
    PetiteCaps,
    AllPetiteCaps,
    Unicase,
    TitlingCaps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontVariantNumericValue {
    LiningNums,
    OldstyleNums,
    ProportionalNums,
    TabularNums,
    DiagonalFractions,
    StackedFractions,
    Ordinal,
    SlashedZero,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FontVariantNumeric {
    Normal,
    Values(Vec<FontVariantNumericValue>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FontVariantAlternates {
    Normal,
    Values {
        historical_forms: bool,
        stylistic: Vec<String>,
        styleset: Vec<String>,
        character_variant: Vec<String>,
        swash: Vec<String>,
        ornaments: Vec<String>,
        annotation: Vec<String>,
    },
}

impl FontVariantAlternates {
    #[cfg(test)]
    pub(crate) fn historical_forms() -> Self {
        Self::Values {
            historical_forms: true,
            stylistic: Vec::new(),
            styleset: Vec::new(),
            character_variant: Vec::new(),
            swash: Vec::new(),
            ornaments: Vec::new(),
            annotation: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontVariantEastAsianValue {
    Jis78,
    Jis83,
    Jis90,
    Jis04,
    Simplified,
    Traditional,
    FullWidth,
    ProportionalWidth,
    Ruby,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FontVariantEastAsian {
    Normal,
    Values(Vec<FontVariantEastAsianValue>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontVariantEmoji {
    Normal,
    Text,
    Emoji,
    Unicode,
}

/// The selected color palette for a COLR font.
///
/// CSS Fonts Level 4 resolves `light` and `dark` through CPAL palette-type
/// flags, while numeric and named selections refer to author-visible palette
/// entries and `@font-palette-values` rules respectively.
/// <https://www.w3.org/TR/css-fonts-4/#font-palette-prop>
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum FontPalette {
    #[default]
    Normal,
    Light,
    Dark,
    Index(u16),
    Named(String),
}

/// A named `@font-palette-values` rule after descriptor parsing.
///
/// Palette overrides are retained separately because they replace individual
/// CPAL entries during COLR paint evaluation rather than changing font
/// matching or OpenType shaping.
/// <https://www.w3.org/TR/css-fonts-4/#font-palette-values>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FontPaletteDefinition {
    pub(crate) families: Vec<String>,
    pub(crate) base: FontPalette,
    pub(crate) overrides: HashMap<u16, CssColor>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FontPaletteValues {
    pub(crate) values: HashMap<String, Vec<FontPaletteDefinition>>,
}

impl FontPaletteValues {
    pub(crate) fn insert(&mut self, name: String, definition: FontPaletteDefinition) {
        self.values
            .entry(name.trim().to_string())
            .or_default()
            .push(definition);
    }

    pub(crate) fn get(&self, name: &str) -> Option<&[FontPaletteDefinition]> {
        self.values.get(name.trim()).map(Vec::as_slice)
    }

    pub(crate) fn extend(&mut self, other: Self) {
        for (name, definitions) in other.values {
            self.values.entry(name).or_default().extend(definitions);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FontFeatureValuesBlock {
    Stylistic,
    Styleset,
    CharacterVariant,
    Swash,
    Ornaments,
    Annotation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontFeatureValue {
    pub(crate) feature_index: u16,
    pub(crate) selector: Option<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FontFeatureValues {
    pub(crate) values: HashMap<(String, FontFeatureValuesBlock, String), FontFeatureValue>,
}

impl FontFeatureValues {
    pub(crate) fn insert(
        &mut self,
        family: String,
        block: FontFeatureValuesBlock,
        name: String,
        value: FontFeatureValue,
    ) {
        self.values.insert(
            (normalize_font_feature_values_family(&family), block, name),
            value,
        );
    }

    pub(crate) fn get(
        &self,
        family: &str,
        block: FontFeatureValuesBlock,
        name: &str,
    ) -> Option<&FontFeatureValue> {
        self.values.get(&(
            normalize_font_feature_values_family(family),
            block,
            name.to_string(),
        ))
    }

    pub(crate) fn extend(&mut self, other: FontFeatureValues) {
        self.values.extend(other.values);
    }
}

pub(crate) fn normalize_font_feature_values_family(family: &str) -> String {
    family.trim().to_ascii_lowercase()
}
