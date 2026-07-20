use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundRepeatAxis {
    Repeat,
    Space,
    Round,
    NoRepeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundRepeat {
    Repeat,
    NoRepeat,
    RepeatX,
    RepeatY,
    Axes {
        x: BackgroundRepeatAxis,
        y: BackgroundRepeatAxis,
    },
}

impl BackgroundRepeat {
    pub(crate) fn new(x: BackgroundRepeatAxis, y: BackgroundRepeatAxis) -> Self {
        match (x, y) {
            (BackgroundRepeatAxis::Repeat, BackgroundRepeatAxis::Repeat) => Self::Repeat,
            (BackgroundRepeatAxis::NoRepeat, BackgroundRepeatAxis::NoRepeat) => Self::NoRepeat,
            (BackgroundRepeatAxis::Repeat, BackgroundRepeatAxis::NoRepeat) => Self::RepeatX,
            (BackgroundRepeatAxis::NoRepeat, BackgroundRepeatAxis::Repeat) => Self::RepeatY,
            (x, y) => Self::Axes { x, y },
        }
    }

    pub(crate) fn x_axis(self) -> BackgroundRepeatAxis {
        match self {
            Self::Repeat | Self::RepeatX => BackgroundRepeatAxis::Repeat,
            Self::NoRepeat | Self::RepeatY => BackgroundRepeatAxis::NoRepeat,
            Self::Axes { x, .. } => x,
        }
    }

    pub(crate) fn y_axis(self) -> BackgroundRepeatAxis {
        match self {
            Self::Repeat | Self::RepeatY => BackgroundRepeatAxis::Repeat,
            Self::NoRepeat | Self::RepeatX => BackgroundRepeatAxis::NoRepeat,
            Self::Axes { y, .. } => y,
        }
    }

    /// Returns whether the background image repeats on the physical x axis.
    ///
    /// CSS Backgrounds and Borders defines `repeat`, `space`, and `round` as
    /// repeated styles; only `no-repeat` suppresses additional tiles:
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
    pub(crate) fn repeats_x(self) -> bool {
        self.x_axis() != BackgroundRepeatAxis::NoRepeat
    }

    /// Returns whether the background image repeats on the physical y axis.
    ///
    /// CSS Backgrounds and Borders defines `repeat`, `space`, and `round` as
    /// repeated styles; only `no-repeat` suppresses additional tiles:
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
    pub(crate) fn repeats_y(self) -> bool {
        self.y_axis() != BackgroundRepeatAxis::NoRepeat
    }
}

/// Computed physical border colors.
///
/// CSS Backgrounds and Borders defines physical border-color longhands and the
/// `border-color` shorthand:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-color>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BorderColors {
    pub top: CssColor,
    pub right: CssColor,
    pub bottom: CssColor,
    pub left: CssColor,
}

impl BorderColors {
    pub const BLACK: Self = Self {
        top: CssColor::BLACK,
        right: CssColor::BLACK,
        bottom: CssColor::BLACK,
        left: CssColor::BLACK,
    };
}

/// Computed physical border styles.
///
/// CSS Backgrounds and Borders defines physical border-style longhands and the
/// `border-style` shorthand:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-style>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BorderStyles {
    pub top: BorderStyle,
    pub right: BorderStyle,
    pub bottom: BorderStyle,
    pub left: BorderStyle,
}

impl BorderStyles {
    pub const NONE: Self = Self {
        top: BorderStyle::None,
        right: BorderStyle::None,
        bottom: BorderStyle::None,
        left: BorderStyle::None,
    };
}

