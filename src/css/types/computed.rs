use std::num::NonZeroUsize;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::css::cascade::ModeledLonghandSet;
use crate::units::{ContentBoxLength, LayoutLength, LayoutSize};

macro_rules! non_negative_css_factor {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
        pub(crate) struct $name(f32);

        #[allow(dead_code)]
        impl $name {
            pub(crate) const ZERO: Self = Self(0.0);
            pub(crate) const ONE: Self = Self(1.0);

            pub(crate) fn new(value: f32) -> Option<Self> {
                (value >= 0.0 && !value.is_nan()).then_some(Self(value))
            }

            pub(crate) const fn value(self) -> f32 {
                self.0
            }

            pub(crate) const fn is_infinite(self) -> bool {
                self.0.is_infinite()
            }
        }

        impl PartialEq<f32> for $name {
            fn eq(&self, other: &f32) -> bool {
                self.0 == *other
            }
        }

        impl PartialOrd<f32> for $name {
            fn partial_cmp(&self, other: &f32) -> Option<std::cmp::Ordering> {
                self.0.partial_cmp(other)
            }
        }
    };
}

non_negative_css_factor!(
    FlexGrowFactor,
    "A non-negative CSS `flex-grow` factor, including CSS infinity."
);
non_negative_css_factor!(
    FlexShrinkFactor,
    "A non-negative CSS `flex-shrink` factor, including CSS infinity."
);

macro_rules! closed_unit_interval {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
        pub(crate) struct $name(f32);

        #[allow(dead_code)]
        impl $name {
            pub(crate) const ZERO: Self = Self(0.0);
            pub(crate) const ONE: Self = Self(1.0);

            pub(crate) fn new_clamped(value: f32) -> Option<Self> {
                value.is_finite().then_some(Self(value.clamp(0.0, 1.0)))
            }

            pub(crate) const fn value(self) -> f32 {
                self.0
            }
        }

        impl PartialEq<f32> for $name {
            fn eq(&self, other: &f32) -> bool {
                self.0 == *other
            }
        }
    };
}

closed_unit_interval!(Opacity, "The closed-unit-interval computed CSS opacity.");
closed_unit_interval!(
    ShapeImageThreshold,
    "The closed-unit-interval computed CSS Shapes image threshold."
);

macro_rules! nonzero_line_count {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub(crate) struct $name(NonZeroUsize);

        #[allow(dead_code)]
        impl $name {
            pub(crate) const TWO: Self = Self(NonZeroUsize::new(2).unwrap());

            pub(crate) const fn new(value: NonZeroUsize) -> Self {
                Self(value)
            }

            pub(crate) fn try_new(value: usize) -> Option<Self> {
                NonZeroUsize::new(value).map(Self)
            }

            pub(crate) const fn get(self) -> usize {
                self.0.get()
            }
        }

        impl PartialEq<usize> for $name {
            fn eq(&self, other: &usize) -> bool {
                self.get() == *other
            }
        }
    };
}

nonzero_line_count!(Orphans, "The nonzero computed CSS `orphans` line count.");
nonzero_line_count!(Widows, "The nonzero computed CSS `widows` line count.");

/// Computed `background-color`, retaining symbolic forms until the owning
/// element's foreground color is known.
/// <https://www.w3.org/TR/css-color-4/#resolving-other-colors>
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BackgroundColor {
    Color(CssColor),
    CurrentColor,
    RelativeCurrentColor {
        expression: String,
        used_color_scheme: UsedColorScheme,
    },
}

/// The complete computed background state.
///
/// Image layers carry the complete list-valued paint representation. The
/// first-layer longhand view is retained while declarations cascade in
/// arbitrary source order and for backend boundaries still being migrated.
/// <https://www.w3.org/TR/css-backgrounds-3/#layering>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Background {
    pub(crate) background_color: BackgroundColor,
    pub(crate) background_image: ComputedImage,
    pub(crate) background_size: BackgroundSize,
    pub(crate) background_position: BackgroundPosition,
    pub(crate) background_repeat: BackgroundRepeat,
    pub(crate) background_attachment: BackgroundAttachment,
    pub(crate) background_origin: BackgroundBox,
    pub(crate) background_clip: BackgroundBox,
    pub(crate) background_layers: Vec<BackgroundLayer>,
    /// This remains private to cascading: a `background-image: none` still
    /// establishes one layer even before a final layer vector is materialized.
    pub(crate) background_image_layer_count: usize,
}

impl Background {
    pub(crate) fn initial() -> Self {
        Self {
            background_color: BackgroundColor::TRANSPARENT,
            background_image: ComputedImage::None,
            background_size: BackgroundSize::AUTO,
            background_position: BackgroundPosition::INITIAL,
            background_repeat: BackgroundRepeat::Repeat,
            background_attachment: BackgroundAttachment::Scroll,
            background_origin: BackgroundBox::Padding,
            background_clip: BackgroundBox::Border,
            background_layers: Vec::new(),
            background_image_layer_count: 1,
        }
    }

    /// Return the clip edge used by the color painted below all image layers.
    pub(crate) fn color_clip(&self) -> BackgroundBox {
        self.background_layers
            .last()
            .map(|layer| layer.clip)
            .unwrap_or(self.background_clip)
    }
}

impl BackgroundColor {
    pub(crate) const TRANSPARENT: Self = Self::Color(CssColor::TRANSPARENT);

    pub(crate) fn resolved_color(&self, current_color: CssColor) -> CssColor {
        match self {
            Self::Color(color) => *color,
            Self::CurrentColor => current_color,
            Self::RelativeCurrentColor {
                expression,
                used_color_scheme,
            } => crate::css::parse_color_from_currentcolor_in_scheme(
                expression,
                current_color,
                *used_color_scheme,
            )
            .unwrap_or(current_color),
        }
    }

    pub(crate) fn visible_color(&self, current_color: CssColor) -> Option<CssColor> {
        let color = self.resolved_color(current_color);
        color.is_visible().then_some(color)
    }

    pub(crate) fn is_transparent(&self) -> bool {
        matches!(self, Self::Color(color) if !color.is_visible())
    }

    #[cfg(test)]
    pub(crate) const fn color(&self) -> Option<CssColor> {
        match self {
            Self::Color(color) => Some(*color),
            Self::CurrentColor | Self::RelativeCurrentColor { .. } => None,
        }
    }

    pub(crate) fn is_potentially_visible(&self) -> bool {
        !self.is_transparent()
    }
}

/// A CSS color whose initial or omitted spelling is `currentcolor`.
/// <https://www.w3.org/TR/css-color-4/#currentcolor-color>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CssColorOrCurrentColor {
    CurrentColor,
    Color(CssColor),
}

/// The used SVG filter color together with whether its computed value remains
/// dependent on `currentcolor`.
///
/// Filter Effects tainting is defined from the computed `flood-color` and
/// `lighting-color` values, not from the final RGBA value supplied to the SVG
/// scene parser.  Keep that distinction through the host-CSS bridge.
/// <https://drafts.csswg.org/filter-effects/#tainted-filter-primitives>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SvgFilterColor {
    pub(crate) color: CssColor,
    pub(crate) current_color_dependent: bool,
}

impl SvgFilterColor {
    pub(crate) const fn absolute(color: CssColor) -> Self {
        Self {
            color,
            current_color_dependent: false,
        }
    }
}

impl CssColorOrCurrentColor {
    pub(crate) const fn resolve(self, current_color: CssColor) -> CssColor {
        match self {
            Self::CurrentColor => current_color,
            Self::Color(color) => color,
        }
    }

    pub(crate) const fn unwrap_or(self, current_color: CssColor) -> CssColor {
        self.resolve(current_color)
    }

    /// Transform only a concrete color while retaining the `currentcolor`
    /// dependency for the element or fragment that eventually paints it.
    pub(crate) fn map_concrete(self, map: impl FnOnce(CssColor) -> CssColor) -> Self {
        match self {
            Self::CurrentColor => Self::CurrentColor,
            Self::Color(color) => Self::Color(map(color)),
        }
    }
}

impl PartialEq<CssColor> for CssColorOrCurrentColor {
    fn eq(&self, other: &CssColor) -> bool {
        matches!(self, Self::Color(color) if *color == *other)
    }
}

/// Computed SVG presentation paint.
/// <https://www.w3.org/TR/SVG2/painting.html#SpecifyingPaint>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SvgPaint {
    None,
    Color(CssColor),
    CurrentColor,
}

impl SvgPaint {
    pub(crate) const fn resolve(self, current_color: CssColor) -> Option<CssColor> {
        match self {
            Self::None => None,
            Self::Color(color) => Some(color),
            Self::CurrentColor => Some(current_color),
        }
    }
}

/// Origin of a host-document SVG presentation property.
/// <https://www.w3.org/TR/SVG2/styling.html#PresentationAttributes>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SvgPresentationSource {
    Initial,
    HostCss,
}

/// SVG paint together with the source information required by the inline SVG
/// adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SvgPresentationPaint {
    pub(crate) paint: SvgPaint,
    pub(crate) source: SvgPresentationSource,
}

impl SvgPresentationPaint {
    pub(crate) const fn initial(paint: SvgPaint) -> Self {
        Self {
            paint,
            source: SvgPresentationSource::Initial,
        }
    }

    pub(crate) const fn host_css(paint: SvgPaint) -> Self {
        Self {
            paint,
            source: SvgPresentationSource::HostCss,
        }
    }

    pub(crate) const fn is_overridden(self) -> bool {
        matches!(self.source, SvgPresentationSource::HostCss)
    }
}

/// SVG `stroke-width` and whether it originates from host-document CSS.
/// <https://www.w3.org/TR/SVG2/painting.html#StrokeProperties>
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SvgStrokeWidth {
    Initial(ComputedLengthPercentage),
    HostCss(ComputedLengthPercentage),
}

impl SvgStrokeWidth {
    pub(crate) fn value(&self) -> &ComputedLengthPercentage {
        match self {
            Self::Initial(value) | Self::HostCss(value) => value,
        }
    }

    pub(crate) const fn is_overridden(&self) -> bool {
        matches!(self, Self::HostCss(_))
    }
}

/// Computed value of CSS Ruby's interlinear annotation positioning.
///
/// `ruby-position` inherits through `::first-line`, even though most Ruby
/// properties do not apply to the pseudo-element itself. Keeping the
/// unsupported inter-character form explicit prevents an inter-character
/// declaration from accidentally being rendered as an interlinear one.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-position>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RubyPosition {
    /// Select the writing-mode appropriate default annotation side.
    #[default]
    Alternate,
    Over,
    Under,
    /// Parsed and cascaded, but not yet supported by Ruby layout.
    InterCharacter,
}

/// Computed inline-axis distribution of ruby base and annotation contents.
///
/// This value is inherited so the anonymous ruby containers generated by the
/// formatting model retain the policy of their originating ruby container.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-align-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RubyAlign {
    Start,
    Center,
    SpaceBetween,
    #[default]
    SpaceAround,
}

/// Computed policy for annotation overlap beyond a ruby base container.
///
/// The legacy CSS Ruby value `none` is parsed as [`Self::Spaces`], as required
/// by CSS Ruby Level 1. `Auto` remains distinct because it selects Quire's
/// documented user-agent overhang policy rather than the author-requested
/// space-only rule.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-overhang>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RubyOverhang {
    #[default]
    Auto,
    Spaces,
}

/// A resolved physical relationship between an interlinear annotation and its
/// base level. This is a layout-time fact, deliberately separate from the
/// inherited [`RubyPosition`] CSS value.
/// <https://drafts.csswg.org/css-ruby-1/#ruby-position>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RubyAnnotationSide {
    Over,
    Under,
}

impl RubyPosition {
    /// Resolve the supported interlinear forms. `inter-character` remains
    /// visibly equivalent to the initial interlinear placement until its
    /// distinct layout model is implemented.
    pub(crate) const fn interlinear_side(self) -> RubyAnnotationSide {
        match self {
            Self::Under => RubyAnnotationSide::Under,
            Self::Alternate | Self::Over | Self::InterCharacter => RubyAnnotationSide::Over,
        }
    }
}

/// The maximum number of line boxes a continuation mode may use.
///
/// Keeping `none` distinct from a positive count prevents an unlimited
/// computed value from accidentally becoming a zero-line used clamp.
/// <https://drafts.csswg.org/css-overflow-4/#max-lines>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MaxLines {
    #[default]
    None,
    Lines(NonZeroUsize),
}

/// A positive CSS line count.
///
/// This is deliberately not a type alias: layout may represent an exhausted
/// budget, but never an available budget of zero lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PositiveLineCount(NonZeroUsize);

impl PositiveLineCount {
    pub(crate) const fn new(value: NonZeroUsize) -> Self {
        Self(value)
    }

    pub(crate) const fn get(self) -> usize {
        self.0.get()
    }

    pub(crate) fn from_rendered_slots(value: usize) -> Option<Self> {
        NonZeroUsize::new(value).map(Self)
    }
}

impl From<NonZeroUsize> for PositiveLineCount {
    fn from(value: NonZeroUsize) -> Self {
        Self::new(value)
    }
}

/// The marker inserted at a CSS block-overflow point.
///
/// CSS Overflow Level 4 models this independently from the line limit: the
/// marker can be automatic, suppressed, or an authored string.  Keeping that
/// distinction in the computed value prevents layout from treating every
/// clamp as a hard-coded U+2026 insertion.
/// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockEllipsis {
    Auto,
    NoEllipsis,
    String(Arc<str>),
}

/// A block-overflow marker that can actually be placed in a line box.
///
/// `no-ellipsis` and the empty string have no inhabitant here, so a marker
/// placement cannot accidentally be synthesized for either CSS value.
/// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderableBlockEllipsis<'a> {
    Auto,
    String(&'a str),
}

impl<'a> RenderableBlockEllipsis<'a> {
    pub(crate) const fn text(self) -> &'a str {
        match self {
            Self::Auto => "…",
            Self::String(text) => text,
        }
    }
}

impl BlockEllipsis {
    /// Convert an authored marker to a renderable one only when CSS permits
    /// a block-overflow ellipsis to be inserted.
    pub(crate) fn renderable(&self) -> Option<RenderableBlockEllipsis<'_>> {
        match self {
            Self::Auto => Some(RenderableBlockEllipsis::Auto),
            Self::NoEllipsis => None,
            Self::String(text) if text.is_empty() => None,
            Self::String(text) => Some(RenderableBlockEllipsis::String(text)),
        }
    }
}

/// A terminal line in the same block formatting context as a clamp
/// container. This capability is created only by inline line selection;
/// block-sibling endpoints intentionally cannot create one.
/// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
#[derive(Debug, Clone, Copy)]
pub(crate) struct EligibleMarkerLine {
    inline_line_index: usize,
}

impl EligibleMarkerLine {
    pub(crate) const fn terminal_inline_line(inline_line_index: usize) -> Self {
        Self { inline_line_index }
    }

    pub(crate) const fn inline_line_index(self) -> usize {
        self.inline_line_index
    }
}

/// A block ellipsis paired with the only kind of endpoint at which it may be
/// painted. `no-ellipsis`, empty strings, and block-only boundaries cannot
/// inhabit this type.
/// <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockEllipsisPlacement<'a> {
    pub(crate) line: EligibleMarkerLine,
    pub(crate) marker: RenderableBlockEllipsis<'a>,
}

impl<'a> BlockEllipsisPlacement<'a> {
    pub(crate) fn at_terminal_inline_line(
        line: EligibleMarkerLine,
        marker: &'a BlockEllipsis,
    ) -> Option<Self> {
        marker.renderable().map(|marker| Self { line, marker })
    }
}

/// CSS Overflow's `continue` longhand.
///
/// `WebkitLegacy` is only synthesized by the line-clamp shorthands, but is a
/// real computed value so post-cascade legacy display resolution cannot be
/// declaration-order dependent.
/// <https://drafts.csswg.org/css-overflow-4/#continue>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Continue {
    #[default]
    Auto,
    Collapse,
    Discard,
    WebkitLegacy,
}

/// The cutoff rule of an eligible line-clamp container.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClampPointRule {
    AfterLines(PositiveLineCount),
    AutomaticBlockSize,
}

/// The specified legacy WebKit display spelling, retained after `display`
/// has been parsed to its CSS Display equivalent.
///
/// `-webkit-line-clamp` tests the specified legacy display value, rather than
/// the computed display that a valid clamp subsequently turns into
/// `flow-root`. Keeping this provenance separate prevents declaration-order
/// dependent activation.
/// <https://drafts.csswg.org/css-overflow-4/#webkit-line-clamp>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LegacyWebkitBox {
    #[default]
    None,
    Block,
    Inline,
}

impl LegacyWebkitBox {
    pub(crate) fn from_specified_display(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "-webkit-box" => Self::Block,
            "-webkit-inline-box" => Self::Inline,
            _ => Self::None,
        }
    }

    pub(crate) const fn is_present(self) -> bool {
        !matches!(self, Self::None)
    }

    const fn outer_display(self) -> Option<DisplayOuter> {
        match self {
            Self::None => None,
            Self::Block => Some(DisplayOuter::Block),
            Self::Inline => Some(DisplayOuter::Inline),
        }
    }
}

