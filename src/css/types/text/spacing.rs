/// Computed CSS `text-autospace`.
///
/// CSS Text Level 4 defines automatic spacing between Han ideographs and
/// adjacent non-ideographic letters or numbers. The computed value is an
/// unordered keyword set, with `normal`/`auto` enabling the UA default set:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextAutospace {
    pub(crate) ideograph_alpha: bool,
    pub(crate) ideograph_numeric: bool,
    pub(crate) punctuation: bool,
}

/// Computed CSS `text-spacing-trim`.
///
/// The property selects the CJK punctuation-spacing policy used after a line
/// candidate has established its physical inline edges:
/// <https://drafts.csswg.org/css-text-4/#text-spacing-trim-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextSpacingTrim {
    SpaceAll,
    Normal,
    SpaceFirst,
    TrimStart,
    TrimBoth,
    TrimAll,
    Auto,
}

impl TextSpacingTrim {
    /// Spindrift's deterministic user-agent policy for the spec-defined `auto`
    /// value. `normal` is conservative and preserves the initial-value
    /// behavior while avoiding platform-dependent PDF output.
    pub(crate) const fn resolved(self) -> Self {
        match self {
            Self::Auto => Self::Normal,
            value => value,
        }
    }
}

impl TextAutospace {
    pub(crate) const NONE: Self = Self {
        ideograph_alpha: false,
        ideograph_numeric: false,
        punctuation: false,
    };

    pub(crate) const NORMAL: Self = Self {
        ideograph_alpha: true,
        ideograph_numeric: true,
        punctuation: false,
    };

    pub(crate) fn is_none(self) -> bool {
        !self.ideograph_alpha && !self.ideograph_numeric && !self.punctuation
    }
}

/// Computed CSS `word-space-transform`.
///
/// CSS Text Level 4 can replace explicit virtual word separators (`<wbr>` and
/// U+200B) with layout-only spaces. `auto-phrase` additionally introduces
/// virtual separators from language-sensitive phrase segmentation; that
/// source is kept distinct from explicit separators in inline collection:
/// <https://drafts.csswg.org/css-text-4/#word-space-transform>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WordSpaceTransform {
    pub(crate) replacement: Option<WordSpaceReplacement>,
    pub(crate) auto_phrase: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordSpaceReplacement {
    Space,
    IdeographicSpace,
}

impl WordSpaceTransform {
    pub(crate) const NONE: Self = Self {
        replacement: None,
        auto_phrase: false,
    };
}
