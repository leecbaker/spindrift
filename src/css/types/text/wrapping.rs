#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WhiteSpace {
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
}

/// Computed CSS Text wrapping mode.
///
/// `white-space` is a legacy shorthand that also sets this component, but
/// `text-wrap-mode` can subsequently override it without changing collapse or
/// segment-break preservation. CSS Text Level 4 defines it as an inherited
/// longhand: <https://drafts.csswg.org/css-text-4/#text-wrap-mode-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextWrapMode {
    /// No CSS `text-wrap-mode` longhand has overridden the legacy shorthand.
    /// This internal state preserves the semantic relationship for styles
    /// assembled directly by layout and UA code.
    Legacy,
    Wrap,
    NoWrap,
}

/// Computed CSS Text wrapping style.
///
/// The style selects among the graph's already-legal soft wrap opportunities;
/// it must never create an opportunity forbidden by `text-wrap-mode` or
/// `white-space`. <https://drafts.csswg.org/css-text-4/#text-wrap-style-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextWrapStyle {
    Auto,
    Balance,
    Stable,
}

/// Controls the preference for soft line breaks within an inline box.
///
/// CSS Text Level 4 makes this a non-inherited property of inline boxes. An
/// `avoid` box retains its ordinary break opportunities, but line selection
/// must prefer an equally fitting opportunity outside that box:
/// <https://drafts.csswg.org/css-text-4/#wrap-inside-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WrapInside {
    #[default]
    Auto,
    Avoid,
}

impl WhiteSpace {
    pub(crate) fn collapses_spaces(self) -> bool {
        matches!(self, Self::Normal | Self::NoWrap | Self::PreLine)
    }

    pub(crate) fn preserves_newlines(self) -> bool {
        matches!(
            self,
            Self::Pre | Self::PreWrap | Self::PreLine | Self::BreakSpaces
        )
    }

    /// Return whether trailing Unicode space separators hang at line end.
    ///
    /// CSS Text white-space processing makes trailing "other space
    /// separators" hang in every legacy white-space mode other than
    /// `break-spaces`. Unlike U+0020, these Unicode separators are not
    /// document white space, so `pre` and `pre-wrap` preservation does not
    /// suppress the Phase II hanging rule:
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
    pub(crate) fn hangs_trailing_space_separators(self) -> bool {
        self != Self::BreakSpaces
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordBreak {
    Normal,
    BreakAll,
    KeepAll,
    /// CSS Text 4 retains ordinary wrapping except within language-detected
    /// phrases. The phrase detector itself lives at the inline paragraph
    /// boundary, where it can preserve one source coordinate system.
    /// <https://drafts.csswg.org/css-text-4/#valdef-word-break-auto-phrase>
    AutoPhrase,
    /// CSS Text 4 disables automatic word-boundary detection in complex
    /// (notably Southeast Asian) scripts while retaining manual breaks.
    /// <https://drafts.csswg.org/css-text-4/#word-boundary-detection>
    Manual,
    /// Legacy `word-break: break-word` behaves as `overflow-wrap: anywhere`
    /// for line breaking and intrinsic sizing, without changing the authored
    /// `overflow-wrap` computed value.
    /// <https://drafts.csswg.org/css-text-3/#valdef-word-break-break-word>
    BreakWord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowWrap {
    Normal,
    Anywhere,
    BreakWord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineBreak {
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}
