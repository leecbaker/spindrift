use std::collections::HashMap;

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
    Oblique,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FontFamily {
    SansSerif,
    Serif,
    Monospace,
    Names(Vec<String>),
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
