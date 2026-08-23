use super::*;

/// Computed CSS `position`, including GCPM running elements.
/// <https://www.w3.org/TR/css-position-3/#position-property>
/// <https://www.w3.org/TR/css-gcpm-3/#running-elements>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
    Running(RunningElementName),
}

impl Position {
    pub(crate) const fn is_running(&self) -> bool {
        matches!(self, Self::Running(_))
    }

    pub(crate) const fn is_out_of_flow_positioned(&self) -> bool {
        matches!(self, Self::Absolute | Self::Fixed)
    }

    pub(crate) const fn is_in_flow_positioned(&self) -> bool {
        matches!(self, Self::Relative | Self::Sticky)
    }

    pub(crate) const fn is_normal_flow(&self) -> bool {
        matches!(
            self,
            Self::Static | Self::Relative | Self::Sticky | Self::Running(_)
        )
    }
}

/// Computed `float` value.
///
/// CSS 2.2 defines left and right floats as boxes shifted to the containing
/// block edge with following flow content shortened around them. GCPM extends
/// the same property with page-footnote extraction:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats> and
/// <https://www.w3.org/TR/css-gcpm-3/#footnotes>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Float {
    None,
    Left,
    Right,
    InlineStart,
    InlineEnd,
    Footnote,
}

/// Computed `clear` value.
///
/// CSS 2.2 defines clearance as moving a box below prior left and/or right
/// floats in the same block formatting context:
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clear {
    None,
    Left,
    Right,
    Both,
    InlineStart,
    InlineEnd,
}

/// Computed CSS `z-index`.
/// <https://www.w3.org/TR/css-position-3/#propdef-z-index>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZIndex {
    Auto,
    StackLevel(i32),
}

impl ZIndex {
    pub(crate) const fn stack_level(self) -> Option<i32> {
        match self {
            Self::Auto => None,
            Self::StackLevel(level) => Some(level),
        }
    }

    pub(crate) const fn establishes_stacking_context(self) -> bool {
        matches!(self, Self::StackLevel(_))
    }

    pub(crate) const fn unwrap_or(self, default: i32) -> i32 {
        match self {
            Self::Auto => default,
            Self::StackLevel(level) => level,
        }
    }
}
