/// Computed CSS `direction`.
///
/// CSS Writing Modes defines `direction` as the inline base direction used by
/// flow-relative property mapping and bidi layout:
/// <https://www.w3.org/TR/css-writing-modes-4/#direction>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Ltr,
    Rtl,
}

/// Computed CSS `unicode-bidi`.
///
/// CSS Writing Modes defines this property as the control for bidi embedding,
/// isolation, overrides, and plaintext paragraph direction resolution:
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnicodeBidi {
    Normal,
    Embed,
    Isolate,
    BidiOverride,
    IsolateOverride,
    Plaintext,
}

/// Computed CSS `writing-mode`.
///
/// This deliberately preserves every modern CSS Writing Modes keyword. The
/// physical geometry and typographic behavior derived from a value are related
/// but not interchangeable: sideways modes have vertical line geometry and
/// horizontal typographic mode.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#block-flow> and
/// <https://www.w3.org/TR/css-writing-modes-4/#typographic-mode>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WritingMode {
    HorizontalTb,
    VerticalRl,
    VerticalLr,
    SidewaysRl,
    SidewaysLr,
}

/// The typographic mode selected by a CSS writing mode.
///
/// `text-orientation` only affects vertical typographic mode. Sideways modes
/// use horizontal metrics and composition even though their line geometry is
/// vertical:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypographicMode {
    Horizontal,
    Vertical,
}

/// The direction in which a sideways writing mode rotates horizontal text.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#valdef-writing-mode-sideways-rl>
/// and
/// <https://www.w3.org/TR/css-writing-modes-4/#valdef-writing-mode-sideways-lr>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidewaysOrientation {
    Right,
    Left,
}

/// The text shaping and placement policy selected by computed writing values.
///
/// This is the used-value boundary between writing-mode geometry and text
/// layout. In particular, a sideways writing mode selects a forced horizontal
/// run rotation and suppresses `text-orientation`; it is not a vertical mode
/// with `text-orientation: sideways`.
///
/// <https://www.w3.org/TR/css-writing-modes-4/#typographic-mode> and
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextLayoutPolicy {
    Horizontal,
    Vertical(TextOrientation),
    Sideways(SidewaysOrientation),
}

impl WritingMode {
    /// Whether line and block geometry use vertical writing axes.
    ///
    /// This is distinct from [`Self::typographic_mode`]: sideways modes have
    /// vertical geometry, but horizontal typography.
    pub(crate) const fn has_vertical_lines(self) -> bool {
        !matches!(self, Self::HorizontalTb)
    }

    /// Return the typographic mode used for shaping, metrics, and baselines.
    pub(crate) const fn typographic_mode(self) -> TypographicMode {
        match self {
            Self::HorizontalTb | Self::SidewaysRl | Self::SidewaysLr => TypographicMode::Horizontal,
            Self::VerticalRl | Self::VerticalLr => TypographicMode::Vertical,
        }
    }

    /// Return the forced sideways orientation, if this is a sideways mode.
    pub(crate) const fn sideways_orientation(self) -> Option<SidewaysOrientation> {
        match self {
            Self::SidewaysRl => Some(SidewaysOrientation::Right),
            Self::SidewaysLr => Some(SidewaysOrientation::Left),
            Self::HorizontalTb | Self::VerticalRl | Self::VerticalLr => None,
        }
    }

    /// Derive the text-layout policy for this writing mode and computed
    /// `text-orientation`.
    ///
    /// `text-orientation` applies only in vertical typographic mode. The two
    /// sideways values instead force all typographic units into horizontally
    /// shaped runs rotated toward their specified line-right direction.
    pub(crate) const fn text_layout_policy(
        self,
        text_orientation: TextOrientation,
    ) -> TextLayoutPolicy {
        if let Some(sideways_orientation) = self.sideways_orientation() {
            return TextLayoutPolicy::Sideways(sideways_orientation);
        }
        match self {
            Self::HorizontalTb => TextLayoutPolicy::Horizontal,
            Self::VerticalRl | Self::VerticalLr => TextLayoutPolicy::Vertical(text_orientation),
            Self::SidewaysRl | Self::SidewaysLr => unreachable!(),
        }
    }

    /// Whether the LTR physical inline progression starts at the bottom of a
    /// vertical line rather than its top.
    pub(crate) const fn ltr_inline_progresses_upward(self) -> bool {
        matches!(self, Self::SidewaysLr)
    }
}

#[cfg(test)]
mod writing_mode_tests {
    use super::*;

    #[test]
    fn text_layout_policy_ignores_text_orientation_for_sideways_modes() {
        assert_eq!(
            WritingMode::HorizontalTb.text_layout_policy(TextOrientation::Upright),
            TextLayoutPolicy::Horizontal
        );
        assert_eq!(
            WritingMode::VerticalRl.text_layout_policy(TextOrientation::Mixed),
            TextLayoutPolicy::Vertical(TextOrientation::Mixed)
        );
        assert_eq!(
            WritingMode::VerticalLr.text_layout_policy(TextOrientation::Upright),
            TextLayoutPolicy::Vertical(TextOrientation::Upright)
        );
        assert_eq!(
            WritingMode::SidewaysRl.text_layout_policy(TextOrientation::Upright),
            TextLayoutPolicy::Sideways(SidewaysOrientation::Right)
        );
        assert_eq!(
            WritingMode::SidewaysLr.text_layout_policy(TextOrientation::Mixed),
            TextLayoutPolicy::Sideways(SidewaysOrientation::Left)
        );
    }
}

/// Computed CSS `text-orientation`.
///
/// CSS Writing Modes defines the orientation of typographic character units in
/// vertical writing modes. Horizontal writing ignores this property at used
/// value time:
/// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextOrientation {
    Mixed,
    Upright,
    Sideways,
}

/// Computed CSS `text-combine-upright`.
///
/// The property requests a tate-chu-yoko atomic inline in vertical typographic
/// modes. `Digits` retains its author-selected maximum run length so inline
/// collection can form the atom before shaping and line breaking.
/// <https://drafts.csswg.org/css-writing-modes-4/#text-combine-upright>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TextCombineUpright {
    #[default]
    None,
    All,
    Digits(u8),
}
