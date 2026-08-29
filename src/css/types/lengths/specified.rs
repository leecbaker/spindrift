use crate::css::CSS_PX_TO_PT;
use crate::units::{LayoutLength, layout_pt};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AbsoluteLengthUnit {
    Px,
    Pt,
    In,
    Cm,
    Mm,
    Q,
    Pc,
    NumberPt,
}

impl AbsoluteLengthUnit {
    pub(crate) fn length_for_value(self, value: f32) -> LayoutLength {
        let points_per_unit = match self {
            Self::Px => CSS_PX_TO_PT,
            Self::Pt | Self::NumberPt => 1.0,
            Self::In => 72.0,
            Self::Cm => 72.0 / 2.54,
            Self::Mm => 72.0 / 25.4,
            Self::Q => 72.0 / 25.4 / 4.0,
            Self::Pc => 12.0,
        };
        layout_pt(value * points_per_unit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SpecifiedLength {
    Absolute {
        value: f32,
        unit: AbsoluteLengthUnit,
    },
    FontRelativeEm(f32),
    FontRelativeCh(f32),
    FontRelativeEx(f32),
    FontRelativeCap(f32),
    FontRelativeIc(f32),
    FontRelativeLh(f32),
    RootFontRelativeRem(f32),
    RootFontRelativeRex(f32),
    RootFontRelativeRcap(f32),
    RootFontRelativeRch(f32),
    RootFontRelativeRic(f32),
    RootFontRelativeRlh(f32),
}

impl SpecifiedLength {
    // CSS Cascade 5 value processing resolves font-relative specified lengths
    // when computed values are produced; layout later turns computed
    // length-percentages into used values against a containing block.
    // https://www.w3.org/TR/css-cascade-5/#computed
    // https://www.w3.org/TR/css-values-4/#font-relative-lengths
    pub(crate) fn to_computed(self, font_size: f32, root_font_size: f32) -> ComputedLength {
        let length = match self {
            Self::Absolute { value, unit } => unit.length_for_value(value),
            Self::FontRelativeEm(value) => layout_pt(value * font_size),
            Self::FontRelativeCh(value) => layout_pt(value * font_size),
            Self::FontRelativeEx(value) => layout_pt(value * font_size * 0.5),
            Self::FontRelativeCap(value) => layout_pt(value * font_size * 0.7),
            Self::FontRelativeIc(value) => layout_pt(value * font_size),
            Self::FontRelativeLh(value) => layout_pt(value * font_size * 1.2),
            Self::RootFontRelativeRem(value) => layout_pt(value * root_font_size),
            Self::RootFontRelativeRex(value) => layout_pt(value * root_font_size * 0.5),
            Self::RootFontRelativeRcap(value) => layout_pt(value * root_font_size * 0.7),
            Self::RootFontRelativeRch(value) | Self::RootFontRelativeRic(value) => {
                layout_pt(value * root_font_size)
            }
            Self::RootFontRelativeRlh(value) => layout_pt(value * root_font_size * 1.2),
        };
        ComputedLength { length }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ComputedLength {
    pub length: LayoutLength,
}