/// The legacy WebKit box orientation used to enable legacy line clamping.
///
/// The initial `horizontal` value deliberately remains distinct from the
/// physical writing-mode axes: only the authored `vertical` keyword enables
/// the compatibility behavior.
/// <https://drafts.csswg.org/css-overflow-4/#webkit-line-clamp>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WebkitBoxOrient {
    #[default]
    Horizontal,
    Vertical,
}

/// The display role captured before CSS Display blockifies an out-of-flow box.
///
/// Absolute and fixed positioning preserve the source role only for static
/// position and baseline rules. Atomic inline is necessarily inline-level, so
/// this enum prevents those two facts from drifting apart on layout clones.
/// <https://drafts.csswg.org/css-display-3/#transformations>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum StaticPositionSource {
    #[default]
    BlockLevel,
    Inline,
    AtomicInline(DisplayInner),
}

impl StaticPositionSource {
    pub(crate) fn from_display(display: Display) -> Self {
        if display.is_atomic_inline() {
            Self::AtomicInline(display.inner)
        } else if display.is_inline_level() {
            Self::Inline
        } else {
            Self::BlockLevel
        }
    }

    pub(crate) const fn is_inline_level(self) -> bool {
        matches!(self, Self::Inline | Self::AtomicInline(_))
    }

    pub(crate) const fn is_atomic_inline(self) -> bool {
        matches!(self, Self::AtomicInline(_))
    }

    pub(crate) const fn atomic_inline_display(self) -> Option<Display> {
        match self {
            Self::AtomicInline(inner) => Some(Display {
                outer: DisplayOuter::Inline,
                inner,
                list_item: false,
            }),
            Self::BlockLevel | Self::Inline => None,
        }
    }
}

/// Layout-only clamp state for one inline/block-flow traversal.
///
/// `max_lines` may be zero only after an ancestor has spent all of its
/// computed non-zero budget. Keeping that state separate from
/// the independently cascaded longhands makes it impossible for traversal and
/// replay to mutate cascaded CSS values.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineLimitTraversal {
    pub(crate) remaining: RemainingLineSlots,
    pub(crate) ellipsis: BlockEllipsis,
    pub(crate) continuation: ClampContinuation,
}

/// A line-limit traversal can be either available with a positive count or
/// exhausted. There is intentionally no `Available(0)` representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemainingLineSlots {
    Available(PositiveLineCount),
    Exhausted,
}

impl RemainingLineSlots {
    pub(crate) const fn visible_line_limit(self) -> usize {
        match self {
            Self::Available(limit) => limit.get(),
            Self::Exhausted => 0,
        }
    }

    pub(crate) fn debit(self, used: PositiveLineCount) -> Self {
        let Self::Available(remaining) = self else {
            return Self::Exhausted;
        };
        let remainder = remaining.get().saturating_sub(used.get());
        match NonZeroUsize::new(remainder) {
            Some(remainder) => Self::Available(PositiveLineCount::from(remainder)),
            None => Self::Exhausted,
        }
    }
}

/// Whether a clamp point after the current traversal has known later
/// in-flow content. Inline overflow is discovered separately from the graph;
/// this value represents only a block-flow boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ClampContinuation {
    #[default]
    None,
    LaterInFlowContent,
}

/// Borrowed clamp view passed to inline layout.
///
/// Inline paragraph contexts are intentionally copyable: line-selection and
/// fragmentation retries pass them by value. This view exposes either a
/// computed declaration or a layout budget without copying an authored
/// custom ellipsis string.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InlineLineClamp<'a> {
    Computed(LineClampContainer<'a>),
    Used(&'a LineLimitTraversal),
    /// A cutoff selected from measured line-box block advances for an
    /// automatic block-size clamp. It retains a source line identity rather
    /// than converting the selection into an invented `max-lines` value.
    Automatic(AutomaticLineClamp<'a>),
}

/// The terminal source line selected by an automatic block-size clamp.
///
/// This is intentionally constructed only by the line-record controller.
/// A raw line count does not convey whether the line's used block advance
/// actually fit the finite clamp container.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
#[derive(Debug, Clone, Copy)]
pub(crate) struct AutomaticLineClamp<'a> {
    cutoff: ClampPoint,
    marker: &'a BlockEllipsis,
}

/// A finite block-axis allowance inherited by an eligible descendant of an
/// automatic line-clamp container.
///
/// This is layout traversal state, not a computed CSS value: the descendant
/// keeps its independently cascaded `max-lines`, `block-ellipsis`, and
/// `continue` longhands.  In particular, this must not be represented as a
/// fabricated numeric `max-lines` budget, because a block boundary may be
/// the selected clamp point and therefore has no eligible marker line.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutomaticBlockSizeTraversal {
    remaining: ContentBoxLength,
    marker: BlockEllipsis,
    terminal_marker_when_full: bool,
}

/// A marker capability carried into a zero-sized in-flow block after an
/// automatic block-boundary cutoff. It has no size allowance: the child may
/// expose its own terminal line, but only that line may host the marker.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutomaticBlockBoundaryMarker(pub(crate) BlockEllipsis);

impl AutomaticBlockSizeTraversal {
    /// Create the layout-only controller at an automatic clamp container's
    /// content-box boundary.
    pub(crate) const fn new(remaining: ContentBoxLength, marker: BlockEllipsis) -> Self {
        Self {
            remaining,
            marker,
            terminal_marker_when_full: false,
        }
    }

    pub(crate) const fn remaining(&self) -> ContentBoxLength {
        self.remaining
    }

    pub(crate) fn marker(&self) -> &BlockEllipsis {
        &self.marker
    }

    /// Debit a rendered normal-flow block contribution.  Saturation records
    /// the legal block-boundary cutoff without manufacturing a zero line
    /// quota; callers test `is_exhausted` before admitting later source.
    pub(crate) fn debit(&mut self, used: ContentBoxLength) {
        self.remaining = ContentBoxLength::new((self.remaining.points() - used.points()).max(0.0));
    }

    pub(crate) fn is_exhausted(&self) -> bool {
        self.remaining.points() <= 0.01
    }

    pub(crate) fn with_terminal_marker_when_full(mut self) -> Self {
        self.terminal_marker_when_full = true;
        self
    }

    pub(crate) const fn terminal_marker_when_full(&self) -> bool {
        self.terminal_marker_when_full
    }
}

/// An opaque source endpoint admitted by a continuation controller.
///
/// Only inline line selection and block-flow traversal construct these
/// values.  This prevents a raw DOM child or line index from being mistaken
/// for a legal overflow endpoint.
/// <https://drafts.csswg.org/css-overflow-4/#valdef-continue-collapse>
#[derive(Debug, Clone, Copy)]
pub(crate) enum ClampPoint {
    AtContainerStart,
    AfterInlineLine(InlineClampPoint),
    BetweenBlockSiblings(BlockClampPoint),
}

/// A same-inline-formatting-context terminal line.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InlineClampPoint {
    block_line_index: usize,
    source_end: Option<InlineSourceEndpoint>,
}

/// An opaque inline-source boundary selected from the line opportunity graph.
///
/// A line ordinal alone is not stable under `text-wrap: balance`: balancing
/// can repartition the same source into different lines. This endpoint keeps
/// the automatic cutoff attached to source rather than to a later reflow's
/// incidental line number.
/// <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InlineSourceEndpoint {
    run_index: usize,
    byte_offset: usize,
}

/// A block-flow source boundary. It is deliberately distinct from an inline
/// line point: a block-only boundary cannot manufacture a block ellipsis.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockClampPoint {
    preceding_in_flow_child_index: usize,
}

impl InlineClampPoint {
    /// Constructed only by the measured inline line selector.
    pub(crate) const fn after_measured_line(block_line_index: usize) -> Self {
        Self {
            block_line_index,
            source_end: None,
        }
    }

    /// Constructed only by a measured inline line whose graph range supplies
    /// the terminal source boundary.
    pub(crate) const fn after_measured_source(
        block_line_index: usize,
        source_end: InlineSourceEndpoint,
    ) -> Self {
        Self {
            block_line_index,
            source_end: Some(source_end),
        }
    }

    pub(crate) const fn block_line_index(self) -> usize {
        self.block_line_index
    }

    pub(crate) const fn source_end(self) -> Option<InlineSourceEndpoint> {
        self.source_end
    }
}

impl InlineSourceEndpoint {
    /// Constructed only at the adapter from a selected inline graph range.
    pub(crate) const fn at_graph_boundary(run_index: usize, byte_offset: usize) -> Self {
        Self {
            run_index,
            byte_offset,
        }
    }

    pub(crate) const fn run_index(self) -> usize {
        self.run_index
    }

    pub(crate) const fn byte_offset(self) -> usize {
        self.byte_offset
    }
}

impl BlockClampPoint {
    /// Constructed only by the block-flow walker at the boundary before a
    /// discarded later sibling.
    pub(crate) const fn after_in_flow_child(preceding_in_flow_child_index: usize) -> Self {
        Self {
            preceding_in_flow_child_index,
        }
    }

    pub(crate) const fn preceding_in_flow_child_index(self) -> usize {
        self.preceding_in_flow_child_index
    }
}

impl<'a> AutomaticLineClamp<'a> {
    pub(crate) const fn at_container_start(marker: &'a BlockEllipsis) -> Self {
        Self {
            cutoff: ClampPoint::AtContainerStart,
            marker,
        }
    }

    pub(crate) const fn after_measured_line(
        block_line_index: usize,
        marker: &'a BlockEllipsis,
    ) -> Self {
        Self {
            cutoff: ClampPoint::AfterInlineLine(InlineClampPoint::after_measured_line(
                block_line_index,
            )),
            marker,
        }
    }

    /// Construct an automatic cutoff from the terminal line and its opaque
    /// graph-source endpoint.
    pub(crate) const fn after_measured_source_line(
        block_line_index: usize,
        source_end: InlineSourceEndpoint,
        marker: &'a BlockEllipsis,
    ) -> Self {
        Self {
            cutoff: ClampPoint::AfterInlineLine(InlineClampPoint::after_measured_source(
                block_line_index,
                source_end,
            )),
            marker,
        }
    }
}

/// Borrowed computed line-clamp fields. This avoids constructing a mutable
/// used value merely to lay out the root formatting context.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LineClampContainer<'a> {
    pub(crate) cutoff: ClampPointRule,
    pub(crate) marker: &'a BlockEllipsis,
}

/// A local Category-3 fragmentation container created by `continue: discard`.
/// It deliberately carries no page/column fragmentainer kind: such a
/// container captures its first region break rather than materializing a
/// destination page.
/// <https://drafts.csswg.org/css-overflow-4/#continue-discard>
#[derive(Debug, Clone, Copy)]
pub(crate) struct DiscardFragmentationContainer<'a> {
    pub(crate) max_lines: MaxLines,
    pub(crate) marker: &'a BlockEllipsis,
}

/// The source endpoint of an unforced local discard-region overflow.
///
/// This is neither a page nor column index. The retained direct-child prefix
/// exists solely so the owning block-flow controller can replay the source
/// that precedes its first Category-3 break.
/// <https://drafts.csswg.org/css-overflow-4/#continue-discard>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegionOverflowPoint {
    retained_direct_children: NonZeroUsize,
}

impl RegionOverflowPoint {
    pub(crate) const fn after_direct_children(retained_direct_children: NonZeroUsize) -> Self {
        Self {
            retained_direct_children,
        }
    }

    pub(crate) const fn retained_direct_children(self) -> NonZeroUsize {
        self.retained_direct_children
    }
}

/// The first local region break captured by `continue: discard`.
///
/// A discard controller captures exactly one Category-3 break and never asks
/// page/column fragmentation to advance. The forced-line case shares the
/// legal clamp-point representation with `continue: collapse`.
/// <https://drafts.csswg.org/css-overflow-4/#continue-discard>
#[derive(Debug, Clone, Copy)]
#[allow(
    dead_code,
    reason = "the current block-flow walker captures unforced local region overflow; the forced max-lines branch is retained for the shared typed controller and exercised by its invariants"
)]
pub(crate) enum CapturedRegionBreak {
    ForcedAfterLines(ClampPoint),
    Overflow(RegionOverflowPoint),
}

/// Mutable local state of one discard fragmentation container.
///
/// `None` means no local break has been captured. Once captured, the first
/// break is immutable: later source is non-rendered rather than becoming a
/// second local region or a page/column continuation.
/// <https://drafts.csswg.org/css-overflow-4/#continue-discard>
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DiscardRegionTraversal {
    first_break: Option<CapturedRegionBreak>,
}

impl DiscardRegionTraversal {
    pub(crate) const fn first_break(self) -> Option<CapturedRegionBreak> {
        self.first_break
    }

    pub(crate) fn capture_overflow(&mut self, point: RegionOverflowPoint) {
        if self.first_break.is_none() {
            self.first_break = Some(CapturedRegionBreak::Overflow(point));
        }
    }

    /// Retain the first max-lines-induced local region break. The caller must
    /// supply a legal continuation endpoint rather than a raw child or line
    /// index.
    pub(crate) fn capture_forced_after_lines(&mut self, point: ClampPoint) {
        if self.first_break.is_none() {
            self.first_break = Some(CapturedRegionBreak::ForcedAfterLines(point));
        }
    }
}

/// The continuation mode after display/legacy resolution. Only this used
/// policy may create a layout cutoff; a `max-lines` declaration by itself is
/// inert.
#[derive(Debug, Clone, Copy)]
pub(crate) enum UsedContinuation<'a> {
    Ordinary,
    LineClamp(LineClampContainer<'a>),
    Discard(DiscardFragmentationContainer<'a>),
}

/// Reference box and signed offset for an overflow clip edge.
///
/// The Level 3 shorthand is deliberately a single value. Future Level 4
/// physical and logical longhands can expand this into per-side values without
/// changing the layout-facing clip-edge abstraction.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-margin>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OverflowClipMargin {
    pub(crate) reference_box: OverflowClipMarginBox,
    pub(crate) offset: LayoutLength,
}

impl OverflowClipMargin {
    pub(crate) const ZERO: Self = Self {
        reference_box: OverflowClipMarginBox::Padding,
        offset: LayoutLength::new(0.0),
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowClipMarginBox {
    Border,
    Padding,
    Content,
}

/// Computed reservation policy for scrollbar gutters.
///
/// The policy is intentionally separate from overflow: layout resolves it
/// only after it knows whether a physical scrollable axis needs a scrollbar.
/// <https://drafts.csswg.org/css-overflow-3/#scrollbar-gutter-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollbarGutter {
    #[default]
    Auto,
    Stable {
        both_edges: bool,
    },
}

/// Author-facing width policy for a scroll container's native scrollbar.
///
/// The actual metric is a user-agent choice.  Keeping `none` distinct means
/// that it can prove a scrollbar reservation is zero before layout starts.
/// <https://drafts.csswg.org/css-scrollbars/#scrollbar-width>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ScrollbarWidth {
    #[default]
    Auto,
    Thin,
    None,
}

impl ComputedStyle {
    /// Rebuild this style's own text-decoration origin after a used-value
    /// resolution boundary.
    ///
    /// Line decorations propagate independently of CSS inheritance, so their
    /// retained origin must own the decorating box's resolved values. In
    /// particular, selected-font metric units such as `ch` cannot remain in a
    /// cascade-time snapshot after the owning style has resolved them.
    /// This preserves any propagated ancestor origins already present on a
    /// used inline style.
    ///
    /// CSS Text Decoration Level 4 § 2 and § 2.9.1; CSS Values Level 4 § 5.1:
    /// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
    /// <https://drafts.csswg.org/css-text-decor-4/#text-decoration-inset-property>
    /// <https://drafts.csswg.org/css-values-4/#font-relative-lengths>
    pub(crate) fn rebuild_own_text_decoration_origin(&mut self) {
        self.text_decoration_origins.clear_own();
        if !self.text_decoration.has_visible_line() {
            return;
        }

        let decoration = self.text_decoration.clone();
        let mut origin_style = self.clone();
        origin_style.text_decoration_origins.clear();
        self.text_decoration_origins.set_own(TextDecorationLayer {
            decoration,
            origin_style: Rc::new(origin_style),
        });
    }

    /// Whether the computed `line-height` retains the CSS `normal` keyword.
    pub(crate) const fn line_height_is_normal(&self) -> bool {
        matches!(&self.line_height_value, ComputedLineHeight::Normal)
    }

    /// Resolve the post-cascade compatibility behavior of legacy WebKit line
    /// clamping.
    ///
    /// The activation condition intentionally runs after every declaration
    /// has been cascaded: `-webkit-line-clamp` observes the specified legacy
    /// display spelling and the final `-webkit-box-orient` value, while an
    /// active clamp computes its layout display to a flow-root container.
    /// <https://drafts.csswg.org/css-overflow-4/#webkit-line-clamp>
    pub(crate) fn resolve_legacy_webkit_line_clamp(&mut self) {
        let legacy_vertical = self.webkit_box_orient == WebkitBoxOrient::Vertical
            && self.legacy_webkit_box.is_present();

        if matches!(
            self.continue_,
            Continue::Collapse | Continue::Discard | Continue::WebkitLegacy
        ) && legacy_vertical
            && let Some(outer) = self.legacy_webkit_box.outer_display()
        {
            // The legacy compatibility algorithm changes the box from the
            // legacy flex model into a block formatting context while keeping
            // the authored outer display role (`-webkit-inline-box` remains
            // inline-level).
            self.display = Display::new(outer, DisplayInner::FlowRoot);
        }
    }

