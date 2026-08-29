/// CSS Box Alignment lets positional alignment opt into `safe` fallback
/// behavior when the alignment subject would overflow the alignment container:
/// <https://www.w3.org/TR/css-align-3/#overflow-values>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlignmentSafety {
    Default,
    Unsafe,
    Safe,
}

/// Content-distribution keyword for `justify-content` and `align-content`.
///
/// CSS Box Alignment defines the shared content-alignment grammar, while
/// individual properties restrict which keywords are accepted:
/// <https://www.w3.org/TR/css-align-3/#content-distribution>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentAlignmentKeyword {
    Normal,
    Start,
    End,
    FlexStart,
    FlexEnd,
    Left,
    Right,
    Center,
    Stretch,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Baseline,
    LastBaseline,
}

/// Computed content-alignment value for `justify-content`/`align-content`.
///
/// CSS Box Alignment defines the main-axis distribution keywords used by
/// flex containers:
/// <https://www.w3.org/TR/css-align-3/#propdef-justify-content>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentAlignment {
    pub keyword: ContentAlignmentKeyword,
    pub safety: AlignmentSafety,
}

impl ContentAlignment {
    pub const NORMAL: Self = Self::new(ContentAlignmentKeyword::Normal);

    pub const fn new(keyword: ContentAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Default,
        }
    }

    pub const fn safe(keyword: ContentAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Safe,
        }
    }

    pub const fn unsafe_position(keyword: ContentAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Unsafe,
        }
    }
}

pub(crate) type JustifyContent = ContentAlignment;
pub(crate) type AlignContent = ContentAlignment;

/// Self-alignment keyword for `align-items`/`align-self`/`justify-*`.
///
/// CSS Box Alignment separates self-alignment from content distribution. The
/// `left`/`right` keywords are valid only for justify-* properties, and parser
/// entrypoints enforce that property-specific restriction:
/// <https://www.w3.org/TR/css-align-3/#self-position>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelfAlignmentKeyword {
    Auto,
    Normal,
    Start,
    End,
    SelfStart,
    SelfEnd,
    FlexStart,
    FlexEnd,
    Left,
    Right,
    Center,
    Stretch,
    Baseline,
    LastBaseline,
}

/// Computed self-alignment value for `align-*` and `justify-*` self properties.
///
/// CSS Box Alignment defines `justify-items` as the inline-axis default
/// self-alignment for child boxes. Flex containers do not use it for normal
/// flex items, but the computed value must still cascade correctly and is
/// needed by `place-items`:
/// <https://www.w3.org/TR/css-align-3/#justify-items-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#alignment>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelfAlignment {
    pub keyword: SelfAlignmentKeyword,
    pub safety: AlignmentSafety,
}

impl SelfAlignment {
    pub const AUTO: Self = Self::new(SelfAlignmentKeyword::Auto);
    pub const NORMAL: Self = Self::new(SelfAlignmentKeyword::Normal);

    pub const fn new(keyword: SelfAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Default,
        }
    }

    pub const fn safe(keyword: SelfAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Safe,
        }
    }

    pub const fn unsafe_position(keyword: SelfAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Unsafe,
        }
    }
}

pub(crate) type JustifyItems = SelfAlignment;
pub(crate) type JustifySelf = SelfAlignment;
pub(crate) type AlignItems = SelfAlignment;
pub(crate) type AlignSelf = SelfAlignment;
