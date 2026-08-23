#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTransform {
    None,
    MathAuto,
    Keywords(TextTransformKeywords),
}

impl TextTransform {
    pub(crate) const NONE: Self = Self::None;

    pub(crate) const fn case(self) -> Option<TextTransformCase> {
        match self {
            Self::Keywords(keywords) => keywords.case,
            Self::None | Self::MathAuto => None,
        }
    }

    pub(crate) const fn applies_full_width(self) -> bool {
        matches!(self, Self::Keywords(keywords) if keywords.full_width)
    }

    pub(crate) const fn applies_full_size_kana(self) -> bool {
        matches!(self, Self::Keywords(keywords) if keywords.full_size_kana)
    }

    pub(crate) const fn applies_math_auto(self) -> bool {
        matches!(self, Self::MathAuto)
    }
}

/// The non-`math-auto` keyword set for CSS `text-transform`.
///
/// Its constructor rejects the empty set so that `TextTransform::None`
/// remains the sole representation of the initial value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextTransformKeywords {
    case: Option<TextTransformCase>,
    full_width: bool,
    full_size_kana: bool,
}

impl TextTransformKeywords {
    pub(crate) const fn new(
        case: Option<TextTransformCase>,
        full_width: bool,
        full_size_kana: bool,
    ) -> Option<Self> {
        if case.is_some() || full_width || full_size_kana {
            Some(Self {
                case,
                full_width,
                full_size_kana,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTransformCase {
    Uppercase,
    Lowercase,
    Capitalize,
}