    /// Return the line-count policy that may be consumed by an inline
    /// formatting context. Automatic clamp points and discard-region breaks
    /// intentionally have separate layout controllers and cannot be mistaken
    /// for a numeric line budget.
    pub(crate) fn line_clamp_container(&self) -> Option<LineClampContainer<'_>> {
        match self.used_continuation() {
            UsedContinuation::LineClamp(container)
                if matches!(container.cutoff, ClampPointRule::AfterLines(_)) =>
            {
                Some(container)
            }
            // A forced region break induced by max-lines has the same source
            // endpoint as a line clamp. The discard controller owns what
            // happens after that endpoint; the shared traversal only needs
            // the positive line budget.
            UsedContinuation::Discard(discard) => match discard.max_lines {
                MaxLines::Lines(limit) => Some(LineClampContainer {
                    cutoff: ClampPointRule::AfterLines(PositiveLineCount::from(limit)),
                    marker: discard.marker,
                }),
                MaxLines::None => None,
            },
            UsedContinuation::Ordinary | UsedContinuation::LineClamp(_) => None,
        }
    }

    /// Resolve the three cascaded longhands into their valid layout behavior.
    /// The caller must still verify the box is an eligible block container.
    pub(crate) fn used_continuation(&self) -> UsedContinuation<'_> {
        let marker = &self.block_ellipsis;
        match self.continue_ {
            Continue::Auto => UsedContinuation::Ordinary,
            Continue::WebkitLegacy
                if self.webkit_box_orient != WebkitBoxOrient::Vertical
                    || !self.legacy_webkit_box.is_present() =>
            {
                // `-webkit-legacy` is a used-value compatibility behavior,
                // not a computed-value rewrite.  Retaining the cascaded
                // longhand lets later layout clones and inheritance-sensitive
                // diagnostics observe the authored state faithfully.
                UsedContinuation::Ordinary
            }
            Continue::Collapse | Continue::WebkitLegacy => {
                let cutoff = match self.max_lines {
                    MaxLines::None => ClampPointRule::AutomaticBlockSize,
                    MaxLines::Lines(limit) => {
                        ClampPointRule::AfterLines(PositiveLineCount::from(limit))
                    }
                };
                UsedContinuation::LineClamp(LineClampContainer { cutoff, marker })
            }
            Continue::Discard => UsedContinuation::Discard(DiscardFragmentationContainer {
                max_lines: self.max_lines,
                marker,
            }),
        }
    }
}

impl LineLimitTraversal {
    pub(crate) fn from_container(container: LineClampContainer<'_>) -> Self {
        Self {
            remaining: RemainingLineSlots::Available(match container.cutoff {
                ClampPointRule::AfterLines(limit) => limit,
                ClampPointRule::AutomaticBlockSize => {
                    unreachable!("automatic clamp points do not create line-limit traversal")
                }
            }),
            ellipsis: container.marker.clone(),
            continuation: ClampContinuation::None,
        }
    }

    pub(crate) fn with_remaining(&self, remaining: RemainingLineSlots) -> Self {
        Self {
            remaining,
            ellipsis: self.ellipsis.clone(),
            continuation: self.continuation,
        }
    }

    /// Attach a traversal fact without mutating the cascaded declaration or
    /// the line budget inherited by a descendant.
    pub(crate) fn with_continuation(&self, continuation: ClampContinuation) -> Self {
        Self {
            remaining: self.remaining,
            ellipsis: self.ellipsis.clone(),
            continuation,
        }
    }
}

impl<'a> InlineLineClamp<'a> {
    /// Return the stable graph-source cutoff of an automatic clamp, when one
    /// was selected from a measured inline line.
    pub(crate) const fn inline_source_end(self) -> Option<InlineSourceEndpoint> {
        match self {
            Self::Automatic(AutomaticLineClamp {
                cutoff: ClampPoint::AfterInlineLine(point),
                ..
            }) => point.source_end(),
            Self::Computed(_) | Self::Used(_) | Self::Automatic(_) => None,
        }
    }

    /// Whether source for `line_index` lies after the selected clamp point.
    pub(crate) fn excludes_line(self, line_index: usize) -> bool {
        match self {
            Self::Computed(clamp) => match clamp.cutoff {
                ClampPointRule::AfterLines(limit) => line_index >= limit.get(),
                ClampPointRule::AutomaticBlockSize => false,
            },
            Self::Used(clamp) => line_index >= clamp.remaining.visible_line_limit(),
            Self::Automatic(clamp) => match clamp.cutoff {
                ClampPoint::AtContainerStart => true,
                ClampPoint::AfterInlineLine(point) => line_index > point.block_line_index(),
                ClampPoint::BetweenBlockSiblings(_) => false,
            },
        }
    }

    /// Whether this selected source line is the terminal marker line.
    pub(crate) fn is_terminal_line(self, line_index: usize) -> bool {
        match self {
            Self::Computed(clamp) => match clamp.cutoff {
                ClampPointRule::AfterLines(limit) => line_index + 1 == limit.get(),
                ClampPointRule::AutomaticBlockSize => false,
            },
            Self::Used(clamp) => line_index + 1 == clamp.remaining.visible_line_limit(),
            Self::Automatic(clamp) => matches!(
                clamp.cutoff,
                ClampPoint::AfterInlineLine(point)
                    if line_index == point.block_line_index()
            ),
        }
    }

    /// Whether the selected sequence cursor has arrived at the clamp point.
    pub(crate) fn reached_after_line_count(self, next_line_index: usize) -> bool {
        match self {
            Self::Computed(clamp) => match clamp.cutoff {
                ClampPointRule::AfterLines(limit) => next_line_index == limit.get(),
                ClampPointRule::AutomaticBlockSize => false,
            },
            Self::Used(clamp) => next_line_index == clamp.remaining.visible_line_limit(),
            Self::Automatic(clamp) => matches!(
                clamp.cutoff,
                ClampPoint::AfterInlineLine(point)
                    if next_line_index == point.block_line_index() + 1
            ),
        }
    }

    pub(crate) fn max_lines(self) -> usize {
        match self {
            Self::Computed(clamp) => match clamp.cutoff {
                ClampPointRule::AfterLines(limit) => limit.get(),
                ClampPointRule::AutomaticBlockSize => {
                    unreachable!("automatic clamp points do not enter inline line-limit selection")
                }
            },
            Self::Used(clamp) => clamp.remaining.visible_line_limit(),
            Self::Automatic(clamp) => match clamp.cutoff {
                ClampPoint::AtContainerStart => 0,
                ClampPoint::AfterInlineLine(point) => point.block_line_index() + 1,
                ClampPoint::BetweenBlockSiblings(_) => 0,
            },
        }
    }

    pub(crate) fn ellipsis(self) -> &'a BlockEllipsis {
        match self {
            Self::Computed(clamp) => clamp.marker,
            Self::Used(clamp) => &clamp.ellipsis,
            Self::Automatic(clamp) => clamp.marker,
        }
    }

    pub(crate) fn continuation(self) -> ClampContinuation {
        match self {
            Self::Computed(_) => ClampContinuation::None,
            Self::Used(clamp) => clamp.continuation,
            Self::Automatic(_) => ClampContinuation::None,
        }
    }
}

/// The computed value of CSS `zoom` on one element.
///
/// CSS Viewport defines `zoom` as a non-inherited number or percentage.  The
/// `0` compatibility value is retained at computed-value time and normalized
/// only when deriving the used scale:
/// <https://drafts.csswg.org/css-viewport/#zoom-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CssZoom(f32);

impl CssZoom {
    pub(crate) const NORMAL: Self = Self(1.0);

    pub(crate) fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let factor = if let Some(percentage) = value.strip_suffix('%') {
            percentage.trim().parse::<f32>().ok()? / 100.0
        } else {
            value.parse::<f32>().ok()?
        };
        if !factor.is_finite() || factor < 0.0 {
            return None;
        }
        Some(Self(factor))
    }

    /// Exposes the unnormalized computed value to cascade tests. Layout uses
    /// [`Self::used_factor`] instead.
    #[cfg(test)]
    pub(crate) const fn factor(self) -> f32 {
        self.0
    }

    pub(crate) const fn used_factor(self) -> f32 {
        if self.0 == 0.0 { 1.0 } else { self.0 }
    }
}

/// The product of an element's `zoom` and all flat-tree ancestor zooms.
///
/// This is deliberately distinct from [`CssZoom`]: the former is a computed
/// property value, while this quantity is only valid at used-value boundaries.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EffectiveZoom(f32);

impl EffectiveZoom {
    pub(crate) const NORMAL: Self = Self(1.0);

    pub(crate) const fn from_parent_and_local(parent: Self, local: CssZoom) -> Self {
        Self(parent.0 * local.used_factor())
    }

    pub(crate) const fn factor(self) -> f32 {
        self.0
    }
}

mod cascaded_style_source {
    pub trait Sealed {}
}

/// A style that may be used as a cascade or fresh used-value source.
///
/// This is deliberately implemented only for [`ComputedStyle`]. In
/// particular, a [`ZoomedLayoutStyle`] must not satisfy this trait through a
/// `Deref` coercion: a used style has already crossed the CSS `zoom` boundary
/// and can never be normalized or inherited from again.
pub(crate) trait CascadedStyleSource: cascaded_style_source::Sealed {
    fn cascaded_style(&self) -> &ComputedStyle;
}

/// CSS Color Adjustment's computed `color-scheme` value. Custom identifiers
/// are preserved for validity but have no meaning until a future CSS module
/// defines them.
/// <https://www.w3.org/TR/css-color-adjust-1/#color-scheme-prop>
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComputedColorScheme {
    Normal,
    Supported(ColorSchemeSupport),
}

/// A stylesheet `@property` registration that Quire can compute today.
/// Registrations remain document-scoped; computed styles carry an `Arc` to
/// their immutable lookup only so custom-property computation has no ambient
/// mutable state.
/// <https://drafts.css-houdini.org/css-properties-values-api/#at-property-rule>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PropertyRegistrationRule {
    pub(crate) names: Vec<String>,
    pub(crate) registration: RegisteredCustomProperty,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RegisteredCustomProperty {
    pub(crate) inherits: bool,
    pub(crate) initial_color: CssColor,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RegisteredCustomProperties {
    pub(crate) by_name: HashMap<String, RegisteredCustomProperty>,
}

/// Computed custom-property value. Unregistered properties retain their parsed
/// CSS component-value stream, while a registered `<color>` stores a typed CSS
/// color and is only serialized at a `var()` substitution boundary.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComputedCustomPropertyValue {
    Tokens(crate::css::component_values::CssComponentValueList),
    Color(CssColor),
}

impl ComputedCustomPropertyValue {
    pub(crate) fn substitution_tokens(&self) -> String {
        match self {
            Self::Tokens(value) => value.as_css().to_string(),
            Self::Color(color) => {
                let color = color.to_rgb_space(RgbColorSpace::Srgb);
                let [red, green, blue] = color.components();
                format!("color(srgb {red} {green} {blue} / {})", color.alpha())
            }
        }
    }

    pub(crate) fn token_stream(&self) -> Option<&str> {
        match self {
            Self::Tokens(value) => Some(value.as_css()),
            Self::Color(_) => None,
        }
    }
}

impl RegisteredCustomProperties {
    pub(crate) fn from_rules<'a>(stylesheets: impl IntoIterator<Item = &'a Stylesheet>) -> Self {
        let mut by_name = HashMap::new();
        for stylesheet in stylesheets {
            for rule in &stylesheet.property_registrations {
                for name in &rule.names {
                    by_name.insert(name.clone(), rule.registration.clone());
                }
            }
        }
        Self { by_name }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColorSchemeSupport {
    pub(crate) schemes: Vec<ColorSchemeName>,
    pub(crate) only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ColorSchemeName {
    Light,
    Dark,
    Custom(String),
}

impl ComputedColorScheme {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let values = crate::css::component_values::split_css_component_values(value);
        if values.len() == 1 && values[0].eq_ignore_ascii_case("normal") {
            return Some(Self::Normal);
        }
        let mut schemes = Vec::new();
        let mut only = false;
        for (index, value) in values.iter().enumerate() {
            if value.eq_ignore_ascii_case("only") {
                if index + 1 != values.len() || schemes.is_empty() {
                    return None;
                }
                only = true;
                continue;
            }
            let name = if value.eq_ignore_ascii_case("light") {
                ColorSchemeName::Light
            } else if value.eq_ignore_ascii_case("dark") {
                ColorSchemeName::Dark
            } else if crate::css::component_values::css_single_ident(value).is_some() {
                ColorSchemeName::Custom((*value).to_string())
            } else {
                return None;
            };
            schemes.push(name);
        }
        (!schemes.is_empty()).then_some(Self::Supported(ColorSchemeSupport { schemes, only }))
    }

    pub(crate) fn used_scheme(
        &self,
        preference: ColorSchemePreference,
        page_scheme: UsedColorScheme,
    ) -> UsedColorScheme {
        let Self::Supported(support) = self else {
            return page_scheme;
        };
        let recognizes = |scheme| match scheme {
            UsedColorScheme::Light => support
                .schemes
                .iter()
                .any(|name| matches!(name, ColorSchemeName::Light)),
            UsedColorScheme::Dark => support
                .schemes
                .iter()
                .any(|name| matches!(name, ColorSchemeName::Dark)),
        };
        if let Some(preferred) = preference.preferred()
            && (recognizes(preferred) || (preference.is_override() && !support.only))
        {
            return preferred;
        }
        support
            .schemes
            .iter()
            .find_map(|name| match name {
                ColorSchemeName::Light => Some(UsedColorScheme::Light),
                ColorSchemeName::Dark => Some(UsedColorScheme::Dark),
                ColorSchemeName::Custom(_) => None,
            })
            .unwrap_or(page_scheme)
    }
}

/// Computed values needed for Quire's single static CSS Animation snapshot.
///
/// This intentionally models only the longhands consumed by the snapshot
/// implementation. CSS Animations' broader multi-animation and timeline
/// behavior remains outside this renderer's current scope.
/// <https://www.w3.org/TR/css-animations-1/#animation-name>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedAnimationSnapshot {
    pub(in crate::css) name: Option<KeyframesName>,
    pub(in crate::css) duration_seconds: f32,
    pub(in crate::css) delay_seconds: f32,
}

impl ComputedAnimationSnapshot {
    pub(in crate::css) const INITIAL: Self = Self {
        name: None,
        duration_seconds: 0.0,
        delay_seconds: 0.0,
    };
}

/// Identifies the counter event that supplies a list marker's ordinal.
///
/// This is layout provenance rather than a CSS property. Principal boxes have
/// their own marker event; tree-abiding generated list items use their
/// `::before` or `::after` event instead.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MarkerCounterOrigin {
    #[default]
    Principal,
    Before,
    After,
}

/// Cascade-only source ownership for canonical longhands.
///
/// This metadata is deliberately excluded from computed-style equality: two
/// styles with equal computed values remain equal even when one value was
/// specified and the other inherited. Layout uses the ownership only when a
/// generated `::first-line` box becomes the inheritance parent of selected
/// inline descendants.
#[derive(Debug, Clone, Default)]
pub(crate) struct ComputedLonghandProvenance {
    pub(crate) current_source: u16,
    pub(crate) sources: Arc<[u16]>,
}

