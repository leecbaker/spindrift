mod computed_values;
pub(crate) use self::computed_values::*;

/// CSS `box-sizing` determines which box the width and height properties size.
/// <https://www.w3.org/TR/css-sizing-3/#box-sizing>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxSizing {
    ContentBox,
    BorderBox,
}

/// Computed CSS `margin-trim` flags.
///
/// CSS Box Model Level 4 lets block containers trim margins adjoining their
/// edges. The property accepts axis shorthands (`block`, `inline`) and
/// individual sides:
/// <https://drafts.csswg.org/css-box-4/#margin-trim>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarginTrim {
    pub block_start: bool,
    pub block_end: bool,
    pub inline_start: bool,
    pub inline_end: bool,
}

impl MarginTrim {
    pub const NONE: Self = Self {
        block_start: false,
        block_end: false,
        inline_start: false,
        inline_end: false,
    };
}
