use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackgroundRepeat {
    Repeat,
    NoRepeat,
    RepeatX,
    RepeatY,
}

impl BackgroundRepeat {
    /// Returns whether the background image repeats on the physical x axis.
    ///
    /// CSS Backgrounds and Borders defines `repeat-x` as `repeat no-repeat`
    /// and `repeat-y` as `no-repeat repeat`:
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
    pub(crate) fn repeats_x(self) -> bool {
        matches!(self, Self::Repeat | Self::RepeatX)
    }

    /// Returns whether the background image repeats on the physical y axis.
    ///
    /// CSS Backgrounds and Borders defines the two-axis repeat model used by
    /// `background-repeat`:
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
    pub(crate) fn repeats_y(self) -> bool {
        matches!(self, Self::Repeat | Self::RepeatY)
    }
}

/// Computed physical border colors.
///
/// CSS Backgrounds and Borders defines physical border-color longhands and the
/// `border-color` shorthand:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-color>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BorderColors {
    pub top: Color,
    pub right: Color,
    pub bottom: Color,
    pub left: Color,
}

impl BorderColors {
    pub const BLACK: Self = Self {
        top: Color::BLACK,
        right: Color::BLACK,
        bottom: Color::BLACK,
        left: Color::BLACK,
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
/// CSS Writing Modes maps block and inline axes to physical axes through
/// `writing-mode`; Reasyprint currently uses this for logical border
/// resolution before broader vertical layout support exists:
/// <https://www.w3.org/TR/css-writing-modes-4/#writing-mode>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WritingMode {
    HorizontalTb,
    VerticalRl,
    VerticalLr,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BorderSpacing {
    pub horizontal: f32,
    pub vertical: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
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

    pub(crate) fn is_zero(self) -> bool {
        self.top_left.is_zero()
            && self.top_right.is_zero()
            && self.bottom_right.is_zero()
            && self.bottom_left.is_zero()
    }
}

/// Per-corner shape keywords from CSS Borders and Box Decorations Level 4.
///
/// The `corner-*-shape` properties define how the border contour connects the
/// two radius tangent points for a corner:
/// <https://drafts.csswg.org/css-borders-4/#corner-shape>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CornerShape {
    Round,
    Bevel,
    Scoop,
    Notch,
}

impl CornerShape {
    pub(crate) const fn is_round(self) -> bool {
        matches!(self, Self::Round)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CornerShapes {
    pub(crate) top_left: CornerShape,
    pub(crate) top_right: CornerShape,
    pub(crate) bottom_right: CornerShape,
    pub(crate) bottom_left: CornerShape,
}

impl CornerShapes {
    pub(crate) const ROUND: Self = Self {
        top_left: CornerShape::Round,
        top_right: CornerShape::Round,
        bottom_right: CornerShape::Round,
        bottom_left: CornerShape::Round,
    };

    pub(crate) const fn all_round(self) -> bool {
        self.top_left.is_round()
            && self.top_right.is_round()
            && self.bottom_right.is_round()
            && self.bottom_left.is_round()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CornerRadius {
    pub x: CssRadius,
    pub y: CssRadius,
}

impl CornerRadius {
    pub const ZERO: Self = Self {
        x: CssRadius::ZERO,
        y: CssRadius::ZERO,
    };

    pub(crate) fn is_zero(self) -> bool {
        self.x.is_zero() && self.y.is_zero()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssRadius {
    pub length: f32,
    pub percent: f32,
}

impl CssRadius {
    pub const ZERO: Self = Self {
        length: 0.0,
        percent: 0.0,
    };

    pub(crate) fn is_zero(self) -> bool {
        self.length == 0.0 && self.percent == 0.0
    }

    pub(crate) fn resolve(self, basis: f32) -> f32 {
        (self.length + basis * self.percent).max(0.0)
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

#[allow(dead_code)]
impl ContentAlignment {
    pub const NORMAL: Self = Self::new(ContentAlignmentKeyword::Normal);
    pub const START: Self = Self::new(ContentAlignmentKeyword::Start);
    pub const END: Self = Self::new(ContentAlignmentKeyword::End);
    pub const FLEX_START: Self = Self::new(ContentAlignmentKeyword::FlexStart);
    pub const FLEX_END: Self = Self::new(ContentAlignmentKeyword::FlexEnd);
    pub const LEFT: Self = Self::new(ContentAlignmentKeyword::Left);
    pub const RIGHT: Self = Self::new(ContentAlignmentKeyword::Right);
    pub const CENTER: Self = Self::new(ContentAlignmentKeyword::Center);
    pub const STRETCH: Self = Self::new(ContentAlignmentKeyword::Stretch);
    pub const SPACE_BETWEEN: Self = Self::new(ContentAlignmentKeyword::SpaceBetween);
    pub const SPACE_AROUND: Self = Self::new(ContentAlignmentKeyword::SpaceAround);
    pub const SPACE_EVENLY: Self = Self::new(ContentAlignmentKeyword::SpaceEvenly);
    pub const BASELINE: Self = Self::new(ContentAlignmentKeyword::Baseline);
    pub const LAST_BASELINE: Self = Self::new(ContentAlignmentKeyword::LastBaseline);

    pub const fn new(keyword: ContentAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Unsafe,
        }
    }

    pub const fn safe(keyword: ContentAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Safe,
        }
    }

    #[allow(non_upper_case_globals)]
    pub const Normal: Self = Self::NORMAL;
    #[allow(non_upper_case_globals)]
    pub const Start: Self = Self::START;
    #[allow(non_upper_case_globals)]
    pub const End: Self = Self::END;
    #[allow(non_upper_case_globals)]
    pub const FlexStart: Self = Self::FLEX_START;
    #[allow(non_upper_case_globals)]
    pub const FlexEnd: Self = Self::FLEX_END;
    #[allow(non_upper_case_globals)]
    pub const Left: Self = Self::LEFT;
    #[allow(non_upper_case_globals)]
    pub const Right: Self = Self::RIGHT;
    #[allow(non_upper_case_globals)]
    pub const Center: Self = Self::CENTER;
    #[allow(non_upper_case_globals)]
    pub const Stretch: Self = Self::STRETCH;
    #[allow(non_upper_case_globals)]
    pub const SpaceBetween: Self = Self::SPACE_BETWEEN;
    #[allow(non_upper_case_globals)]
    pub const SpaceAround: Self = Self::SPACE_AROUND;
    #[allow(non_upper_case_globals)]
    pub const SpaceEvenly: Self = Self::SPACE_EVENLY;
    #[allow(non_upper_case_globals)]
    pub const Baseline: Self = Self::BASELINE;
    #[allow(non_upper_case_globals)]
    pub const LastBaseline: Self = Self::LAST_BASELINE;
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

#[allow(dead_code)]
impl SelfAlignment {
    pub const AUTO: Self = Self::new(SelfAlignmentKeyword::Auto);
    pub const NORMAL: Self = Self::new(SelfAlignmentKeyword::Normal);
    pub const START: Self = Self::new(SelfAlignmentKeyword::Start);
    pub const END: Self = Self::new(SelfAlignmentKeyword::End);
    pub const SELF_START: Self = Self::new(SelfAlignmentKeyword::SelfStart);
    pub const SELF_END: Self = Self::new(SelfAlignmentKeyword::SelfEnd);
    pub const FLEX_START: Self = Self::new(SelfAlignmentKeyword::FlexStart);
    pub const FLEX_END: Self = Self::new(SelfAlignmentKeyword::FlexEnd);
    pub const LEFT: Self = Self::new(SelfAlignmentKeyword::Left);
    pub const RIGHT: Self = Self::new(SelfAlignmentKeyword::Right);
    pub const CENTER: Self = Self::new(SelfAlignmentKeyword::Center);
    pub const STRETCH: Self = Self::new(SelfAlignmentKeyword::Stretch);
    pub const BASELINE: Self = Self::new(SelfAlignmentKeyword::Baseline);
    pub const LAST_BASELINE: Self = Self::new(SelfAlignmentKeyword::LastBaseline);

    pub const fn new(keyword: SelfAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Unsafe,
        }
    }

    pub const fn safe(keyword: SelfAlignmentKeyword) -> Self {
        Self {
            keyword,
            safety: AlignmentSafety::Safe,
        }
    }

    #[allow(non_upper_case_globals)]
    pub const Auto: Self = Self::AUTO;
    #[allow(non_upper_case_globals)]
    pub const Normal: Self = Self::NORMAL;
    #[allow(non_upper_case_globals)]
    pub const Start: Self = Self::START;
    #[allow(non_upper_case_globals)]
    pub const End: Self = Self::END;
    #[allow(non_upper_case_globals)]
    pub const SelfStart: Self = Self::SELF_START;
    #[allow(non_upper_case_globals)]
    pub const SelfEnd: Self = Self::SELF_END;
    #[allow(non_upper_case_globals)]
    pub const FlexStart: Self = Self::FLEX_START;
    #[allow(non_upper_case_globals)]
    pub const FlexEnd: Self = Self::FLEX_END;
    #[allow(non_upper_case_globals)]
    pub const Left: Self = Self::LEFT;
    #[allow(non_upper_case_globals)]
    pub const Right: Self = Self::RIGHT;
    #[allow(non_upper_case_globals)]
    pub const Center: Self = Self::CENTER;
    #[allow(non_upper_case_globals)]
    pub const Stretch: Self = Self::STRETCH;
    #[allow(non_upper_case_globals)]
    pub const Baseline: Self = Self::BASELINE;
    #[allow(non_upper_case_globals)]
    pub const LastBaseline: Self = Self::LAST_BASELINE;
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TabSize {
    Spaces(f32),
    Length(ComputedLengthPercentage),
}

impl TabSize {
    pub(crate) const INITIAL: Self = Self::Spaces(8.0);

    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: f32) {
        if let Self::Length(length) = self {
            length.resolve_font_metric_lengths(ch_advance);
        }
    }

    pub(crate) fn resolve_viewport_lengths(
        &mut self,
        viewport_width: f32,
        viewport_height: f32,
        viewport_inline: f32,
        viewport_block: f32,
    ) {
        if let Self::Length(length) = self {
            length.resolve_viewport_lengths(
                viewport_width,
                viewport_height,
                viewport_inline,
                viewport_block,
            );
        }
    }

    pub(crate) fn used_tab_stop_advance(self, space_advance: f32) -> f32 {
        match self {
            Self::Spaces(columns) => columns * space_advance,
            Self::Length(length) => length.length,
        }
        .max(0.0)
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

impl TextAlignLast {
    pub(crate) fn effective(self, text_align: TextAlign, direction: Direction) -> TextAlign {
        match self {
            Self::Align(align) => align,
            Self::Auto => match text_align {
                TextAlign::Justify => logical_start_align(direction),
                TextAlign::JustifyAll => TextAlign::Justify,
                align => align,
            },
        }
    }
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
pub(crate) enum VerticalAlign {
    Baseline,
    Sub,
    Super,
    Top,
    Middle,
    Bottom,
    TextTop,
    TextBottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WhiteSpace {
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
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

    pub(crate) fn allows_soft_wrap(self) -> bool {
        !matches!(self, Self::NoWrap | Self::Pre)
    }

    pub(crate) fn preserves_space_edges(self) -> bool {
        matches!(self, Self::Pre | Self::PreWrap | Self::BreakSpaces)
    }

    /// Return whether trailing Unicode space separators hang at line end.
    ///
    /// CSS Text white-space processing makes trailing "other space
    /// separators" hang for `normal`, `nowrap`, and `pre-line`; preserved
    /// modes keep their own edge-space behavior, and `break-spaces` explicitly
    /// prevents this hanging:
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
    pub(crate) fn hangs_trailing_space_separators(self) -> bool {
        matches!(self, Self::Normal | Self::NoWrap | Self::PreLine)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordBreak {
    Normal,
    BreakAll,
    KeepAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowWrap {
    Normal,
    Anywhere,
    BreakWord,
}

/// Computed CSS `overflow` value.
///
/// CSS Overflow defines `overflow` as a shorthand controlling whether content
/// that extends past the padding box is visible, clipped, or scrollable:
/// <https://www.w3.org/TR/css-overflow-3/#propdef-overflow>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Overflow {
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

impl Overflow {
    pub(crate) fn clips_overflow(self) -> bool {
        !matches!(self, Self::Visible)
    }

    /// Returns whether this computed overflow value is scrollable.
    ///
    /// CSS Overflow classifies `hidden`, `scroll`, and `auto` as scrollable
    /// overflow values, while `visible` and `clip` are non-scrollable. CSS
    /// Flexbox uses that distinction when resolving automatic minimum sizes
    /// for flex items:
    /// <https://www.w3.org/TR/css-overflow-3/#overflow-properties> and
    /// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
    pub(crate) fn is_scrollable(self) -> bool {
        matches!(self, Self::Hidden | Self::Scroll | Self::Auto)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineBreak {
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hyphens {
    None,
    Manual,
    Auto,
}

/// Computed value of CSS `hyphenate-limit-chars`.
///
/// CSS Text defines this as total word characters, characters before the
/// hyphenation break, and characters after the break. `auto` values are
/// user-agent defined; this renderer uses the CSS Text examples' conventional
/// defaults of 5/2/2:
/// <https://www.w3.org/TR/css-text-4/#hyphenate-limit-chars>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HyphenateLimitChars {
    pub(crate) total: u16,
    pub(crate) before: u16,
    pub(crate) after: u16,
}

impl HyphenateLimitChars {
    pub(crate) const AUTO_TOTAL: u16 = 5;
    pub(crate) const AUTO_BEFORE: u16 = 2;
    pub(crate) const AUTO_AFTER: u16 = 2;

    pub(crate) const AUTO: Self = Self {
        total: Self::AUTO_TOTAL,
        before: Self::AUTO_BEFORE,
        after: Self::AUTO_AFTER,
    };
}