impl PartialEq for ComputedLonghandProvenance {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedStyle {
    pub(crate) longhand_provenance: ComputedLonghandProvenance,
    pub custom_properties: HashMap<String, ComputedCustomPropertyValue>,
    pub(crate) registered_custom_properties: Arc<RegisteredCustomProperties>,
    /// Computed author support declared by CSS Color Adjustment's
    /// `color-scheme` property.
    pub color_scheme: ComputedColorScheme,
    /// The resolved scheme used by color-dependent values on this element.
    pub used_color_scheme: UsedColorScheme,
    /// The document page scheme inherited by `color-scheme: normal`.
    pub page_color_scheme: UsedColorScheme,
    /// The subset of CSS Animation computed values that Quire needs to
    /// synthesize its static keyframe snapshot. These values are non-inherited
    /// by default, but remain on the computed style so explicit `inherit` can
    /// copy the parent's animation state.
    /// <https://www.w3.org/TR/css-animations-1/#animation-name>
    pub(crate) animation_snapshot: ComputedAnimationSnapshot,
    /// The element's own non-inherited CSS `zoom` computed value.
    pub zoom: CssZoom,
    /// Layout-only product of this element's and all ancestor zoom values.
    pub effective_zoom: EffectiveZoom,
    pub display: Display,
    /// Specified legacy `display: -webkit-box` provenance.
    ///
    /// It establishes a flex formatting context unless a final line-clamp
    /// resolution turns it into a flow-root container.
    pub legacy_webkit_box: LegacyWebkitBox,
    /// The independently cascaded legacy orientation.
    pub webkit_box_orient: WebkitBoxOrient,
    pub flex_direction: FlexDirection,
    pub justify_content: JustifyContent,
    pub justify_items: JustifyItems,
    pub justify_self: JustifySelf,
    pub align_content: AlignContent,
    pub align_items: AlignItems,
    pub align_self: AlignSelf,
    pub flex_grow: FlexGrowFactor,
    pub flex_shrink: FlexShrinkFactor,
    pub flex_basis: ComputedFlexBasis,
    pub flex_wrap: FlexWrap,
    /// Author-requested minimum number of balanced flex lines.
    ///
    /// CSS Flexbox Level 2 only applies this property to balanced wrapping:
    /// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>.
    pub flex_line_count: FlexLineCount,
    pub order: i32,
    pub row_gap: ComputedGap,
    pub row_rule: GapRuleAxis,
    pub grid_template_rows: GridTrackList,
    pub grid_template_columns: GridTrackList,
    pub grid_template_areas: GridTemplateAreas,
    pub grid_auto_rows: GridAutoTrackList,
    pub grid_auto_columns: GridAutoTrackList,
    pub grid_auto_flow: GridAutoFlow,
    pub grid_lanes_direction: GridLanesDirection,
    pub grid_lanes_flow_tolerance: GridLanesFlowTolerance,
    pub grid_row_start: GridPlacement,
    pub grid_row_end: GridPlacement,
    pub grid_column_start: GridPlacement,
    pub grid_column_end: GridPlacement,
    pub column_count: ColumnCount,
    pub column_width: ComputedColumnWidth,
    pub column_height: ComputedColumnHeight,
    pub column_wrap: ColumnWrap,
    pub column_fill: ColumnFill,
    pub column_span: ColumnSpan,
    pub column_gap: ComputedGap,
    pub column_rule: GapRuleAxis,
    pub rule_overlap: GapRuleOverlap,
    pub box_values: ComputedBoxValues,
    pub aspect_ratio: AspectRatio,
    pub contain_intrinsic_size: ContainIntrinsicSize,
    pub margin_trim: MarginTrim,
    pub margin: Edges,
    pub ua_margin_em: OptionalEdges<f32>,
    pub padding: Edges,
    pub border_width: f32,
    pub border_widths: Edges,
    pub border_width_values: PhysicalEdges<ComputedLengthPercentage>,
    pub border_color: CssColorOrCurrentColor,
    pub border_colors: BorderColors,
    pub border_styles: BorderStyles,
    pub border_radius: BorderRadius,
    pub corner_shapes: CornerShapes,
    pub border_shape: BorderShape,
    pub shape_outside: ShapeOutside,
    /// CSS Shapes Level 1 offset applied to the resolved `shape-outside`
    /// contour. Percentages resolve against the float containing block's
    /// inline size at used-value time.
    /// <https://drafts.csswg.org/css-shapes-1/#shape-margin-property>
    pub shape_margin: ComputedLengthPercentage,
    /// CSS Shapes alpha cutoff for image-backed float contours.
    /// <https://drafts.csswg.org/css-shapes-1/#shape-image-threshold-property>
    pub shape_image_threshold: ShapeImageThreshold,
    pub border_image: BorderImage,
    pub outline_width: f32,
    pub outline_width_value: ComputedLengthPercentage,
    pub outline_color: CssColorOrCurrentColor,
    pub outline_style: BorderStyle,
    pub outline_offset: ComputedLengthPercentage,
    pub border_collapse: BorderCollapse,
    pub caption_side: CaptionSide,
    pub table_layout: TableLayout,
    pub empty_cells: EmptyCells,
    pub border_spacing: CascadedTableBorderSpacing,
    pub background: Background,
    pub object_fit: ObjectFit,
    pub object_view_box: ObjectViewBox,
    pub image_orientation: ImageOrientation,
    pub image_rendering: ImageRendering,
    /// CSS Images positions the concrete object inside a replaced element's
    /// content box.  The grammar and percentage basis are the same as one
    /// background-position layer, but its initial value is centered.
    /// <https://www.w3.org/TR/css-images-3/#the-object-position>
    pub object_position: BackgroundPosition,
    pub box_decoration_break: BoxDecorationBreak,
    pub box_shadow: Vec<BoxShadow>,
    /// CSS CssColor Adjustment opt-out state inherited by this element.
    pub forced_color_adjust: ForcedColorAdjust,
    pub color: CssColor,
    pub svg_fill: SvgPresentationPaint,
    pub svg_stroke: SvgPresentationPaint,
    pub svg_stroke_width: SvgStrokeWidth,
    pub svg_flood_color: SvgFilterColor,
    pub svg_lighting_color: SvgFilterColor,
    /// WebKit-compatible glyph fill color.
    pub text_fill_color: CssColorOrCurrentColor,
    pub font_size: f32,
    /// Used font size of the document root, the computed-value basis for `rem`.
    /// <https://www.w3.org/TR/css-values-4/#rem>
    pub root_font_size: f32,
    /// The pre-used-value form of `font-size`.
    ///
    /// Mutable formatting boxes retain this until the pre-freeze font
    /// resolution pass has selected the parent metric font.
    pub deferred_font_size: DeferredFontSize,
    pub font_size_adjust: FontSizeAdjust,
    pub line_height_value: ComputedLineHeight,
    pub line_height: f32,
    pub letter_spacing: ComputedLengthPercentage,
    pub word_spacing: ComputedLengthPercentage,
    pub box_sizing: BoxSizing,
    pub direction: Direction,
    pub unicode_bidi: UnicodeBidi,
    pub writing_mode: WritingMode,
    /// Writing mode of the nearest flat-tree ancestor that generates a box.
    ///
    /// This is layout-tree context rather than a CSS longhand. CSS Display 3
    /// requires an element whose `flow` inner display type has a different
    /// writing mode from its box parent to establish an independent formatting
    /// context. `display: contents` ancestors do not generate a box and must
    /// therefore be skipped.
    /// <https://drafts.csswg.org/css-display-3/#transformations>
    pub(crate) nearest_box_parent_writing_mode: WritingMode,
    pub text_orientation: TextOrientation,
    pub text_combine_upright: TextCombineUpright,
    pub text_align: TextAlign,
    pub text_align_last: TextAlignLast,
    pub text_justify: TextJustify,
    pub text_autospace: TextAutospace,
    pub text_fit: TextFit,
    pub text_spacing_trim: TextSpacingTrim,
    pub word_space_transform: WordSpaceTransform,
    pub line_fit_edge: LineFitEdge,
    pub text_box_trim: TextBoxTrim,
    pub text_box_edge: TextBoxEdge,
    pub initial_letter: InitialLetter,
    pub initial_letter_align: InitialLetterAlign,
    pub initial_letter_wrap: InitialLetterWrap,
    pub text_indent: ComputedTextIndent,
    pub hanging_punctuation: HangingPunctuation,
    pub vertical_align: VerticalAlign,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub font_width: FontWidth,
    pub font_family: FontFamily,
    pub font_language_override: FontLanguageOverride,
    pub font_synthesis: FontSynthesis,
    pub font_feature_settings: FontFeatureSettings,
    pub font_variation_settings: FontVariationSettings,
    pub font_kerning: FontKerning,
    pub font_variant_ligatures: FontVariantLigatures,
    pub font_variant_position: FontVariantPosition,
    pub font_variant_caps: FontVariantCaps,
    pub font_variant_numeric: FontVariantNumeric,
    pub font_variant_alternates: FontVariantAlternates,
    pub font_variant_east_asian: FontVariantEastAsian,
    pub font_variant_emoji: FontVariantEmoji,
    pub font_palette: FontPalette,
    pub language: ContentLanguage,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    pub text_wrap_mode: TextWrapMode,
    pub text_wrap_style: TextWrapStyle,
    pub wrap_inside: WrapInside,
    /// CSS Overflow's independently cascaded line-limit longhand.
    pub max_lines: MaxLines,
    /// The inherited marker policy for block-axis truncation.
    pub block_ellipsis: BlockEllipsis,
    /// CSS Overflow's continuation behavior. `continue` is a Rust keyword,
    /// so the field has a trailing underscore.
    pub continue_: Continue,
    /// A transient layout override for a computed line clamp inherited from
    /// an ancestor block-flow traversal. It is never cascaded or serialized.
    pub(crate) line_limit_traversal: Option<LineLimitTraversal>,
    /// A transient automatic clamp allowance propagated through an eligible
    /// block formatting context. It is never cascaded or serialized.
    pub(crate) automatic_block_size_traversal: Option<AutomaticBlockSizeTraversal>,
    pub(crate) automatic_block_boundary_marker: Option<AutomaticBlockBoundaryMarker>,
    pub tab_size: TabSize,
    pub word_break: WordBreak,
    pub overflow: Overflow,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub scrollbar_gutter: ScrollbarGutter,
    pub scrollbar_width: ScrollbarWidth,
    pub scroll_snap_type: ScrollSnapType,
    pub scroll_snap_align: ScrollSnapAlign,
    pub scroll_snap_stop: ScrollSnapStop,
    pub scroll_target_group: ScrollTargetGroup,
    pub scroll_marker_group: Option<ScrollMarkerGroup>,
    pub scroll_padding: PhysicalEdges<ScrollPadding>,
    pub scroll_margin: PhysicalEdges<ComputedLengthPercentage>,
    pub overflow_clip_margin: OverflowClipMargin,
    pub overflow_wrap: OverflowWrap,
    pub line_break: LineBreak,
    pub hyphens: Hyphens,
    pub hyphenate_character: HyphenateCharacter,
    pub hyphenate_limit_chars: HyphenateLimitChars,
    pub visibility: Visibility,
    pub list_style_type: ListStyleType,
    pub list_style_position: ListStylePosition,
    /// The selected CSS image for an automatic list marker.
    ///
    /// Keeping the full image value preserves `image-set()`'s selected
    /// candidate resolution until marker intrinsic sizing.
    /// <https://drafts.csswg.org/css-lists-3/#list-style-image-property>
    /// <https://drafts.csswg.org/css-images-4/#image-set-notation>
    pub list_style_image: ComputedImage,
    pub marker_side: MarkerSide,
    /// The counter snapshot that owns a tree-abiding list item's marker.
    ///
    /// A generated `::before`/`::after` list item shares its originating DOM
    /// element but has a distinct counter event. Keep that identity on the
    /// layout style so marker construction can select the pseudo's snapshot
    /// after the box tree has detached it from the originating element.
    pub(crate) marker_counter_origin: MarkerCounterOrigin,
    pub marker_content: MarkerContent,
    pub marker_style: Option<Box<ComputedStyle>>,
    pub content: Content,
    pub before_style: Option<Box<ComputedStyle>>,
    pub after_style: Option<Box<ComputedStyle>>,
    pub scroll_marker_style: Option<Box<ComputedStyle>>,
    pub scroll_marker_group_style: Option<Box<ComputedStyle>>,
    /// Author-cascaded GCPM `::footnote-call` style. Its default generated
    /// counter content is created only when the element is a footnote.
    pub footnote_call_style: Option<Box<ComputedStyle>>,
    /// Author-cascaded GCPM `::footnote-marker` style. Its default generated
    /// counter content is created only when the element is a footnote.
    pub footnote_marker_style: Option<Box<ComputedStyle>>,
    pub first_line_style: Option<Box<ComputedStyle>>,
    /// Canonical longhands addressed by winning `::first-line` declarations.
    /// Layout replays these through the typed computed-value copier.
    pub(crate) first_line_overrides: ModeledLonghandSet,
    pub first_letter_style: Option<Box<ComputedStyle>>,
    pub quotes: Quotes,
    pub counter_resets: Vec<CounterReset>,
    pub counter_increments: Vec<CounterChange>,
    pub counter_sets: Vec<CounterChange>,
    pub string_sets: Vec<NamedStringSet>,
    pub page: PageAssignment,
    pub break_before: PageBreak,
    pub break_after: PageBreak,
    pub break_inside: BreakInsideAvoidance,
    pub orphans: Orphans,
    pub widows: Widows,
    pub text_decoration_origins: TextDecorationOrigins,
    pub text_decoration: TextDecoration,
    pub text_shadow: Vec<TextShadow>,
    pub text_emphasis_style: TextEmphasisStyle,
    pub text_emphasis_color: CssColorOrCurrentColor,
    pub text_emphasis_position: TextEmphasisPosition,
    pub text_emphasis_skip: TextEmphasisSkip,
    pub ruby_position: RubyPosition,
    pub ruby_align: RubyAlign,
    pub ruby_overhang: RubyOverhang,
    pub position: Position,
    pub float: Float,
    /// GCPM presentation mode for this element after `float: footnote`
    /// extracts it from normal flow.
    pub footnote_display: FootnoteDisplay,
    /// GCPM page-break policy for the call/body pair created by
    /// `float: footnote`.
    pub footnote_policy: FootnotePolicy,
    pub clear: Clear,
    pub abspos_static_source: StaticPositionSource,
    pub z_index: ZIndex,
    pub opacity: Opacity,
    pub transform: TransformList,
    pub individual_transforms: IndividualTransforms,
    pub transform_origin: TransformOrigin,
    pub perspective: ComputedPerspective,
    pub perspective_origin: PerspectiveOrigin,
    pub transform_box: TransformBox,
    pub transform_style: TransformStyle,
    /// Internal CSS Display bridge for an anonymous table wrapper inside an
    /// ancestor's 3D rendering context. This is not a CSS property.
    pub anonymous_3d_layout_bridge: bool,
    pub backface_visibility: BackfaceVisibility,
    pub isolation: Isolation,
    pub mix_blend_mode: MixBlendMode,
    pub filter: FilterValue,
    pub legacy_clip: LegacyClip,
    pub clip_path: ClipPath,
    pub mask: MaskValue,
    /// `mask-border-source` is retained separately from `mask-image`: both
    /// are CSS Transforms grouping-property inputs, while only mask-image is
    /// currently emitted by the paint backend.
    pub mask_border_source: ComputedImage,
    pub contain: Contain,
    /// `container-type` / `container` query-container axis capability.
    /// <https://www.w3.org/TR/css-contain-3/#container-type>
    pub container_type: ContainerType,
    /// Names selected by named `@container` rules.
    /// <https://www.w3.org/TR/css-contain-3/#container-name>
    pub container_names: ContainerNames,
    pub content_visibility: ContentVisibility,
    pub will_change: WillChange,
    pub bookmark_level: BookmarkLevel,
    pub bookmark_label: BookmarkLabel,
    pub bookmark_state: CssBookmarkState,
}

impl cascaded_style_source::Sealed for ComputedStyle {}

impl CascadedStyleSource for ComputedStyle {
    fn cascaded_style(&self) -> &ComputedStyle {
        self
    }
}

/// A cloned style at the layout used-value boundary, before CSS `zoom`.
///
/// Viewport- and font-relative normalization mutate this owned working copy;
/// cascade and frozen-tree state remain [`ComputedStyle`]. Consuming this
/// wrapper is the only path to [`ZoomedLayoutStyle`].
/// <https://drafts.csswg.org/css-cascade-5/#used>
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
#[derive(Debug, Clone)]
pub(crate) struct LayoutStyle(ComputedStyle);

impl LayoutStyle {
    pub(crate) fn from_computed(style: &impl CascadedStyleSource) -> Self {
        Self(style.cascaded_style().clone())
    }

    pub(crate) fn into_zoomed(mut self) -> ZoomedLayoutStyle {
        self.0.apply_effective_zoom();
        ZoomedLayoutStyle(ZoomedComputedStyle(self.0))
    }
}

impl Deref for LayoutStyle {
    type Target = ComputedStyle;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LayoutStyle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// A layout style whose fixed components have crossed the CSS `zoom`
/// used-value boundary exactly once.
///
/// This wrapper deliberately has no conversion back to [`LayoutStyle`]. It
/// can be read or mutated by layout algorithms, but it cannot be zoomed a
/// second time or reused as a cascade input.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ZoomedLayoutStyle(ZoomedComputedStyle);

/// Private field-access facade for a zoomed style.
///
/// Keeping this separate from [`ComputedStyle`] ensures that
/// [`ZoomedLayoutStyle`] itself has no `Deref<Target = ComputedStyle>` escape
/// hatch.  The facade is intentionally not exported from this module: public
/// layout boundaries receive the opaque used-style token, while existing
/// low-level property consumers can be migrated independently.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ZoomedComputedStyle(ComputedStyle);

impl Deref for ZoomedComputedStyle {
    type Target = ComputedStyle;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ZoomedComputedStyle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ZoomedLayoutStyle {
    /// Clone this style for a legacy used-value consumer.
    ///
    /// This bridge is retained only while converting durable replay records
    /// to carry [`ZoomedLayoutStyle`] directly. It is intentionally distinct
    /// from a cascade source: [`CascadedStyleSource`] remains implemented
    /// only by `ComputedStyle`.
    pub(crate) fn clone_for_legacy_used_consumer(&self) -> ComputedStyle {
        self.0.0.clone()
    }

    /// Apply a transformation that is meaningful only after resolving used
    /// values. The underlying computed representation never escapes this
    /// closure, so the result remains unable to participate in cascade.
    pub(crate) fn map_used_values(mut self, transform: impl FnOnce(&mut ComputedStyle)) -> Self {
        transform(&mut self.0.0);
        self
    }
}

impl Deref for ZoomedLayoutStyle {
    type Target = ZoomedComputedStyle;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ZoomedLayoutStyle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The availability of a discretionary hyphenation opportunity during line
/// fitting.
///
/// This describes CSS policy, not whether a source U+00AD exists. In
/// particular, `word-break:auto-phrase` preserves authored and dictionary
/// opportunities for its later overflow-relaxation stage.
/// <https://drafts.csswg.org/css-text-4/#word-break-property>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscretionaryHyphenationPolicy {
    /// The source control and dictionary opportunities have no legal break.
    Disabled,
    /// The candidate participates in ordinary line fitting.
    Ordinary,
    /// The candidate participates only after phrase-wrap relaxation fails.
    DeferredForAutoPhrase,
}

impl ComputedStyle {
    /// Resolves the specified border-width values to the used physical edge
    /// widths consumed by layout.
    ///
    /// `none` and `hidden` force a zero used width even though the specified
    /// width remains available for a later cascade declaration.
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-width>
    fn resolve_used_border_widths(&mut self) {
        let used_width = |value: &ComputedLengthPercentage, border_style: BorderStyle| {
            if border_style.suppresses_used_width() {
                0.0
            } else {
                used_css_border_width(value.clone().length_max_zero().points())
            }
        };
        self.border_widths = Edges {
            top: used_width(&self.border_width_values.top, self.border_styles.top),
            right: used_width(&self.border_width_values.right, self.border_styles.right),
            bottom: used_width(&self.border_width_values.bottom, self.border_styles.bottom),
            left: used_width(&self.border_width_values.left, self.border_styles.left),
        };
        self.border_width = self
            .border_widths
            .top
            .max(self.border_widths.right)
            .max(self.border_widths.bottom)
            .max(self.border_widths.left);
    }