/// CSS border line style.
///
/// CSS Backgrounds and Borders defines the standardized line styles and makes
/// `none` and `hidden` force the used border width to zero:
/// <https://www.w3.org/TR/css-backgrounds-3/#line-style> and
/// <https://www.w3.org/TR/css-backgrounds-3/#border-width>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorderStyle {
    None,
    Hidden,
    Dotted,
    Dashed,
    Solid,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl BorderStyle {
    /// Returns whether this line style forces a zero used border width.
    ///
    /// CSS Backgrounds and Borders defines `none` and `hidden` as styles whose
    /// used border width is zero:
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-width>.
    pub(crate) fn suppresses_used_width(self) -> bool {
        matches!(self, Self::None | Self::Hidden)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BorderCollapse {
    Separate,
    Collapse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptionSide {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableLayout {
    Auto,
    Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmptyCells {
    Show,
    Hide,
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderSpacing {
    pub horizontal: ComputedLengthPercentage,
    pub vertical: ComputedLengthPercentage,
}

impl BorderSpacing {
    pub(crate) const ZERO: Self = Self {
        horizontal: ComputedLengthPercentage::ZERO,
        vertical: ComputedLengthPercentage::ZERO,
    };

    pub(crate) fn from_lengths(horizontal: f32, vertical: f32) -> Self {
        Self {
            horizontal: ComputedLengthPercentage::from_points(horizontal),
            vertical: ComputedLengthPercentage::from_points(vertical),
        }
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.horizontal.resolve_font_metric_lengths(ch_advance);
        self.vertical.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.horizontal.requires_ch_advance() || self.vertical.requires_ch_advance()
    }

    /// Scale fixed border-spacing components at the CSS `zoom` used-value
    /// boundary.
    ///
    /// Percentage components remain relative to the table's already zoomed
    /// used geometry.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    /// <https://www.w3.org/TR/CSS22/tables.html#separated-borders>
    pub(crate) fn scale_fixed_length_components(&mut self, factor: f32) {
        self.horizontal.scale_fixed_length_components(factor);
        self.vertical.scale_fixed_length_components(factor);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BorderRadius {
    pub top_left: CornerRadius,
    pub top_right: CornerRadius,
    pub bottom_right: CornerRadius,
    pub bottom_left: CornerRadius,
}

impl BorderRadius {
    pub const ZERO: Self = Self {
        top_left: CornerRadius::ZERO,
        top_right: CornerRadius::ZERO,
        bottom_right: CornerRadius::ZERO,
        bottom_left: CornerRadius::ZERO,
    };

    pub(crate) fn is_zero(&self) -> bool {
        self.top_left.is_zero()
            && self.top_right.is_zero()
            && self.bottom_right.is_zero()
            && self.bottom_left.is_zero()
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.top_left.resolve_font_metric_lengths(ch_advance);
        self.top_right.resolve_font_metric_lengths(ch_advance);
        self.bottom_right.resolve_font_metric_lengths(ch_advance);
        self.bottom_left.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.top_left.requires_ch_advance()
            || self.top_right.requires_ch_advance()
            || self.bottom_right.requires_ch_advance()
            || self.bottom_left.requires_ch_advance()
    }
}

/// Superellipse corner-shape parameter from CSS Borders and Box Decorations
/// Level 4.
///
/// CSS Borders and Box Decorations Level 4 defines `corner-*-shape` in terms
/// of `superellipse()` parameters, with keyword aliases for common values:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SuperellipseParameter {
    NegativeInfinity,
    Number(f32),
    Infinity,
}

impl SuperellipseParameter {
    pub(crate) const ROUND: Self = Self::Number(1.0);
    pub(crate) const SQUIRCLE: Self = Self::Number(2.0);
    pub(crate) const BEVEL: Self = Self::Number(0.0);
    pub(crate) const SCOOP: Self = Self::Number(-1.0);
}

/// Per-corner shape from CSS Borders and Box Decorations Level 4.
///
/// The `corner-*-shape` properties define how the border contour connects the
/// two radius tangent points for a corner:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CornerShape {
    pub(crate) superellipse: SuperellipseParameter,
}

impl CornerShape {
    pub(crate) const ROUND: Self = Self {
        superellipse: SuperellipseParameter::ROUND,
    };
    pub(crate) const SQUIRCLE: Self = Self {
        superellipse: SuperellipseParameter::SQUIRCLE,
    };
    pub(crate) const SQUARE: Self = Self {
        superellipse: SuperellipseParameter::Infinity,
    };
    pub(crate) const BEVEL: Self = Self {
        superellipse: SuperellipseParameter::BEVEL,
    };
    pub(crate) const SCOOP: Self = Self {
        superellipse: SuperellipseParameter::SCOOP,
    };
    pub(crate) const NOTCH: Self = Self {
        superellipse: SuperellipseParameter::NegativeInfinity,
    };

    pub(crate) const fn superellipse(parameter: SuperellipseParameter) -> Self {
        Self {
            superellipse: parameter,
        }
    }

    pub(crate) fn is_round(self) -> bool {
        self.superellipse == SuperellipseParameter::ROUND
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CornerShapes {
    pub(crate) top_left: CornerShape,
    pub(crate) top_right: CornerShape,
    pub(crate) bottom_right: CornerShape,
    pub(crate) bottom_left: CornerShape,
}

impl CornerShapes {
    pub(crate) const ROUND: Self = Self {
        top_left: CornerShape::ROUND,
        top_right: CornerShape::ROUND,
        bottom_right: CornerShape::ROUND,
        bottom_left: CornerShape::ROUND,
    };

    pub(crate) fn all_round(self) -> bool {
        self.top_left.is_round()
            && self.top_right.is_round()
            && self.bottom_right.is_round()
            && self.bottom_left.is_round()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CornerRadius {
    pub x: CssRadius,
    pub y: CssRadius,
}

impl CornerRadius {
    pub const ZERO: Self = Self {
        x: CssRadius::ZERO,
        y: CssRadius::ZERO,
    };

    pub(crate) fn is_zero(&self) -> bool {
        self.x.is_zero() && self.y.is_zero()
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.x.resolve_font_metric_lengths(ch_advance);
        self.y.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.x.requires_ch_advance() || self.y.requires_ch_advance()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CssRadius {
    pub value: ComputedLengthPercentage,
}

impl CssRadius {
    pub const ZERO: Self = Self {
        value: ComputedLengthPercentage::ZERO,
    };

    pub(crate) fn is_zero(&self) -> bool {
        self.value == ComputedLengthPercentage::ZERO
    }

    pub(crate) fn resolve(self, basis: PercentageBasis<LayoutLength>) -> LayoutLength {
        self.value
            .used_length_with_percentage_basis(basis)
            .unwrap_or_else(|| layout_pt(self.value.length_points()))
            .max(layout_pt(0.0))
    }

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.value.resolve_font_metric_lengths(ch_advance);
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.value.requires_ch_advance()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

impl FlexDirection {
    /// Returns whether the flex container's main axis is the inline/row axis.
    ///
    /// CSS Flexbox defines `row` and `row-reverse` as opposite directions on
    /// the same main axis:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>.
    pub(crate) fn is_row_axis(self) -> bool {
        matches!(self, Self::Row | Self::RowReverse)
    }

    /// Returns whether the flex container's main axis is the block/column axis.
    ///
    /// CSS Flexbox defines `column` and `column-reverse` as opposite directions
    /// on the same main axis:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>.
    pub(crate) fn is_column_axis(self) -> bool {
        matches!(self, Self::Column | Self::ColumnReverse)
    }

    /// Returns whether two flex-direction values share the same physical axis.
    ///
    /// CSS Flexbox reverses item order for `*-reverse` values without changing
    /// which physical size is the main size:
    /// <https://www.w3.org/TR/css-flexbox-1/#flow-order>.
    pub(crate) fn shares_axis_with(self, other: Self) -> bool {
        (self.is_row_axis() && other.is_row_axis())
            || (self.is_column_axis() && other.is_column_axis())
    }
}

/// Overflow-position modifier for CSS Box Alignment values.
///
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
    /// CSS Flexbox Level 2 balanced wrapping.
    ///
    /// `balance` is a wrapping mode, rather than an alignment distribution:
    /// <https://drafts.csswg.org/css-flexbox-2/#flex-wrap-property>.
    Balance,
    /// Balanced wrapping with cross-axis reversal.
    BalanceReverse,
}

impl FlexWrap {
    pub(crate) const fn wraps(self) -> bool {
        !matches!(self, Self::NoWrap)
    }

    pub(crate) const fn reverses_cross_axis(self) -> bool {
        matches!(self, Self::WrapReverse | Self::BalanceReverse)
    }

    pub(crate) const fn balances_lines(self) -> bool {
        matches!(self, Self::Balance | Self::BalanceReverse)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextAlign {
    Start,
    End,
    Left,
    Center,
    Right,
    Justify,
    JustifyAll,
}

impl TextAlign {
    /// Resolves logical `text-align` keywords to physical alignment.
    ///
    /// CSS Text defines `start` and `end` relative to the inline base
    /// direction of the block container:
    /// <https://www.w3.org/TR/css-text-3/#text-align-property>.
    pub(crate) fn physical(self, direction: Direction) -> Self {
        match self {
            Self::Start => logical_start_align(direction),
            Self::End => logical_end_align(direction),
            align => align,
        }
    }

    /// Return whether this value distributes inline content to fill the line.
    ///
    /// CSS Text defines both `justify` and `justify-all` as justification
    /// values. `justify-all` additionally affects the last line through
    /// `text-align-last: auto`:
    /// <https://www.w3.org/TR/css-text-3/#text-align-property>.
    pub(crate) fn justifies(self) -> bool {
        matches!(self, Self::Justify | Self::JustifyAll)
    }
}

/// Computed CSS `tab-size` value.
///
/// CSS Text Level 3 defines preserved tab advances as periodic tab stops,
/// initially every 8 spaces. Numeric values are resolved from the selected
/// font's U+0020 advance, while length values are already computed CSS layout
/// lengths:
/// <https://www.w3.org/TR/css-text-3/#tab-size-property>.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TabSize {
    Spaces(f32),
    Length(ComputedLengthPercentage),
}

impl TabSize {
    pub(crate) const INITIAL: Self = Self::Spaces(8.0);

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        if let Self::Length(length) = self {
            length.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn requires_ch_advance(&self) -> bool {
        matches!(self, Self::Length(length) if length.requires_ch_advance())
    }

    pub(crate) fn used_tab_stop_advance(&self, space_advance: f32) -> LayoutLength {
        match self {
            Self::Spaces(columns) => layout_pt(*columns * space_advance),
            Self::Length(length) => length.fixed_component(),
        }
        .max(layout_pt(0.0))
    }
}

/// Computed CSS `text-align-last`.
///
/// CSS Text defines `text-align-last` as the alignment used for the last line
/// of a block or a line before a forced break; `auto` defers to
/// `text-align`, except that `text-align: justify` falls back to logical
/// start for the affected line, while `justify-all` keeps final-line
/// justification:
/// <https://www.w3.org/TR/css-text-3/#text-align-last-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextAlignLast {
    Auto,
    Align(TextAlign),
}

/// Computed CSS `text-justify`.
///
/// CSS Text defines the justification method used when `text-align: justify`
/// distributes remaining inline space:
/// <https://www.w3.org/TR/css-text-3/#text-justify-property>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextJustify {
    Auto,
    InterWord,
    InterCharacter,
    None,
}

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
    /// Quire's deterministic user-agent policy for the spec-defined `auto`
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

pub(crate) fn logical_start_align(direction: Direction) -> TextAlign {
    match direction {
        Direction::Ltr => TextAlign::Left,
        Direction::Rtl => TextAlign::Right,
    }
}

pub(crate) fn logical_end_align(direction: Direction) -> TextAlign {
    match direction {
        Direction::Ltr => TextAlign::Right,
        Direction::Rtl => TextAlign::Left,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BaselineMetric {
    TextBottom,
    Alphabetic,
    Ideographic,
    Middle,
    Central,
    Mathematical,
    Hanging,
    TextTop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DominantBaseline {
    Auto,
    Metric(BaselineMetric),
}

impl ResolveViewportLengths for BorderSpacing {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.horizontal.resolve_viewport_lengths(basis);
        self.vertical.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for BorderRadius {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.top_left.resolve_viewport_lengths(basis);
        self.top_right.resolve_viewport_lengths(basis);
        self.bottom_right.resolve_viewport_lengths(basis);
        self.bottom_left.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for CornerRadius {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.x.resolve_viewport_lengths(basis);
        self.y.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for CssRadius {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.value.resolve_viewport_lengths(basis);
    }
}

impl ResolveViewportLengths for TabSize {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        if let Self::Length(length) = self {
            length.resolve_viewport_lengths(basis);
        }
    }
}