    /// Return the used `direction` after writing-mode text-orientation rules.
    ///
    /// In vertical typographic modes, `text-orientation: upright` makes the
    /// used value of `direction` be `ltr`.  The computed value remains useful
    /// for cascade and serialization, so consumers that resolve logical axes
    /// must take this used-value boundary explicitly instead of rewriting the
    /// stored property.
    /// <https://drafts.csswg.org/css-writing-modes-4/#text-orientation>
    pub(crate) const fn used_direction(&self) -> Direction {
        match self.text_layout_policy() {
            TextLayoutPolicy::Vertical(TextOrientation::Upright) => Direction::Ltr,
            TextLayoutPolicy::Horizontal
            | TextLayoutPolicy::Vertical(TextOrientation::Mixed | TextOrientation::Sideways)
            | TextLayoutPolicy::Sideways(_) => self.direction,
        }
    }

    /// Return the used text-layout policy derived from writing-mode and
    /// text-orientation.
    ///
    /// CSS Writing Modes applies `text-orientation` only to vertical
    /// typographic mode; sideways writing modes instead force horizontal runs
    /// into their specified rotation.
    /// <https://www.w3.org/TR/css-writing-modes-4/#text-orientation>.
    pub(crate) const fn text_layout_policy(&self) -> TextLayoutPolicy {
        self.writing_mode.text_layout_policy(self.text_orientation)
    }

    /// Whether this inline may contribute soft wrap opportunities.
    ///
    /// The legacy `white-space` shorthand determines the initial wrapping
    /// mode, while the CSS Text 4 `text-wrap-mode` longhand can override it.
    /// <https://drafts.csswg.org/css-text-4/#text-wrap-mode-property>
    pub(crate) const fn allows_soft_wrap(&self) -> bool {
        match self.text_wrap_mode {
            TextWrapMode::Legacy => {
                !matches!(self.white_space, WhiteSpace::NoWrap | WhiteSpace::Pre)
            }
            TextWrapMode::Wrap => true,
            TextWrapMode::NoWrap => false,
        }
    }

    /// Return the policy for authored U+00AD discretionary hyphens.
    ///
    /// `word-break:auto-phrase` retains these source-faithful opportunities
    /// as a fallback when phrase wrapping cannot prevent overflow; it does
    /// not remove the control from the source text.
    /// <https://drafts.csswg.org/css-text-4/#word-break-property>
    pub(crate) fn authored_discretionary_hyphenation_policy(
        &self,
    ) -> DiscretionaryHyphenationPolicy {
        if self.hyphens == Hyphens::None || matches!(self.word_break, WordBreak::BreakAll) {
            DiscretionaryHyphenationPolicy::Disabled
        } else if matches!(self.word_break, WordBreak::AutoPhrase) {
            DiscretionaryHyphenationPolicy::DeferredForAutoPhrase
        } else {
            DiscretionaryHyphenationPolicy::Ordinary
        }
    }

    /// Return the policy for language-dictionary hyphenation opportunities.
    ///
    /// `hyphens:auto` is required to collect dictionary opportunities, while
    /// `word-break:auto-phrase` defers rather than suppresses those
    /// opportunities.
    /// <https://drafts.csswg.org/css-text-3/#hyphens-property>
    /// <https://drafts.csswg.org/css-text-4/#word-break-property>
    pub(crate) fn automatic_discretionary_hyphenation_policy(
        &self,
    ) -> DiscretionaryHyphenationPolicy {
        if self.hyphens != Hyphens::Auto {
            DiscretionaryHyphenationPolicy::Disabled
        } else {
            self.authored_discretionary_hyphenation_policy()
        }
    }

    /// Returns the clip edge for the color layer beneath every background
    /// image layer.
    ///
    /// CSS Backgrounds and Borders paints `background-color` below the image
    /// layers and clips it using the final (bottom-most) `background-clip`
    /// value after layer-list repetition:
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-clip>.
    pub(crate) fn background_color_clip(&self) -> BackgroundBox {
        self.background.color_clip()
    }

    /// Applies the element's effective CSS zoom at the used-value boundary.
    ///
    /// `zoom` scales fixed used lengths, while percentage and `auto` values
    /// remain relative to the already scaled containing block.  Callers use a
    /// fresh layout-style clone, so this transformation is intentionally
    /// destructive and must be performed exactly once per clone.
    /// <https://drafts.csswg.org/css-viewport/#zoom-property>
    fn apply_effective_zoom(&mut self) {
        let factor = self.effective_zoom.factor();
        if factor == 1.0 {
            return;
        }
        self.box_values.scale_fixed_length_components(factor);
        self.flex_basis.scale_fixed_length_components(factor);
        self.row_gap.scale_fixed_length_components(factor);
        self.column_gap.scale_fixed_length_components(factor);
        self.column_width.scale_fixed_length_components(factor);
        self.column_height.scale_fixed_length_components(factor);
        self.column_rule.scale_fixed_length_components(factor);
        self.row_rule.scale_fixed_length_components(factor);
        self.grid_template_rows
            .scale_fixed_length_components(factor);
        self.grid_template_columns
            .scale_fixed_length_components(factor);
        self.grid_auto_rows.scale_fixed_length_components(factor);
        self.grid_auto_columns.scale_fixed_length_components(factor);
        self.grid_lanes_flow_tolerance
            .scale_fixed_length_components(factor);
        self.border_spacing.scale_fixed_length_components(factor);
        for shadow in &mut self.text_shadow {
            shadow.scale_fixed_length_components(factor);
        }
        for shadow in &mut self.box_shadow {
            shadow.scale_fixed_length_components(factor);
        }
        self.font_size *= factor;
        self.root_font_size *= factor;
        self.line_height_value.scale_fixed_length_components(factor);
        self.line_height *= factor;
        self.margin = scaled_edges(self.margin, factor);
        self.padding = scaled_edges(self.padding, factor);
        self.border_width *= factor;
        self.border_widths = scaled_edges(self.border_widths, factor);
        for value in [
            &mut self.border_width_values.top,
            &mut self.border_width_values.right,
            &mut self.border_width_values.bottom,
            &mut self.border_width_values.left,
        ] {
            value.scale_fixed_length_components(factor);
        }
        self.outline_width *= factor;
        self.outline_width_value
            .scale_fixed_length_components(factor);
        self.letter_spacing.scale_fixed_length_components(factor);
        self.word_spacing.scale_fixed_length_components(factor);
        self.text_indent
            .amount
            .scale_fixed_length_components(factor);
    }

    pub fn initial() -> Self {
        let font_size = 12.0;
        Self {
            longhand_provenance: ComputedLonghandProvenance::default(),
            custom_properties: HashMap::new(),
            registered_custom_properties: Arc::default(),
            color_scheme: ComputedColorScheme::Normal,
            used_color_scheme: UsedColorScheme::Light,
            page_color_scheme: UsedColorScheme::Light,
            animation_snapshot: ComputedAnimationSnapshot::INITIAL,
            zoom: CssZoom::NORMAL,
            effective_zoom: EffectiveZoom::NORMAL,
            display: Display::INLINE,
            legacy_webkit_box: LegacyWebkitBox::None,
            webkit_box_orient: WebkitBoxOrient::Horizontal,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::NORMAL,
            justify_items: JustifyItems::NORMAL,
            justify_self: JustifySelf::AUTO,
            align_content: AlignContent::NORMAL,
            align_items: AlignItems::NORMAL,
            align_self: AlignSelf::AUTO,
            flex_grow: FlexGrowFactor::ZERO,
            flex_shrink: FlexShrinkFactor::ONE,
            flex_basis: ComputedFlexBasis::AUTO,
            flex_wrap: FlexWrap::NoWrap,
            flex_line_count: FlexLineCount::ONE,
            order: 0,
            row_gap: ComputedGap::NORMAL,
            row_rule: GapRuleAxis::initial(),
            grid_template_rows: GridTrackList::NONE,
            grid_template_columns: GridTrackList::NONE,
            grid_template_areas: GridTemplateAreas::NONE,
            grid_auto_rows: GridAutoTrackList::initial(),
            grid_auto_columns: GridAutoTrackList::initial(),
            grid_auto_flow: GridAutoFlow::ROW,
            grid_lanes_direction: GridLanesDirection::NORMAL,
            grid_lanes_flow_tolerance: GridLanesFlowTolerance::NORMAL,
            grid_row_start: GridPlacement::AUTO,
            grid_row_end: GridPlacement::AUTO,
            grid_column_start: GridPlacement::AUTO,
            grid_column_end: GridPlacement::AUTO,
            column_count: ColumnCount::Auto,
            column_width: ComputedColumnWidth::AUTO,
            column_height: ComputedColumnHeight::AUTO,
            column_wrap: ColumnWrap::Auto,
            column_fill: ColumnFill::Balance,
            column_span: ColumnSpan::None,
            column_gap: ComputedGap::NORMAL,
            column_rule: GapRuleAxis::initial(),
            rule_overlap: GapRuleOverlap::RowOverColumn,
            box_values: ComputedBoxValues::initial(),
            aspect_ratio: AspectRatio::AUTO,
            contain_intrinsic_size: ContainIntrinsicSize::NONE,
            margin_trim: MarginTrim::NONE,
            margin: Edges::ZERO,
            ua_margin_em: OptionalEdges::NONE,
            padding: Edges::ZERO,
            border_width: 0.0,
            border_widths: Edges::ZERO,
            // `medium` is the initial *specified* border width.  The
            // corresponding resolved edge widths remain zero until a
            // non-suppressing line style makes them usable by layout.
            // <https://www.w3.org/TR/css-backgrounds-3/#border-width>
            border_width_values: PhysicalEdges::all(ComputedLengthPercentage::from_points(
                3.0 * CSS_PX_TO_PT,
            )),
            border_color: CssColorOrCurrentColor::CurrentColor,
            border_colors: BorderColors::CURRENT_COLOR,
            border_styles: BorderStyles::NONE,
            border_radius: BorderRadius::ZERO,
            corner_shapes: CornerShapes::ROUND,
            border_shape: BorderShape::None,
            shape_outside: ShapeOutside::NONE,
            shape_margin: ComputedLengthPercentage::ZERO,
            shape_image_threshold: ShapeImageThreshold::ZERO,
            border_image: BorderImage::initial(),
            outline_width: 3.0 * CSS_PX_TO_PT,
            outline_width_value: ComputedLengthPercentage::from_points(3.0 * CSS_PX_TO_PT),
            outline_color: CssColorOrCurrentColor::CurrentColor,
            outline_style: BorderStyle::None,
            outline_offset: ComputedLengthPercentage::ZERO,
            border_collapse: BorderCollapse::Separate,
            caption_side: CaptionSide::Top,
            table_layout: TableLayout::Auto,
            empty_cells: EmptyCells::Show,
            border_spacing: CascadedTableBorderSpacing::INITIAL,
            background: Background::initial(),
            object_fit: ObjectFit::Fill,
            object_view_box: ObjectViewBox::NONE,
            image_orientation: ImageOrientation::FromImage,
            image_rendering: ImageRendering::Auto,
            object_position: BackgroundPosition {
                x: BackgroundPositionAxis {
                    origin: BackgroundPositionOrigin::Center,
                    offset: ComputedLengthPercentage::ZERO,
                },
                y: BackgroundPositionAxis {
                    origin: BackgroundPositionOrigin::Center,
                    offset: ComputedLengthPercentage::ZERO,
                },
            },
            box_decoration_break: BoxDecorationBreak::Slice,
            box_shadow: Vec::new(),
            forced_color_adjust: ForcedColorAdjust::Auto,
            color: CssColor::BLACK,
            svg_fill: SvgPresentationPaint::initial(SvgPaint::Color(CssColor::BLACK)),
            svg_stroke: SvgPresentationPaint::initial(SvgPaint::None),
            svg_stroke_width: SvgStrokeWidth::Initial(ComputedLengthPercentage::from_points(
                CSS_PX_TO_PT,
            )),
            svg_flood_color: SvgFilterColor::absolute(CssColor::BLACK),
            svg_lighting_color: SvgFilterColor::absolute(CssColor::WHITE),
            text_fill_color: CssColorOrCurrentColor::CurrentColor,
            font_size,
            root_font_size: font_size,
            deferred_font_size: DeferredFontSize::INITIAL,
            font_size_adjust: FontSizeAdjust::None,
            line_height_value: ComputedLineHeight::NORMAL,
            line_height: font_size * 1.2,
            letter_spacing: ComputedLengthPercentage::ZERO,
            word_spacing: ComputedLengthPercentage::ZERO,
            box_sizing: BoxSizing::ContentBox,
            direction: Direction::Ltr,
            unicode_bidi: UnicodeBidi::Normal,
            writing_mode: WritingMode::HorizontalTb,
            nearest_box_parent_writing_mode: WritingMode::HorizontalTb,
            text_orientation: TextOrientation::Mixed,
            text_combine_upright: TextCombineUpright::None,
            text_align: TextAlign::Start,
            text_align_last: TextAlignLast::Auto,
            text_justify: TextJustify::Auto,
            text_autospace: TextAutospace::NORMAL,
            text_fit: TextFit::NONE,
            text_spacing_trim: TextSpacingTrim::Normal,
            word_space_transform: WordSpaceTransform::NONE,
            line_fit_edge: LineFitEdge::Leading,
            text_box_trim: TextBoxTrim::None,
            text_box_edge: TextBoxEdge::Auto,
            initial_letter: InitialLetter::Normal,
            initial_letter_align: InitialLetterAlign::ALPHABETIC,
            initial_letter_wrap: InitialLetterWrap::None,
            text_indent: ComputedTextIndent::ZERO,
            hanging_punctuation: HangingPunctuation::NONE,
            vertical_align: VerticalAlign::BASELINE,
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_width: FontWidth::NORMAL,
            font_family: FontFamily::SansSerif,
            font_language_override: FontLanguageOverride::Normal,
            font_synthesis: FontSynthesis::ALL,
            font_feature_settings: FontFeatureSettings::NORMAL,
            font_variation_settings: FontVariationSettings::NORMAL,
            font_kerning: FontKerning::Auto,
            font_variant_ligatures: FontVariantLigatures::Normal,
            font_variant_position: FontVariantPosition::Normal,
            font_variant_caps: FontVariantCaps::Normal,
            font_variant_numeric: FontVariantNumeric::Normal,
            font_variant_alternates: FontVariantAlternates::Normal,
            font_variant_east_asian: FontVariantEastAsian::Normal,
            font_variant_emoji: FontVariantEmoji::Normal,
            font_palette: FontPalette::Normal,
            language: ContentLanguage::Unknown,
            text_transform: TextTransform::NONE,
            white_space: WhiteSpace::Normal,
            text_wrap_mode: TextWrapMode::Legacy,
            text_wrap_style: TextWrapStyle::Auto,
            wrap_inside: WrapInside::Auto,
            max_lines: MaxLines::None,
            block_ellipsis: BlockEllipsis::NoEllipsis,
            continue_: Continue::Auto,
            line_limit_traversal: None,
            automatic_block_size_traversal: None,
            automatic_block_boundary_marker: None,
            tab_size: TabSize::INITIAL,
            word_break: WordBreak::Normal,
            overflow: Overflow::Visible,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            scrollbar_gutter: ScrollbarGutter::Auto,
            scrollbar_width: ScrollbarWidth::Auto,
            scroll_snap_type: ScrollSnapType::None,
            scroll_snap_align: ScrollSnapAlign::default(),
            scroll_snap_stop: ScrollSnapStop::Normal,
            scroll_target_group: ScrollTargetGroup::None,
            scroll_marker_group: None,
            scroll_padding: PhysicalEdges::all(ScrollPadding::Auto),
            scroll_margin: PhysicalEdges::all(ComputedLengthPercentage::ZERO),
            overflow_clip_margin: OverflowClipMargin::ZERO,
            overflow_wrap: OverflowWrap::Normal,
            line_break: LineBreak::Auto,
            hyphens: Hyphens::Manual,
            hyphenate_character: HyphenateCharacter::Auto,
            hyphenate_limit_chars: HyphenateLimitChars::AUTO,
            visibility: Visibility::Visible,
            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
            list_style_image: ComputedImage::None,
            marker_side: MarkerSide::MatchSelf,
            marker_counter_origin: MarkerCounterOrigin::Principal,
            marker_content: MarkerContent::Auto,
            marker_style: None,
            content: Content::Normal,
            before_style: None,
            after_style: None,
            scroll_marker_style: None,
            scroll_marker_group_style: None,
            footnote_call_style: None,
            footnote_marker_style: None,
            first_line_style: None,
            first_line_overrides: ModeledLonghandSet::empty(),
            first_letter_style: None,
            quotes: Quotes::auto(),
            counter_resets: Vec::new(),
            counter_increments: Vec::new(),
            counter_sets: Vec::new(),
            string_sets: Vec::new(),
            page: PageAssignment::Unspecified,
            break_before: PageBreak::Auto,
            break_after: PageBreak::Auto,
            break_inside: BreakInsideAvoidance::Auto,
            orphans: Orphans::TWO,
            widows: Widows::TWO,
            text_decoration_origins: TextDecorationOrigins::default(),
            text_decoration: TextDecoration {
                underline: false,
                overline: false,
                line_through: false,
                blink: false,
                spelling_error: false,
                grammar_error: false,
                style: TextDecorationStyle::Solid,
                thickness: TextDecorationThickness::Auto,
                inset: TextDecorationInset::ZERO,
                skip_ink: TextDecorationSkipInk::Auto,
                skip_self: TextDecorationSkipSelf::Auto,
                skip_box: TextDecorationSkipBox::None,
                skip_spaces: TextDecorationSkipSpaces::START_END,
                underline_offset: TextUnderlineOffset::Auto,
                underline_position: TextUnderlinePosition::AUTO,
                color: CssColorOrCurrentColor::CurrentColor,
            },
            text_shadow: Vec::new(),
            text_emphasis_style: TextEmphasisStyle::None,
            text_emphasis_color: CssColorOrCurrentColor::CurrentColor,
            text_emphasis_position: TextEmphasisPosition::default(),
            text_emphasis_skip: TextEmphasisSkip::default(),
            ruby_position: RubyPosition::Alternate,
            ruby_align: RubyAlign::SpaceAround,
            ruby_overhang: RubyOverhang::Auto,
            position: Position::Static,
            float: Float::None,
            footnote_display: FootnoteDisplay::Block,
            footnote_policy: FootnotePolicy::Auto,
            clear: Clear::None,
            abspos_static_source: StaticPositionSource::BlockLevel,
            z_index: ZIndex::Auto,
            opacity: Opacity::ONE,
            transform: Vec::new(),
            individual_transforms: IndividualTransforms::NONE,
            transform_origin: TransformOrigin::INITIAL,
            perspective: ComputedPerspective::NONE,
            perspective_origin: PerspectiveOrigin::INITIAL,
            transform_box: TransformBox::INITIAL,
            transform_style: TransformStyle::Flat,
            anonymous_3d_layout_bridge: false,
            backface_visibility: BackfaceVisibility::Visible,
            isolation: Isolation::Auto,
            mix_blend_mode: MixBlendMode::Normal,
            filter: FilterValue::None,
            legacy_clip: LegacyClip::AUTO,
            clip_path: ClipPath::None,
            mask: MaskValue::None,
            mask_border_source: ComputedImage::None,
            contain: Contain::NONE,
            container_type: ContainerType::Normal,
            container_names: ContainerNames::default(),
            content_visibility: ContentVisibility::Visible,
            will_change: WillChange::default(),
            bookmark_level: BookmarkLevel::None,
            bookmark_label: BookmarkLabel::content_text(),
            bookmark_state: CssBookmarkState::Open,
        }
    }

    /// Resolves the deferred `font-size` against the already-used parent
    /// font. This is deliberately separate from ordinary font-relative
    /// property resolution: CSS Fonts gives `font-size` parent-relative
    /// metric bases, whereas other `em`/`ch` lengths use this style's font.
    /// <https://www.w3.org/TR/css-fonts-4/#font-size-prop>
    pub(crate) fn resolve_deferred_font_size(&mut self, parent: FontRelativeLengthBasis) {
        self.font_size = clamp_used_layout_length(self.deferred_font_size.resolve(parent)).points();
        let (line_height, _, _) = self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
    }

    /// Resolves deferred `font-size` after the page viewport has become known.
    ///
    /// CSS viewport-relative font sizes establish the inherited `em` basis of
    /// descendants, so this must happen before ordinary font-metric lengths
    /// are projected for the formatting tree:
    /// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>.
    pub(crate) fn resolve_deferred_font_size_with_viewport(
        &mut self,
        parent: FontRelativeLengthBasis,
        viewport: LayoutSize,
    ) {
        self.resolve_deferred_font_size_with_viewport_and_root_metrics(parent, viewport, None);
    }

    pub(crate) fn resolve_deferred_font_size_with_viewport_and_root_metrics(
        &mut self,
        parent: FontRelativeLengthBasis,
        viewport: LayoutSize,
        root_metrics: Option<RootFontMetricLengthBasis>,
    ) {
        let basis = ViewportLengthBasis::for_writing_mode(viewport, self.writing_mode);
        self.font_size = clamp_used_layout_length(
            self.deferred_font_size
                .resolve_with_viewport_and_root_metrics(parent, Some(basis), root_metrics),
        )
        .points();
        let (line_height, _, _) = self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
    }

    /// Returns whether this style has an unresolved value that depends on its
    /// own used `ch` advance.
    ///
    /// `ComputedStyle` intentionally keeps `ch` components through the
    /// computed-value phase. This read-only traversal covers every
    /// style-owned metric-bearing property, including deferred CSS math,
    /// without turning an otherwise font-free style into a font lookup:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn requires_ch_advance(&self) -> bool {
        self.line_height_value.requires_ch_advance()
            || self.row_gap.requires_ch_advance()
            || self.column_gap.requires_ch_advance()
            || self.row_rule.requires_ch_advance()
            || self.column_rule.requires_ch_advance()
            || self.grid_template_rows.requires_ch_advance()
            || self.grid_template_columns.requires_ch_advance()
            || self.grid_auto_rows.requires_ch_advance()
            || self.grid_auto_columns.requires_ch_advance()
            || self.column_width.requires_ch_advance()
            || self.column_height.requires_ch_advance()
            || self.letter_spacing.requires_ch_advance()
            || self.word_spacing.requires_ch_advance()
            || self.box_values.requires_ch_advance()
            || self.border_radius.requires_ch_advance()
            || self.border_width_values.top.requires_ch_advance()
            || self.border_width_values.right.requires_ch_advance()
            || self.border_width_values.bottom.requires_ch_advance()
            || self.border_width_values.left.requires_ch_advance()
            || self.outline_width_value.requires_ch_advance()
            || self.outline_offset.requires_ch_advance()
            || self.flex_basis.requires_ch_advance()
            || self.text_indent.amount.requires_ch_advance()
            || self.vertical_align.requires_ch_advance()
            || self.tab_size.requires_ch_advance()
            || self
                .background
                .background_image
                .as_image()
                .is_some_and(BackgroundImage::requires_ch_advance)
            || self.background.background_size.requires_ch_advance()
            || self.background.background_position.requires_ch_advance()
            || self.object_position.requires_ch_advance()
            || self.object_view_box.requires_ch_advance()
            || self
                .background
                .background_layers
                .iter()
                .any(BackgroundLayer::requires_ch_advance)
            || self
                .transform
                .iter()
                .any(TransformFunction::requires_ch_advance)
            || self.individual_transforms.requires_ch_advance()
            || self.transform_origin.requires_ch_advance()
            || self.perspective.requires_ch_advance()
            || self.perspective_origin.requires_ch_advance()
            || self.border_image.requires_ch_advance()
            || self.text_decoration.requires_ch_advance()
            || self.border_spacing.requires_ch_advance()
            || self.text_shadow.iter().any(TextShadow::requires_ch_advance)
            || self.box_shadow.iter().any(BoxShadow::requires_ch_advance)
    }

    /// Returns whether resolving this style needs metrics from its selected
    /// font, rather than only the font-size fallback values.
    ///
    /// `ch`, `ic`, `ex`, and `cap` all use a selected-font metric. The broad
    /// `ch` traversal covers properties resolved in the earlier font-metric
    /// phase, while the structural checks below cover the fields resolved in
    /// the later `ic`/`ex`/`cap` phase:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn requires_selected_font_metrics(&self) -> bool {
        self.requires_ch_advance()
            || self.line_height_value.requires_selected_font_metrics()
            || self.box_values.requires_selected_font_metrics()
            || self
                .background
                .background_image
                .as_image()
                .is_some_and(BackgroundImage::requires_selected_font_metrics)
            || self
                .background
                .background_size
                .requires_selected_font_metrics()
            || self
                .background
                .background_position
                .requires_selected_font_metrics()
            || self
                .background
                .background_layers
                .iter()
                .any(BackgroundLayer::requires_selected_font_metrics)
    }

    /// Returns whether this style contains a root-font metric unit.
    ///
    /// Root-relative selected-font units use the document root's chosen face,
    /// so the root must be measured before any descendant resolves one.  The
    /// Structural predicates keep that lookup lazy for documents that never
    /// use such a unit.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn requires_root_font_metrics(&self) -> bool {
        self.deferred_font_size.requires_root_font_metrics()
            || self.line_height_value.requires_root_font_metrics()
            || self.box_values.requires_root_font_metrics()
            || self.contain_intrinsic_size.requires_root_font_metrics()
            || self.row_gap.requires_root_font_metrics()
            || self.column_gap.requires_root_font_metrics()
            || self.row_rule.requires_root_font_metrics()
            || self.column_rule.requires_root_font_metrics()
            || self.grid_template_rows.requires_root_font_metrics()
            || self.grid_template_columns.requires_root_font_metrics()
            || self.grid_auto_rows.requires_root_font_metrics()
            || self.grid_auto_columns.requires_root_font_metrics()
            || self.column_width.requires_root_font_metrics()
            || self.column_height.requires_root_font_metrics()
            || self.letter_spacing.requires_root_font_metrics()
            || self.word_spacing.requires_root_font_metrics()
            || self.border_radius.requires_root_font_metrics()
            || self.border_shape.requires_root_font_metrics()
            || self.shape_outside.requires_root_font_metrics()
            || self.shape_margin.requires_root_font_metrics()
            || self.object_view_box.requires_root_font_metrics()
            || self.border_width_values.top.requires_root_font_metrics()
            || self.border_width_values.right.requires_root_font_metrics()
            || self.border_width_values.bottom.requires_root_font_metrics()
            || self.border_width_values.left.requires_root_font_metrics()
            || self.outline_width_value.requires_root_font_metrics()
            || self.outline_offset.requires_root_font_metrics()
            || self.flex_basis.requires_root_font_metrics()
            || self.text_indent.requires_root_font_metrics()
            || self.vertical_align.requires_root_font_metrics()
            || self.tab_size.requires_root_font_metrics()
            || [
                &self.scroll_padding.top,
                &self.scroll_padding.right,
                &self.scroll_padding.bottom,
                &self.scroll_padding.left,
            ]
            .into_iter()
            .any(ScrollPadding::requires_root_font_metrics)
            || [
                &self.scroll_margin.top,
                &self.scroll_margin.right,
                &self.scroll_margin.bottom,
                &self.scroll_margin.left,
            ]
            .into_iter()
            .any(ComputedLengthPercentage::requires_root_font_metrics)
            || self
                .background
                .background_image
                .as_image()
                .is_some_and(BackgroundImage::requires_root_font_metrics)
            || self.background.background_size.requires_root_font_metrics()
            || self
                .background
                .background_position
                .requires_root_font_metrics()
            || self.object_position.requires_root_font_metrics()
            || self
                .background
                .background_layers
                .iter()
                .any(BackgroundLayer::requires_root_font_metrics)
            || self
                .transform
                .iter()
                .any(TransformFunction::requires_root_font_metrics)
            || self.individual_transforms.requires_root_font_metrics()
            || self.transform_origin.requires_root_font_metrics()
            || self.perspective.requires_root_font_metrics()
            || self.perspective_origin.requires_root_font_metrics()
            || self.border_image.requires_root_font_metrics()
            || self.text_decoration.requires_root_font_metrics()
            || self.border_spacing.requires_root_font_metrics()
            || self
                .text_shadow
                .iter()
                .any(TextShadow::requires_root_font_metrics)
            || self
                .box_shadow
                .iter()
                .any(BoxShadow::requires_root_font_metrics)
            || self.legacy_clip.requires_root_font_metrics()
            || self.clip_path.requires_root_font_metrics()
            || [
                self.marker_style.as_deref(),
                self.before_style.as_deref(),
                self.after_style.as_deref(),
                self.scroll_marker_style.as_deref(),
                self.scroll_marker_group_style.as_deref(),
                self.first_line_style.as_deref(),
                self.first_letter_style.as_deref(),
                self.footnote_call_style.as_deref(),
                self.footnote_marker_style.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(Self::requires_root_font_metrics)
    }

    /// Returns whether a generated pseudo-style needs this style's `ch`
    /// advance to resolve its deferred `font-size`.
    pub(crate) fn pseudo_styles_require_parent_ch_advance(&self) -> bool {
        [
            self.marker_style.as_deref(),
            self.before_style.as_deref(),
            self.after_style.as_deref(),
            self.scroll_marker_style.as_deref(),
            self.scroll_marker_group_style.as_deref(),
            self.footnote_call_style.as_deref(),
            self.footnote_marker_style.as_deref(),
            self.first_line_style.as_deref(),
            self.first_letter_style.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|pseudo| {
            pseudo
                .deferred_font_size
                .requires_parent_ch_advance(self.font_size)
        })
    }

    /// Finalizes the ordinary font-relative components of box-model computed
    /// values once the cascade has selected this element's font sizes.
    ///
    /// CSS Values computes `em` and `rem` from font sizes, while units based
    /// on selected-font metrics remain unresolved until layout.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn finalize_computed_font_relative_lengths(&mut self) {
        let font_size = self.font_size;
        let root_font_size = self.root_font_size;
        self.box_values
            .resolve_em_relative_lengths(layout_pt(font_size));
        self.box_values
            .resolve_root_font_relative_lengths(root_font_size);
        if let Some(image) = self.background.background_image.as_image_mut() {
            image.resolve_em_relative_lengths(layout_pt(font_size));
            image.resolve_root_font_relative_lengths(root_font_size);
        }
        self.background
            .background_size
            .resolve_em_relative_lengths(layout_pt(font_size));
        self.background
            .background_size
            .resolve_root_font_relative_lengths(root_font_size);
        self.background
            .background_position
            .resolve_em_relative_lengths(layout_pt(font_size));
        self.background
            .background_position
            .resolve_root_font_relative_lengths(root_font_size);
        for layer in &mut self.background.background_layers {
            layer.resolve_em_relative_lengths(layout_pt(font_size));
            layer.resolve_root_font_relative_lengths(root_font_size);
        }
    }

    /// Resolves font-metric-relative computed lengths for this style.
    ///
    /// CSS Values defines `ch` relative to the used advance of the "0" glyph.
    /// The cascade stores that component separately, then layout resolves it
    /// with the selected font face before formatting contexts consume used
    /// lengths:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
    /// <https://www.w3.org/TR/css-cascade-5/#used>.
    pub(crate) fn resolve_font_metric_lengths(&mut self, ch_advance: LayoutLength) {
        self.resolve_ch_relative_lengths(ch_advance);
    }

    /// Finalize all local selected-font metrics at the computed-value
    /// boundary. Each unit uses the element's inline-axis metric; physical
    /// box edges do not provide independent `ch` or `ic` bases.
    /// <https://drafts.csswg.org/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_selected_font_metric_lengths(
        &mut self,
        basis: SelectedFontMetricLengthBasis,
    ) {
        self.resolve_font_metric_lengths(basis.ch_advance);
        self.line_height_value
            .resolve_selected_font_metric_lengths(basis);
        self.box_values.resolve_selected_font_metric_lengths(basis);

        // Image values can retain metric expressions in generated geometry
        // which is not part of the box-model traversal above.
        if let Some(image) = self.background.background_image.as_image_mut() {
            image.resolve_selected_font_metric_lengths(basis);
        }
        self.background
            .background_size
            .resolve_selected_font_metric_lengths(basis);
        self.background
            .background_position
            .resolve_selected_font_metric_lengths(basis);
        for layer in &mut self.background.background_layers {
            layer.resolve_selected_font_metric_lengths(basis);
        }

        let (line_height, _, _) = self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
    }

    fn resolve_ch_relative_lengths(&mut self, ch_advance: LayoutLength) {
        self.line_height_value
            .resolve_font_metric_lengths(ch_advance);
        let (line_height, _, _) = self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
        self.row_gap.resolve_font_metric_lengths(ch_advance);
        self.column_gap.resolve_font_metric_lengths(ch_advance);
        self.row_rule.resolve_font_metric_lengths(ch_advance);
        self.column_rule.resolve_font_metric_lengths(ch_advance);
        self.grid_template_rows
            .resolve_font_metric_lengths(ch_advance);
        self.grid_template_columns
            .resolve_font_metric_lengths(ch_advance);
        self.grid_auto_rows.resolve_font_metric_lengths(ch_advance);
        self.grid_auto_columns
            .resolve_font_metric_lengths(ch_advance);
        self.column_width.resolve_font_metric_lengths(ch_advance);
        self.column_height.resolve_font_metric_lengths(ch_advance);
        self.letter_spacing.resolve_font_metric_lengths(ch_advance);
        self.word_spacing.resolve_font_metric_lengths(ch_advance);
        self.box_values.resolve_ch_relative_lengths(ch_advance);
        self.border_radius.resolve_font_metric_lengths(ch_advance);
        self.object_view_box.resolve_font_metric_lengths(ch_advance);
        self.border_width_values
            .top
            .resolve_font_metric_lengths(ch_advance);
        self.border_width_values
            .right
            .resolve_font_metric_lengths(ch_advance);
        self.border_width_values
            .bottom
            .resolve_font_metric_lengths(ch_advance);
        self.border_width_values
            .left
            .resolve_font_metric_lengths(ch_advance);
        self.resolve_used_border_widths();
        self.outline_width_value
            .resolve_font_metric_lengths(ch_advance);
        self.outline_width = self.outline_width_value.length_max_zero().points();
        self.outline_offset.resolve_font_metric_lengths(ch_advance);
        self.flex_basis.resolve_font_metric_lengths(ch_advance);
        self.text_indent
            .amount
            .resolve_font_metric_lengths(ch_advance);
        self.vertical_align.resolve_font_metric_lengths(ch_advance);
        self.tab_size.resolve_font_metric_lengths(ch_advance);
        if let Some(image) = self.background.background_image.as_image_mut() {
            image.resolve_font_metric_lengths(ch_advance);
        }
        self.background
            .background_size
            .resolve_font_metric_lengths(ch_advance);
        self.background
            .background_position
            .resolve_font_metric_lengths(ch_advance);
        self.object_position.resolve_font_metric_lengths(ch_advance);
        for layer in &mut self.background.background_layers {
            layer.resolve_font_metric_lengths(ch_advance);
        }
        for function in &mut self.transform {
            function.resolve_font_metric_lengths(ch_advance);
        }
        self.individual_transforms
            .resolve_font_metric_lengths(ch_advance);
        self.transform_origin
            .resolve_font_metric_lengths(ch_advance);
        self.perspective.resolve_font_metric_lengths(ch_advance);
        self.perspective_origin
            .resolve_font_metric_lengths(ch_advance);
        self.border_image.resolve_font_metric_lengths(ch_advance);
        self.text_decoration.resolve_font_metric_lengths(ch_advance);
        self.border_spacing.resolve_font_metric_lengths(ch_advance);
        for shadow in &mut self.text_shadow {
            shadow.resolve_font_metric_lengths(ch_advance);
        }
        for shadow in &mut self.box_shadow {
            shadow.resolve_font_metric_lengths(ch_advance);
        }
    }

    /// Resolves ordinary `lh` components after this style's computed line
    /// height is available. `line-height` itself is intentionally excluded:
    /// CSS Values gives `lh` in that property the inherited line-height basis.
    /// <https://www.w3.org/TR/css-values-4/#lh>
    pub(crate) fn resolve_line_height_relative_lengths(&mut self) {
        let line_height = layout_pt(self.line_height);
        self.box_values
            .resolve_line_height_relative_lengths(line_height);
        self.background
            .background_size
            .resolve_line_height_relative_lengths(line_height);
        self.background
            .background_position
            .resolve_line_height_relative_lengths(line_height);
        if let Some(image) = self.background.background_image.as_image_mut() {
            image.resolve_line_height_relative_lengths(line_height);
        }
        for layer in &mut self.background.background_layers {
            layer.resolve_line_height_relative_lengths(line_height);
        }
    }

    /// Resolves root-font metric components against the one document-root
    /// metric snapshot shared by every element and pseudo-element.
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
    pub(crate) fn resolve_root_font_metric_lengths(&mut self, basis: RootFontMetricLengthBasis) {
        self.line_height_value
            .resolve_root_font_metric_lengths(basis);
        let (line_height, _, _) = self.line_height_value.clone().projected(self.font_size);
        self.line_height = line_height;
        self.box_values.resolve_root_font_metric_lengths(basis);
        self.contain_intrinsic_size
            .resolve_root_font_metric_lengths(basis);
        self.row_gap.resolve_root_font_metric_lengths(basis);
        self.column_gap.resolve_root_font_metric_lengths(basis);
        self.row_rule.resolve_root_font_metric_lengths(basis);
        self.column_rule.resolve_root_font_metric_lengths(basis);
        self.grid_template_rows
            .resolve_root_font_metric_lengths(basis);
        self.grid_template_columns
            .resolve_root_font_metric_lengths(basis);
        self.grid_auto_rows.resolve_root_font_metric_lengths(basis);
        self.grid_auto_columns
            .resolve_root_font_metric_lengths(basis);
        self.column_width.resolve_root_font_metric_lengths(basis);
        self.column_height.resolve_root_font_metric_lengths(basis);
        self.letter_spacing.resolve_root_font_metric_lengths(basis);
        self.word_spacing.resolve_root_font_metric_lengths(basis);
        self.border_radius.resolve_root_font_metric_lengths(basis);
        self.border_shape.resolve_root_font_metric_lengths(basis);
        self.shape_outside.resolve_root_font_metric_lengths(basis);
        self.shape_margin.resolve_root_font_metric_lengths(basis);
        self.object_view_box.resolve_root_font_metric_lengths(basis);
        self.border_width_values
            .top
            .resolve_root_font_metric_lengths(basis);
        self.border_width_values
            .right
            .resolve_root_font_metric_lengths(basis);
        self.border_width_values
            .bottom
            .resolve_root_font_metric_lengths(basis);
        self.border_width_values
            .left
            .resolve_root_font_metric_lengths(basis);
        self.resolve_used_border_widths();
        self.outline_width_value
            .resolve_root_font_metric_lengths(basis);
        self.outline_width = self.outline_width_value.length_max_zero().points();
        self.outline_offset.resolve_root_font_metric_lengths(basis);
        self.flex_basis.resolve_root_font_metric_lengths(basis);
        self.text_indent.resolve_root_font_metric_lengths(basis);
        self.vertical_align.resolve_root_font_metric_lengths(basis);
        self.tab_size.resolve_root_font_metric_lengths(basis);
        for edge in [
            &mut self.scroll_padding.top,
            &mut self.scroll_padding.right,
            &mut self.scroll_padding.bottom,
            &mut self.scroll_padding.left,
        ] {
            edge.resolve_root_font_metric_lengths(basis);
        }
        for edge in [
            &mut self.scroll_margin.top,
            &mut self.scroll_margin.right,
            &mut self.scroll_margin.bottom,
            &mut self.scroll_margin.left,
        ] {
            edge.resolve_root_font_metric_lengths(basis);
        }
        if let Some(image) = self.background.background_image.as_image_mut() {
            image.resolve_root_font_metric_lengths(basis);
        }
        self.background
            .background_size
            .resolve_root_font_metric_lengths(basis);
        self.background
            .background_position
            .resolve_root_font_metric_lengths(basis);
        self.object_position.resolve_root_font_metric_lengths(basis);
        for layer in &mut self.background.background_layers {
            layer.resolve_root_font_metric_lengths(basis);
        }
        for function in &mut self.transform {
            function.resolve_root_font_metric_lengths(basis);
        }
        self.individual_transforms
            .resolve_root_font_metric_lengths(basis);
        self.transform_origin
            .resolve_root_font_metric_lengths(basis);
        self.perspective.resolve_root_font_metric_lengths(basis);
        self.perspective_origin
            .resolve_root_font_metric_lengths(basis);
        self.border_image.resolve_root_font_metric_lengths(basis);
        self.text_decoration.resolve_root_font_metric_lengths(basis);
        self.border_spacing.resolve_root_font_metric_lengths(basis);
        for shadow in &mut self.text_shadow {
            shadow.resolve_root_font_metric_lengths(basis);
        }
        for shadow in &mut self.box_shadow {
            shadow.resolve_root_font_metric_lengths(basis);
        }
        self.legacy_clip.resolve_root_font_metric_lengths(basis);
        self.clip_path.resolve_root_font_metric_lengths(basis);
    }

    /// Resolves font-metric-relative lengths while preserving physical
    /// block-size constraints for a later formatting-context-specific pass.
    ///
    /// CSS Tables consumes table-cell `height`/`min-height`/`max-height` as
    /// row-axis constraints, even when the cell's own writing mode is
    /// orthogonal. Keeping those values metric-aware lets table layout resolve
    /// their `ch` components against the row-axis writing context instead of
    /// the cell content writing context.
    pub(crate) fn resolve_font_metric_lengths_preserving_box_block_sizes(
        &mut self,
        ch_advance: LayoutLength,
    ) {
        let height = self.box_values.height.clone();
        let min_height = self.box_values.min_height.clone();
        let max_height = self.box_values.max_height.clone();
        self.resolve_font_metric_lengths(ch_advance);
        self.box_values.height = height;
        self.box_values.min_height = min_height;
        self.box_values.max_height = max_height;
    }

    /// Resolves viewport-relative computed lengths for paged layout.
    ///
    /// CSS Values defines viewport-percentage lengths against the initial
    /// containing block. CSS Paged Media uses the page area as the initial
    /// containing block for document layout, so layout projects these
    /// components to absolute lengths after the active page context is known:
    /// <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths> and
    /// <https://www.w3.org/TR/css-page-3/#page-model>.
    pub(crate) fn resolve_viewport_lengths_for_viewport(&mut self, physical_viewport: LayoutSize) {
        let basis = ViewportLengthBasis::for_writing_mode(physical_viewport, self.writing_mode);
        ResolveViewportLengths::resolve_viewport_lengths(self, basis);
    }

    /// Resolve viewport lengths plus container-relative terms selected by the
    /// active layout ancestor context. The container basis is deliberately a
    /// used-value input: computed styles remain reusable across layout
    /// replays and formatting contexts.
    /// <https://drafts.csswg.org/css-conditional-5/#container-lengths>
    pub(crate) fn resolve_viewport_lengths_for_viewport_and_container(
        &mut self,
        physical_viewport: LayoutSize,
        container_physical: LayoutSize,
    ) {
        let basis = ViewportLengthBasis::for_writing_mode(physical_viewport, self.writing_mode)
            .with_container_physical(container_physical);
        ResolveViewportLengths::resolve_viewport_lengths(self, basis);
    }

    /// Returns whether either the legacy transform list or an independent
    /// transform property applies a non-initial transformation.
    pub(crate) fn has_transform(&self) -> bool {
        !self.transform.is_empty() || !self.individual_transforms.is_none()
    }

    /// Whether CSS Transforms establishes the stacking/positioning effects of
    /// either a transform or a specified `transform-style: preserve-3d`.
    ///
    /// Grouping properties can flatten the *used* transform style, but the
    /// specified `preserve-3d` still establishes the containing block and
    /// stacking context required by CSS Transforms Level 2.
    /// <https://drafts.csswg.org/css-transforms-2/#transform-style-property>
    pub(crate) fn has_transform_or_preserve_3d(&self) -> bool {
        self.has_transform()
            || !matches!(self.perspective, ComputedPerspective::None)
            || self.transform_style == TransformStyle::Preserve3d
    }

    pub(crate) fn transform_applicability(&self) -> TransformApplicability {
        if self.display.is_ruby() || self.display.is_ruby_internal() {
            TransformApplicability::NonTransformableRubyInternal
        } else {
            TransformApplicability::Transformable
        }
    }

    /// Remove transforms that are specified on a non-transformable ruby box
    /// before that style reaches inline geometry or paint setup.
    pub(crate) fn suppress_inapplicable_transform(&mut self) {
        if self.transform_applicability() == TransformApplicability::NonTransformableRubyInternal {
            self.transform.clear();
            self.individual_transforms = IndividualTransforms::NONE;
            self.perspective = ComputedPerspective::NONE;
        }
    }

    /// Return the used `letter-spacing` length in layout units.
    ///
    /// CSS Text Level 4 accepts `normal | <length-percentage>` for
    /// `letter-spacing`; percentages resolve against the used font size and
    /// font-relative components such as `ch` resolve before layout consumes
    /// the used value:
    /// <https://drafts.csswg.org/css-text-4/#letter-spacing-property> and
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths>.
    pub(crate) fn used_letter_spacing(&self) -> LayoutLength {
        self.letter_spacing
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(self.font_size)))
            .unwrap_or(self.letter_spacing.fixed_component())
    }

    /// Return the used `word-spacing` length in layout units.
    ///
    /// CSS Text Level 4 defines `word-spacing` as an inherited spacing
    /// adjustment applied between words. Percentages inherit intact and resolve
    /// against the current element's used font size before text shaping and
    /// line breaking consume the used value:
    /// <https://www.w3.org/TR/css-text-4/#word-spacing-property>.
    pub(crate) fn used_word_spacing(&self) -> LayoutLength {
        self.word_spacing
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(self.font_size)))
            .unwrap_or(self.word_spacing.fixed_component())
    }
}

/// Resolve a visible CSS border width to the layout used value.
///
/// A used border width remains the specified non-negative CSS length.
///
/// Device-pixel snapping belongs to rasterization, not CSS layout. Rounding
/// here changes box geometry (for example, `border: 1pt` becomes 0.75pt) and
/// quantizes the visible edge at the renderer's CSS-pixel boundary while
/// retaining the specified value separately for subsequent cascade work.
/// <https://www.w3.org/TR/css-backgrounds-3/#border-width>
fn used_css_border_width(points: f32) -> f32 {
    if !points.is_finite() || points <= 0.0 {
        return 0.0;
    }
    points
}

fn scaled_edges(edges: Edges, factor: f32) -> Edges {
    Edges {
        top: edges.top * factor,
        right: edges.right * factor,
        bottom: edges.bottom * factor,
        left: edges.left * factor,
    }
}

impl ResolveViewportLengths for ComputedStyle {
    fn resolve_viewport_lengths(&mut self, basis: ViewportLengthBasis) {
        self.row_gap.resolve_viewport_lengths(basis);
        self.column_gap.resolve_viewport_lengths(basis);
        self.row_rule.resolve_viewport_lengths(basis);
        self.column_rule.resolve_viewport_lengths(basis);
        self.grid_template_rows.resolve_viewport_lengths(basis);
        self.grid_template_columns.resolve_viewport_lengths(basis);
        self.grid_auto_rows.resolve_viewport_lengths(basis);
        self.grid_auto_columns.resolve_viewport_lengths(basis);
        self.grid_lanes_flow_tolerance
            .resolve_viewport_lengths(basis);
        self.column_width.resolve_viewport_lengths(basis);
        self.column_height.resolve_viewport_lengths(basis);
        self.letter_spacing.resolve_viewport_lengths(basis);
        self.word_spacing.resolve_viewport_lengths(basis);
        self.box_values.resolve_viewport_lengths(basis);
        self.border_radius.resolve_viewport_lengths(basis);
        self.border_width_values.top.resolve_viewport_lengths(basis);
        self.border_width_values
            .right
            .resolve_viewport_lengths(basis);
        self.border_width_values
            .bottom
            .resolve_viewport_lengths(basis);
        self.border_width_values
            .left
            .resolve_viewport_lengths(basis);
        self.resolve_used_border_widths();
        self.outline_width_value.resolve_viewport_lengths(basis);
        self.outline_width = self.outline_width_value.length_max_zero().points();
        self.outline_offset.resolve_viewport_lengths(basis);
        for edge in [
            &mut self.scroll_padding.top,
            &mut self.scroll_padding.right,
            &mut self.scroll_padding.bottom,
            &mut self.scroll_padding.left,
        ] {
            if let ScrollPadding::LengthPercentage(value) = edge {
                value.resolve_viewport_lengths(basis);
            }
        }
        for edge in [
            &mut self.scroll_margin.top,
            &mut self.scroll_margin.right,
            &mut self.scroll_margin.bottom,
            &mut self.scroll_margin.left,
        ] {
            edge.resolve_viewport_lengths(basis);
        }
        self.flex_basis.resolve_viewport_lengths(basis);
        self.text_indent.amount.resolve_viewport_lengths(basis);
        self.vertical_align.resolve_viewport_lengths(basis);
        self.tab_size.resolve_viewport_lengths(basis);
        if let Some(image) = self.background.background_image.as_image_mut() {
            image.resolve_viewport_lengths(basis);
        }
        self.background
            .background_size
            .resolve_viewport_lengths(basis);
        self.background
            .background_position
            .resolve_viewport_lengths(basis);
        self.object_position.resolve_viewport_lengths(basis);
        for layer in &mut self.background.background_layers {
            layer.resolve_viewport_lengths(basis);
        }
        for function in &mut self.transform {
            function.resolve_viewport_lengths(basis);
        }
        self.individual_transforms.resolve_viewport_lengths(basis);
        self.transform_origin.resolve_viewport_lengths(basis);
        self.perspective_origin.resolve_viewport_lengths(basis);
        self.object_view_box.resolve_viewport_lengths(basis);
        self.border_image.resolve_viewport_lengths(basis);
        self.text_decoration.resolve_viewport_lengths(basis);
        self.border_spacing.resolve_viewport_lengths(basis);
        for shadow in &mut self.text_shadow {
            shadow.resolve_viewport_lengths(basis);
        }
        for shadow in &mut self.box_shadow {
            shadow.resolve_viewport_lengths(basis);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_language_keeps_authored_validity_distinct_from_unknown() {
        assert_eq!(
            ContentLanguage::from_html_attribute(""),
            ContentLanguage::Unknown
        );
        assert_eq!(
            ContentLanguage::from_html_attribute("   "),
            ContentLanguage::Unknown
        );

        let unregistered = ContentLanguage::from_html_attribute("qaa");
        assert_eq!(unregistered.as_deref(), Some("qaa"));

        let malformed = ContentLanguage::from_html_attribute("ja_Hang");
        assert_eq!(malformed.as_deref(), None);
        let ContentLanguage::Tagged(tag) = malformed else {
            panic!("nonempty malformed language remains tagged")
        };
        assert_eq!(tag.as_str(), "ja_Hang");
    }

    #[test]
    fn constrained_css_scalars_preserve_their_invariants() {
        assert!(FlexGrowFactor::new(f32::INFINITY).unwrap().is_infinite());
        assert!(FlexShrinkFactor::new(-1.0).is_none());
        assert_eq!(Opacity::new_clamped(2.0).unwrap(), 1.0);
        assert_eq!(ShapeImageThreshold::new_clamped(-0.5).unwrap(), 0.0);
        assert!(Orphans::try_new(0).is_none());
        assert_eq!(Widows::try_new(3).unwrap().get(), 3);
    }

    #[test]
    fn upright_vertical_text_uses_ltr_direction_without_rewriting_computed_value() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.direction = Direction::Rtl;
        style.text_orientation = TextOrientation::Upright;

        assert_eq!(style.direction, Direction::Rtl);
        assert_eq!(style.used_direction(), Direction::Ltr);

        style.writing_mode = WritingMode::SidewaysLr;
        assert_eq!(style.used_direction(), Direction::Rtl);
    }

    #[test]
    fn zoom_keeps_computed_zero_but_normalizes_its_used_factor() {
        let zero = CssZoom::parse("0").unwrap();
        let percentage = CssZoom::parse("150%").unwrap();

        assert_eq!(zero.factor(), 0.0);
        assert_eq!(zero.used_factor(), 1.0);
        assert_eq!(percentage.used_factor(), 1.5);
        assert_eq!(
            EffectiveZoom::from_parent_and_local(
                EffectiveZoom::from_parent_and_local(EffectiveZoom::NORMAL, percentage),
                percentage,
            )
            .factor(),
            2.25
        );
        assert!(CssZoom::parse("-1").is_none());
    }

    #[test]
    fn effective_zoom_scales_fixed_lengths_without_scaling_percentages() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.box_values.width = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_affine(layout_pt(9.0), 0.5, true),
        );
        style.font_size = 10.0;
        style.line_height = 12.0;
        let style = LayoutStyle::from_computed(&style).into_zoomed();

        let ComputedLengthPercentageOrAuto::LengthPercentage(width) = &style.box_values.width
        else {
            panic!("test width remains a length-percentage");
        };
        assert_eq!(
            width
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(68.0)
        );
        assert_eq!(style.font_size, 20.0);
        assert_eq!(style.line_height, 24.0);
    }

    #[test]
    fn effective_zoom_scales_shadow_lengths_without_scaling_percentages() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.text_shadow = vec![TextShadow {
            color: TextShadowColor::CurrentColor,
            offset_x: ComputedLengthPercentage::from_affine(layout_pt(3.0), 0.1, true),
            offset_y: ComputedLengthPercentage::from_points(4.0),
            blur_radius: ComputedLengthPercentage::from_points(5.0),
            spread: ComputedLengthPercentage::from_affine(layout_pt(-2.0), 0.2, true),
            inset: false,
        }];
        style.box_shadow = vec![BoxShadow {
            color: BoxShadowColor::CssColor(CssColor::new(1, 2, 3)),
            offset_x: ComputedLengthPercentage::from_points(6.0),
            offset_y: ComputedLengthPercentage::from_points(7.0),
            blur_radius: ComputedLengthPercentage::from_points(8.0),
            spread: ComputedLengthPercentage::from_points(9.0),
            inset: true,
        }];

        let style = LayoutStyle::from_computed(&style).into_zoomed();
        let text_shadow = &style.text_shadow[0];
        assert_eq!(text_shadow.color, TextShadowColor::CurrentColor);
        assert_eq!(text_shadow.offset_x.length_points(), 6.0);
        assert_eq!(text_shadow.offset_x.percentage_coefficient_or_zero(), 0.1);
        assert_eq!(text_shadow.offset_y.length_points(), 8.0);
        assert_eq!(text_shadow.blur_radius.length_points(), 10.0);
        assert_eq!(text_shadow.spread.length_points(), -4.0);
        assert_eq!(text_shadow.spread.percentage_coefficient_or_zero(), 0.2);

        let box_shadow = &style.box_shadow[0];
        assert_eq!(
            box_shadow.color,
            BoxShadowColor::CssColor(CssColor::new(1, 2, 3))
        );
        assert!(box_shadow.inset);
        assert_eq!(box_shadow.offset_x.length_points(), 12.0);
        assert_eq!(box_shadow.offset_y.length_points(), 14.0);
        assert_eq!(box_shadow.blur_radius.length_points(), 16.0);
        assert_eq!(box_shadow.spread.length_points(), 18.0);
    }

    #[test]
    fn effective_zoom_scales_fixed_position_insets_once() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.box_values.inset_left = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_affine(layout_pt(9.0), 0.25, true),
        );
        style.box_values.inset_top = ComputedLengthPercentageOrAuto::LengthPercentage(
            ComputedLengthPercentage::from_points(7.0),
        );
        style.box_values.inset_right = ComputedLengthPercentageOrAuto::AUTO;

        let style = LayoutStyle::from_computed(&style).into_zoomed();

        let ComputedLengthPercentageOrAuto::LengthPercentage(left) = &style.box_values.inset_left
        else {
            panic!("left inset remains a length-percentage");
        };
        assert_eq!(left.length_points(), 18.0);
        assert_eq!(left.percentage_coefficient_or_zero(), 0.25);
        let ComputedLengthPercentageOrAuto::LengthPercentage(top) = &style.box_values.inset_top
        else {
            panic!("top inset remains a length-percentage");
        };
        assert_eq!(top.length_points(), 14.0);
        assert!(style.box_values.inset_right.is_auto());
    }

    #[test]
    fn effective_zoom_scales_flex_lengths_without_scaling_percentages() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.flex_basis = ComputedFlexBasis::LengthPercentage(ComputedFlexBasisLength::new(
            ComputedLengthPercentage::from_affine(layout_pt(9.0), 0.5, true),
        ));
        style.row_gap = ComputedGap::LengthPercentage(ComputedLengthPercentage::from_affine(
            layout_pt(3.0),
            0.25,
            true,
        ));
        style.column_gap =
            ComputedGap::LengthPercentage(ComputedLengthPercentage::from_points(7.0));

        let style = LayoutStyle::from_computed(&style).into_zoomed();

        let ComputedFlexBasis::LengthPercentage(basis) = &style.flex_basis else {
            panic!("flex basis remains a length-percentage");
        };
        assert_eq!(
            basis
                .value
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(68.0)
        );
        let ComputedGap::LengthPercentage(row_gap) = &style.row_gap else {
            panic!("row gap remains a length-percentage");
        };
        assert_eq!(
            row_gap
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(31.0)
        );
        let ComputedGap::LengthPercentage(column_gap) = &style.column_gap else {
            panic!("column gap remains a length-percentage");
        };
        assert_eq!(column_gap.length_points(), 14.0);
    }

    #[test]
    fn effective_zoom_scales_multicol_dimensions_and_rule_geometry_once() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.column_width = ComputedColumnWidth::Length(ComputedLengthPercentage::from_affine(
            layout_pt(9.0),
            0.0,
            false,
        ));
        style.column_height = ComputedColumnHeight::Length(ComputedLengthPercentage::from_affine(
            layout_pt(7.0),
            0.0,
            false,
        ));
        style.column_rule.widths = GapRuleList::from_parts(
            vec![GapRuleListComponent::Value(
                ComputedLengthPercentage::from_affine(layout_pt(3.0), 0.25, true),
            )],
            Some(vec![ComputedLengthPercentage::from_points(5.0)]),
            Vec::new(),
        );
        style.column_rule.inset_cap_start = GapRuleInsetValue::LengthPercentage(
            ComputedLengthPercentage::from_affine(layout_pt(2.0), 0.5, true),
        );

        let style = LayoutStyle::from_computed(&style).into_zoomed();

        let ComputedColumnWidth::Length(width) = &style.column_width else {
            panic!("column width remains a length");
        };
        assert_eq!(width.length_points(), 18.0);
        let ComputedColumnHeight::Length(height) = &style.column_height else {
            panic!("column height remains a length");
        };
        assert_eq!(height.length_points(), 14.0);
        let first_rule = style.column_rule.widths.value_for_index(0, 2).unwrap();
        assert_eq!(
            first_rule
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(31.0),
        );
        assert_eq!(
            style
                .column_rule
                .widths
                .value_for_index(1, 2)
                .unwrap()
                .length_points(),
            10.0,
        );
        let GapRuleInsetValue::LengthPercentage(inset) = &style.column_rule.inset_cap_start else {
            panic!("fixed rule inset remains a length percentage");
        };
        assert_eq!(
            inset
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(54.0),
        );
    }

    #[test]
    fn effective_zoom_scales_fixed_border_spacing_once_without_scaling_percentages() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.border_spacing = CascadedTableBorderSpacing::NonAuthor(BorderSpacing {
            horizontal: ComputedLengthPercentage::from_affine(layout_pt(7.0), 0.25, true),
            vertical: ComputedLengthPercentage::from_points(11.0),
        });

        let style = LayoutStyle::from_computed(&style).into_zoomed();

        assert_eq!(
            style
                .border_spacing
                .horizontal
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(39.0),
        );
        assert_eq!(style.border_spacing.vertical.length_points(), 22.0);
    }

    #[test]
    fn effective_zoom_scales_fixed_grid_tracks_without_scaling_percentages_or_fr() {
        let fixed_and_percentage =
            ComputedLengthPercentage::from_affine(layout_pt(6.0), 0.25, true);
        let fixed_fit_content = ComputedLengthPercentage::from_points(8.0);
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.grid_template_columns = GridTrackList::Tracks {
            components: vec![GridTrackListComponent::Repeat(
                Vec::new(),
                GridRepeat {
                    count: GridRepeatCount::Number(2),
                    tracks: vec![
                        GridTrackListComponent::Track(
                            Vec::new(),
                            GridTrackSize {
                                min: GridMinTrackBreadth::LengthPercentage(fixed_and_percentage),
                                max: GridMaxTrackBreadth::Flex(1.0),
                            },
                        ),
                        GridTrackListComponent::Track(
                            Vec::new(),
                            GridTrackSize {
                                min: GridMinTrackBreadth::Auto,
                                max: GridMaxTrackBreadth::FitContent(fixed_fit_content),
                            },
                        ),
                    ],
                    trailing_names: Vec::new(),
                },
            )],
            trailing_names: Vec::new(),
        };
        style.grid_auto_rows = GridAutoTrackList::from_tracks(vec![GridTrackSize {
            min: GridMinTrackBreadth::LengthPercentage(ComputedLengthPercentage::from_points(9.0)),
            max: GridMaxTrackBreadth::MaxContent,
        }])
        .expect("test grid auto-track list is non-empty");

        let style = LayoutStyle::from_computed(&style).into_zoomed();

        let GridTrackList::Tracks { components, .. } = &style.grid_template_columns else {
            panic!("grid template remains an explicit track list");
        };
        let GridTrackListComponent::Repeat(_, repeat) = &components[0] else {
            panic!("nested repeat remains present");
        };
        let GridTrackListComponent::Track(_, fixed_track) = &repeat.tracks[0] else {
            panic!("fixed track remains present");
        };
        let GridMinTrackBreadth::LengthPercentage(value) = &fixed_track.min else {
            panic!("track minimum remains a length-percentage");
        };
        assert_eq!(
            value
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(100.0)))
                .unwrap(),
            layout_pt(37.0)
        );
        assert!(matches!(fixed_track.max, GridMaxTrackBreadth::Flex(1.0)));
        let GridTrackListComponent::Track(_, fit_track) = &repeat.tracks[1] else {
            panic!("fit-content track remains present");
        };
        let GridMaxTrackBreadth::FitContent(value) = &fit_track.max else {
            panic!("track maximum remains fit-content");
        };
        assert_eq!(value.length_points(), 16.0);
        let GridMinTrackBreadth::LengthPercentage(value) = &style
            .grid_auto_rows
            .get(0)
            .expect("test grid auto-track list is non-empty")
            .min
        else {
            panic!("implicit track minimum remains a length-percentage");
        };
        assert_eq!(value.length_points(), 18.0);
    }

    #[test]
    fn effective_zoom_scales_fixed_grid_lanes_flow_tolerance_without_scaling_percentages() {
        let mut style = ComputedStyle::initial();
        style.effective_zoom = EffectiveZoom(2.0);
        style.grid_lanes_flow_tolerance = GridLanesFlowTolerance::LengthPercentage(
            ComputedLengthPercentage::from_affine(layout_pt(51.0), 0.17, true),
        );

        let style = LayoutStyle::from_computed(&style).into_zoomed();

        let GridLanesFlowTolerance::LengthPercentage(value) = &style.grid_lanes_flow_tolerance
        else {
            panic!("flow tolerance remains a length-percentage");
        };
        assert_eq!(value.length_points(), 102.0);
        assert_eq!(
            value
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(300.0)))
                .unwrap(),
            layout_pt(153.0)
        );
    }

    #[test]
    fn cloned_styles_do_not_cross_contaminate_shared_length_expression_trees() {
        let mut original = ComputedStyle::initial();
        original.box_values.width =
            ComputedLengthPercentageOrAuto::LengthPercentage(ComputedLengthPercentage::sum(
                ComputedLengthPercentage::from_rem(1.0),
                ComputedLengthPercentage::from_em(1.0),
            ));

        let mut first = original.clone();
        first.font_size = 10.0;
        first.root_font_size = 20.0;
        first.finalize_computed_font_relative_lengths();

        let mut second = original.clone();
        second.font_size = 30.0;
        second.root_font_size = 40.0;
        second.finalize_computed_font_relative_lengths();

        assert_eq!(first.box_values.width.length_if_no_percent(), Some(30.0));
        assert_eq!(second.box_values.width.length_if_no_percent(), Some(70.0));
        assert_eq!(original.box_values.width.length_if_no_percent(), None);
    }

    #[test]
    fn root_metric_resolution_reaches_non_box_length_carriers() {
        let mut style = ComputedStyle::initial();
        style.row_gap = ComputedGap::LengthPercentage(ComputedLengthPercentage::from_rch(1.0));
        style.column_width = ComputedColumnWidth::Length(ComputedLengthPercentage::from_rcap(1.0));
        style.flex_basis = ComputedFlexBasis::LengthPercentage(ComputedFlexBasisLength::new(
            ComputedLengthPercentage::from_ric(1.0),
        ));
        style.transform_origin.z = ComputedLengthPercentage::from_rlh(1.0);
        style.outline_offset = ComputedLengthPercentage::from_rex(1.0);

        style.resolve_root_font_metric_lengths(RootFontMetricLengthBasis {
            font_size: layout_pt(10.0),
            ch_advance: layout_pt(2.0),
            x_height: layout_pt(3.0),
            cap_height: layout_pt(4.0),
            ic_advance: layout_pt(5.0),
            line_height: layout_pt(6.0),
        });

        let ComputedGap::LengthPercentage(row_gap) = style.row_gap else {
            panic!("row gap remains a length-percentage");
        };
        assert_eq!(row_gap.length_points(), 2.0);
        let ComputedColumnWidth::Length(column_width) = style.column_width else {
            panic!("column width remains a length");
        };
        assert_eq!(column_width.length_points(), 4.0);
        let ComputedFlexBasis::LengthPercentage(flex_basis) = style.flex_basis else {
            panic!("flex basis remains a length-percentage");
        };
        assert_eq!(flex_basis.value.length_points(), 5.0);
        assert_eq!(style.transform_origin.z.length_points(), 6.0);
        assert_eq!(style.outline_offset.length_points(), 3.0);
    }

    #[test]
    fn visible_border_widths_preserve_used_css_lengths() {
        assert_eq!(used_css_border_width(0.0), 0.0);
        assert_eq!(
            used_css_border_width(0.3 * CSS_PX_TO_PT),
            0.3 * CSS_PX_TO_PT
        );
        assert_eq!(
            used_css_border_width(0.9 * CSS_PX_TO_PT),
            0.9 * CSS_PX_TO_PT
        );
        assert_eq!(
            used_css_border_width(1.9 * CSS_PX_TO_PT),
            1.9 * CSS_PX_TO_PT
        );
        assert_eq!(
            used_css_border_width(3.9 * CSS_PX_TO_PT),
            3.9 * CSS_PX_TO_PT
        );
    }

    #[test]
    fn non_none_perspective_establishes_transform_effects() {
        let mut style = ComputedStyle::initial();
        assert!(!style.has_transform_or_preserve_3d());
        style.perspective = ComputedPerspective::Distance(
            NonNegativeComputedLength::new(ComputedLengthPercentage::ZERO)
                .expect("zero perspective is valid"),
        );
        assert!(style.has_transform_or_preserve_3d());
        style.perspective = ComputedPerspective::NONE;
        assert!(!style.has_transform_or_preserve_3d());
    }
}
