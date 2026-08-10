use super::*;
use crate::document::paint::geometry::AxisSelectivePaintClip;
use crate::layout::baseline::{
    BaselinePair, PhysicalBaselineSets, PhysicalLeftBaselineAxis, PhysicalLeftBaselineOffset,
    PhysicalTopBaselineAxis, PhysicalTopBaselineOffset,
};
use std::num::NonZeroUsize;
use std::ops::{Add, Deref, DerefMut, Sub};

/// A computed style after the flex formatting-context used-value boundary.
///
/// The DOM and frozen box tree retain computed styles so descendants cascade
/// from unscaled values.  Flex sizing, placement, and replay instead consume
/// this marker, whose constructor requires the one-time CSS `zoom` conversion
/// to have completed.  Keeping these stages distinct prevents an already
/// zoomed parent from being reused as a cascade parent.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
/// <https://drafts.csswg.org/css-flexbox-1/#layout-algorithm>
#[derive(Debug, Clone)]
pub(super) struct FlexUsedStyle(css::ZoomedLayoutStyle);

impl FlexUsedStyle {
    pub(super) fn from_normalized(style: css::ZoomedLayoutStyle) -> Self {
        Self(style)
    }

    pub(super) fn as_computed(&self) -> &ComputedStyle {
        &self.0
    }
}

impl Deref for FlexUsedStyle {
    type Target = ComputedStyle;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FlexUsedStyle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Marker for the resolved flex main axis.
///
/// The main and cross axes are resolved only after CSS Writing Modes maps the
/// specified `flex-direction` into the physical layout coordinate system:
/// <https://www.w3.org/TR/css-flexbox-1/#main-axis>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct FlexMainAxis;

/// Marker for the resolved flex cross axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct FlexCrossAxis;

/// A coordinate relative to the start of a resolved flex axis.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexAxisOffset<Axis>(f32, std::marker::PhantomData<Axis>);

impl<Axis> FlexAxisOffset<Axis> {
    pub(super) const fn new(points: f32) -> Self {
        Self(points, std::marker::PhantomData)
    }

    pub(super) const fn points(self) -> f32 {
        self.0
    }

    pub(super) fn min(self, other: Self) -> Self {
        if self.0 <= other.0 { self } else { other }
    }

    pub(super) fn max(self, other: Self) -> Self {
        if self.0 >= other.0 { self } else { other }
    }

    /// Return this coordinate relative to an explicitly chosen axis origin.
    ///
    /// Positions may be negative after `wrap-reverse`, so a coordinate
    /// difference is not itself a non-negative extent.
    pub(super) fn relative_to(self, origin: Self) -> FlexAxisLength<Axis> {
        FlexAxisLength::new(self.0 - origin.0)
    }
}

/// A signed displacement on a resolved flex axis.
///
/// Unlike [`FlexAxisSize`], this intentionally preserves negative values:
/// used margins can be negative, and intrinsic outer-edge calculations must
/// not clamp them before the Flexbox algorithm selects its non-negative used
/// size.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexAxisLength<Axis>(f32, std::marker::PhantomData<Axis>);

impl<Axis> FlexAxisLength<Axis> {
    pub(super) const fn new(points: f32) -> Self {
        Self(points, std::marker::PhantomData)
    }

    pub(super) fn non_negative_size(self) -> FlexAxisSize<Axis> {
        FlexAxisSize::new(self.0)
    }

    pub(super) fn negated(self) -> Self {
        Self::new(-self.0)
    }

    pub(super) fn max(self, other: Self) -> Self {
        if self.0 >= other.0 { self } else { other }
    }

    pub(super) const fn points(self) -> f32 {
        self.0
    }

    /// Magnitude used only for tolerance comparisons in the Flex algorithm.
    pub(super) fn abs(self) -> f32 {
        self.0.abs()
    }

    pub(super) const fn is_negative(&self) -> bool {
        self.0 < 0.0
    }

    pub(super) const fn is_positive(&self) -> bool {
        self.0 > 0.0
    }

    pub(super) const fn is_non_positive(&self) -> bool {
        self.0 <= 0.0
    }

    /// Divide a signed Flex displacement by a non-zero item count.
    pub(super) fn divide(self, divisor: std::num::NonZeroUsize) -> Self {
        Self::new(self.0 / divisor.get() as f32)
    }

    pub(super) fn half(self) -> Self {
        Self::new(self.0 / 2.0)
    }
}

/// A non-negative extent on a resolved flex axis.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexAxisSize<Axis>(f32, std::marker::PhantomData<Axis>);

impl<Axis> FlexAxisSize<Axis> {
    pub(super) fn new(points: f32) -> Self {
        Self(points.max(0.0), std::marker::PhantomData)
    }

    pub(super) const fn points(self) -> f32 {
        self.0
    }

    pub(super) const fn is_finite(&self) -> bool {
        self.0.is_finite()
    }

    pub(super) const fn is_positive(&self) -> bool {
        self.0 > 0.0
    }

    pub(super) fn max(self, other: Self) -> Self {
        if self.0 >= other.0 { self } else { other }
    }

    pub(super) fn min(self, other: Self) -> Self {
        if self.0 <= other.0 { self } else { other }
    }

    pub(super) fn scale(self, factor: f32) -> Self {
        Self::new(self.0 * factor)
    }

    pub(super) fn divide(self, divisor: std::num::NonZeroUsize) -> Self {
        Self::new(self.0 / divisor.get() as f32)
    }
}

impl<Axis> Add for FlexAxisLength<Axis> {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.0 + other.0)
    }
}

impl<Axis> Sub for FlexAxisLength<Axis> {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.0 - other.0)
    }
}

impl<Axis> Add for FlexAxisSize<Axis> {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.0 + other.0)
    }
}

impl<Axis> Sub for FlexAxisSize<Axis> {
    type Output = FlexAxisLength<Axis>;

    fn sub(self, other: Self) -> Self::Output {
        FlexAxisLength::new(self.0 - other.0)
    }
}

impl<Axis> Sub<FlexAxisLength<Axis>> for FlexAxisSize<Axis> {
    type Output = FlexAxisLength<Axis>;

    fn sub(self, length: FlexAxisLength<Axis>) -> Self::Output {
        FlexAxisLength::new(self.0 - length.0)
    }
}

impl<Axis> Sub<FlexAxisSize<Axis>> for FlexAxisLength<Axis> {
    type Output = Self;

    fn sub(self, other: FlexAxisSize<Axis>) -> Self::Output {
        Self::new(self.0 - other.0)
    }
}

impl<Axis> Add<FlexAxisLength<Axis>> for FlexAxisSize<Axis> {
    type Output = FlexAxisLength<Axis>;

    fn add(self, other: FlexAxisLength<Axis>) -> Self::Output {
        FlexAxisLength::new(self.0 + other.0)
    }
}

impl<Axis> Add<FlexAxisSize<Axis>> for FlexAxisOffset<Axis> {
    type Output = Self;

    fn add(self, size: FlexAxisSize<Axis>) -> Self {
        Self::new(self.0 + size.0)
    }
}

impl<Axis> Add<FlexAxisLength<Axis>> for FlexAxisOffset<Axis> {
    type Output = Self;

    fn add(self, length: FlexAxisLength<Axis>) -> Self {
        Self::new(self.0 + length.0)
    }
}

impl<Axis> Sub<FlexAxisLength<Axis>> for FlexAxisOffset<Axis> {
    type Output = Self;

    fn sub(self, length: FlexAxisLength<Axis>) -> Self {
        Self::new(self.0 - length.0)
    }
}

impl<Axis> Sub<FlexAxisSize<Axis>> for FlexAxisOffset<Axis> {
    type Output = Self;

    fn sub(self, size: FlexAxisSize<Axis>) -> Self {
        Self::new(self.0 - size.0)
    }
}

impl<Axis> Sub for FlexAxisOffset<Axis> {
    type Output = FlexAxisLength<Axis>;

    fn sub(self, other: Self) -> Self::Output {
        FlexAxisLength::new(self.0 - other.0)
    }
}

pub(super) type FlexMainOffset = FlexAxisOffset<FlexMainAxis>;
pub(super) type FlexCrossOffset = FlexAxisOffset<FlexCrossAxis>;
pub(super) type FlexMainLength = FlexAxisLength<FlexMainAxis>;
pub(super) type FlexCrossLength = FlexAxisLength<FlexCrossAxis>;
pub(super) type FlexMainSize = FlexAxisSize<FlexMainAxis>;
pub(super) type FlexCrossSize = FlexAxisSize<FlexCrossAxis>;

/// Marker for a physical horizontal coordinate or extent in the flex
/// container's post-writing-mode coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct FlexPhysicalHorizontalAxis;

/// Marker for a physical vertical coordinate or extent in the flex
/// container's post-writing-mode coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct FlexPhysicalVerticalAxis;

/// A physical position inside the flex container after writing-mode
/// projection. This remains distinct from a Flex main/cross offset until a
/// caller explicitly selects the flex axis.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexPhysicalOffset<Axis>(f32, std::marker::PhantomData<Axis>);

impl<Axis> FlexPhysicalOffset<Axis> {
    pub(super) const fn new(points: f32) -> Self {
        Self(points, std::marker::PhantomData)
    }

    pub(super) const fn points(self) -> f32 {
        self.0
    }
}

/// A non-negative physical border-box extent inside the flex container after
/// writing-mode projection.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexPhysicalSize<Axis>(f32, std::marker::PhantomData<Axis>);

impl<Axis> FlexPhysicalSize<Axis> {
    pub(super) fn new(points: f32) -> Self {
        Self(points.max(0.0), std::marker::PhantomData)
    }

    pub(super) const fn points(self) -> f32 {
        self.0
    }
}

pub(super) type FlexPhysicalHorizontalOffset = FlexPhysicalOffset<FlexPhysicalHorizontalAxis>;
pub(super) type FlexPhysicalVerticalOffset = FlexPhysicalOffset<FlexPhysicalVerticalAxis>;
pub(super) type FlexPhysicalHorizontalSize = FlexPhysicalSize<FlexPhysicalHorizontalAxis>;
pub(super) type FlexPhysicalVerticalSize = FlexPhysicalSize<FlexPhysicalVerticalAxis>;

/// The resolved Taffy extents replayed into a flex item's nested formatting
/// context.
///
/// The replay interface needs the same Taffy result both as a frozen
/// border-box used size and as its legacy content-availability input. Keeping
/// those named conversions together prevents callers from extracting a raw
/// scalar and silently re-labeling it at each use site.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemReplayDimensions {
    width: FlexPhysicalHorizontalSize,
    height: FlexPhysicalVerticalSize,
}

impl FlexItemReplayDimensions {
    pub(super) fn border_box_width(self) -> BorderBoxLength {
        border_box_pt(self.width.points())
    }

    pub(super) fn border_box_height(self) -> BorderBoxLength {
        border_box_pt(self.height.points())
    }

    /// Adapt the frozen Taffy extent to the nested replay availability
    /// interface.
    ///
    /// The nested formatting context applies replayed box metrics itself, so
    /// this is not a general CSS border-to-content conversion.
    pub(super) fn available_width_for_replay(self) -> PhysicalContentWidth {
        PhysicalContentWidth::new(content_box_pt(self.width.points()))
    }

    /// See [`Self::available_width_for_replay`].
    pub(super) fn available_height_for_replay(self) -> PhysicalContentHeight {
        PhysicalContentHeight::new(content_box_pt(self.height.points()))
    }
}

/// Adapt a physical border-box height to a vertical item-baseline offset.
/// This is the physical-size to baseline-coordinate boundary used when CSS
/// Align synthesizes a baseline from a rectangle edge.
pub(in crate::layout::flex) fn flex_vertical_baseline_from_physical_height(
    height: FlexPhysicalVerticalSize,
) -> FlexVerticalBaselineOffset {
    flex_vertical_baseline_from_points(height.points())
}

/// Adapt a physical border-box width to a horizontal item-baseline offset.
pub(in crate::layout::flex) fn flex_horizontal_baseline_from_physical_width(
    width: FlexPhysicalHorizontalSize,
) -> FlexHorizontalBaselineOffset {
    flex_horizontal_baseline_from_points(width.points())
}

/// A non-negative CSS `flex-grow` factor after the flex used-value boundary.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexGrowFactor(f32);

impl FlexGrowFactor {
    pub(super) fn new(value: f32) -> Self {
        Self(value.max(0.0))
    }

    pub(super) const fn value(self) -> f32 {
        self.0
    }

    pub(super) fn resolve(self, base: FlexMainSize, fraction: FlexGrowFraction) -> FlexMainSize {
        FlexMainSize::new(base.points() + self.0 * fraction.0)
    }
}

/// A non-negative CSS `flex-shrink` factor after the flex used-value boundary.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexShrinkFactor(f32);

impl FlexShrinkFactor {
    pub(super) fn new(value: f32) -> Self {
        Self(value.max(0.0))
    }

    pub(super) const fn value(self) -> f32 {
        self.0
    }

    pub(super) fn resolve(self, base: FlexMainSize, fraction: FlexShrinkFraction) -> FlexMainSize {
        FlexMainSize::new(base.points() + self.0 * base.points() * fraction.0)
    }
}

/// The positive result of Flexbox's grow-fraction calculation.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexGrowFraction(f32);

impl FlexGrowFraction {
    pub(super) fn from_free_space(
        free_space: FlexMainSize,
        total_grow: FlexGrowFactor,
    ) -> Option<Self> {
        (total_grow.0 > 0.0).then(|| Self(free_space.points() / total_grow.0))
    }
}

/// Flexbox's scaled-shrink fraction. It is an algorithm coefficient, not a
/// layout length, and may be negative.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexShrinkFraction(f32);

/// The intrinsic Flex fraction selected by the max-content algorithm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum FlexIntrinsicFraction {
    None,
    Grow(FlexGrowFraction),
    Shrink(FlexShrinkFraction),
}

impl FlexIntrinsicFraction {
    pub(super) fn from_algorithm_value(value: f32) -> Self {
        if value > 0.0 {
            Self::Grow(FlexGrowFraction(value))
        } else if value < 0.0 {
            Self::Shrink(FlexShrinkFraction(value))
        } else {
            Self::None
        }
    }
}

/// Convert a resolved physical content-box length at the Flex main boundary.
pub(super) fn flex_main_size_from_content_box(length: ContentBoxLength) -> FlexMainSize {
    FlexMainSize::new(length.points())
}

/// Convert a resolved physical content-box length at the Flex cross boundary.
pub(super) fn flex_cross_size_from_content_box(length: ContentBoxLength) -> FlexCrossSize {
    FlexCrossSize::new(length.points())
}

/// Project a non-negative intrinsic gap into the Flex main axis.
pub(super) fn flex_main_gap_size(length: LayoutLength) -> FlexMainSize {
    FlexMainSize::new(length.points())
}

/// Convert a non-negative physical layout extent into the resolved Flex main
/// axis at a caller that has already selected that axis.
pub(super) fn flex_main_size_from_layout_extent(length: LayoutLength) -> FlexMainSize {
    FlexMainSize::new(length.points())
}

/// Project a non-negative intrinsic gap into the Flex cross axis.
pub(super) fn flex_cross_gap_size(length: LayoutLength) -> FlexCrossSize {
    FlexCrossSize::new(length.points())
}

/// Convert a non-negative physical layout extent into the resolved Flex cross
/// axis at a caller that has already selected that axis.
pub(super) fn flex_cross_size_from_layout_extent(length: LayoutLength) -> FlexCrossSize {
    FlexCrossSize::new(length.points())
}

/// Adapt a resolved Flex main extent for a legacy CSS content-box resolver.
pub(super) fn flex_main_content_box_length(size: FlexMainSize) -> ContentBoxLength {
    content_box_pt(size.points())
}

/// Adapt a resolved Flex cross extent for a legacy CSS content-box resolver.
pub(super) fn flex_cross_content_box_length(size: FlexCrossSize) -> ContentBoxLength {
    content_box_pt(size.points())
}

/// Transfer a resolved main-axis content size through a preferred aspect ratio.
///
/// The Flex direction is required so callers cannot re-label a size without
/// recording whether the main axis is physical horizontal or vertical.
pub(super) fn flex_cross_size_from_main_aspect_ratio(
    main: FlexMainSize,
    direction: FlexDirection,
    ratio: f32,
) -> FlexCrossSize {
    debug_assert!(ratio > 0.0);
    if direction.is_row_axis() {
        FlexCrossSize::new(main.points() / ratio)
    } else {
        FlexCrossSize::new(main.points() * ratio)
    }
}

/// An authored CSS `flex-direction`, before Writing Modes maps it to Taffy's
/// physical row/column representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SpecifiedFlexDirection(FlexDirection);

impl SpecifiedFlexDirection {
    pub(super) const fn new(value: FlexDirection) -> Self {
        Self(value)
    }
}

/// A flex direction expressed in Taffy's physical row/column coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PhysicalFlexDirection(FlexDirection);

impl PhysicalFlexDirection {
    pub(super) const fn new(value: FlexDirection) -> Self {
        Self(value)
    }

    pub(super) const fn taffy_direction(self) -> FlexDirection {
        self.0
    }

    pub(super) fn is_row_axis(self) -> bool {
        self.0.is_row_axis()
    }
}

/// Taffy's physical horizontal and vertical gap inputs.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PhysicalFlexGaps {
    pub(super) horizontal: css::ComputedGap,
    pub(super) vertical: css::ComputedGap,
}

pub(super) struct FlexLayout {
    /// Final physical content-box height of the flex container.
    pub(super) height: PhysicalContentHeight,
    /// Resolved gutter between adjacent items on a final flex line.
    ///
    /// The value has already resolved its percentage basis at the flex-layout
    /// boundary.  Consumers of final flex geometry must retain it when they
    /// derive an automatic main-axis span from line measurements:
    /// <https://www.w3.org/TR/css-align-3/#gaps>.
    pub(super) main_gap: FlexMainSize,
    /// First and last baselines exported by the flex container, relative to
    /// its physical content-box origin.
    ///
    /// A flex container can export either a vertical or horizontal baseline
    /// depending on its writing mode. Keep both physical axes until the
    /// parent formatting context selects the compatible one.
    pub(super) baselines: FlexContainerBaselineSets,
    pub(super) items: Vec<FlexItemLayout>,
    /// Flex line metadata recovered from the final Taffy layout.
    ///
    /// CSS Flexbox performs cross-axis alignment, baseline sharing, and
    /// fragmentation per flex line:
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-lines>.
    pub(super) lines: Vec<FlexLineLayout>,
    /// Paged-fragmentation metadata prepared from the flex line layout.
    ///
    /// CSS Flexbox fragments flex containers line-by-line and item-by-item in
    /// paged media:
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
    pub(super) fragment_plan: FlexFragmentPlan,
}

/// Flexbox-specific reason a final flex item size is definite.
///
/// CSS Flexbox adds definiteness rules on top of CSS Sizing so percentage
/// descendants can resolve against post-flexing item sizes:
/// <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FlexDefiniteSizeSource {
    PostFlexingMainSizeFromDefiniteContainer,
    PostFlexingMainSizeFromDefiniteFlexBase,
    SpecifiedMainSize,
    StretchedCrossSizeFromDefiniteSingleLineContainer,
    ResolvedLineCrossSize,
}

pub(super) type FlexPercentageBasis = PercentageBasis<ContentBoxLength, FlexDefiniteSizeSource>;

/// Provenance for a flex container available-space axis that is definite enough
/// to resolve percentages.
///
/// The raw available size can still constrain layout when it is not a valid
/// percentage basis. Keeping the percentage basis separate follows CSS Sizing's
/// definition of definiteness while preserving Flexbox's available-space
/// inputs:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FlexAvailableSizeSource {
    ContainingBlock,
    IntrinsicContainerSize,
    DefiniteCrossSize,
    DefiniteFlexBase,
    /// The flex algorithm has resolved this item's used main size. Descendant
    /// measurement uses it as a definite inline-size constraint while the
    /// item's automatic cross size is recomputed.
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
    PostFlexingMainSize,
    DefinitePreferredMainSize,
    DefinitePreferredCrossSize,
}

pub(super) type FlexAvailablePercentageBasis =
    PercentageBasis<ContentBoxLength, FlexAvailableSizeSource>;

pub(super) fn flex_available_percentage_basis(
    value: Option<ContentBoxLength>,
    source: FlexAvailableSizeSource,
) -> FlexAvailablePercentageBasis {
    value
        .map(|value| PercentageBasis::definite_from(value, source))
        .unwrap_or_else(PercentageBasis::indefinite)
}

/// Adapt a scalar legacy layout result into a flex percentage basis.
///
/// CSS used-value resolution and the Taffy adapter still exchange scalar
/// points.  Keep that conversion at the adapter boundary rather than making
/// physical Flex callers discard their content-box type prematurely.
pub(super) fn flex_available_percentage_basis_from_legacy_points(
    value: Option<f32>,
    source: FlexAvailableSizeSource,
) -> FlexAvailablePercentageBasis {
    flex_available_percentage_basis(value.map(content_box_pt), source)
}

/// Backwards-compatible name for scalar CSS/Taffy adapter call sites.
///
/// New Flex code should pass `ContentBoxLength` to
/// [`flex_available_percentage_basis`] instead.
pub(super) fn flex_available_percentage_basis_from_points(
    value: Option<f32>,
    source: FlexAvailableSizeSource,
) -> FlexAvailablePercentageBasis {
    flex_available_percentage_basis_from_legacy_points(value, source)
}

/// Layout metadata for one flex line in flex main/cross coordinates.
///
/// CSS Flexbox defines flex lines as the units used for cross-axis sizing,
/// alignment, and paged fragmentation. `main_start`/`main_end` and
/// `cross_start`/`cross_end` are measured in the container's flex-axis space,
/// not directly as physical x/y coordinates:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-lines> and
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FlexLineLayout {
    pub(super) item_indices: Vec<usize>,
    /// Position in the order-modified sequence of flex lines, measured from
    /// flex cross-start. This identity survives physical `wrap-reverse`
    /// placement and later `align-content` translations.
    pub(super) logical_cross_start_rank: usize,
    pub(super) source_start: usize,
    pub(super) source_end: usize,
    pub(super) main_start: FlexMainOffset,
    pub(super) main_end: FlexMainOffset,
    pub(super) cross_start: FlexCrossOffset,
    pub(super) cross_end: FlexCrossOffset,
    pub(super) first_baseline: Option<FlexCrossOffset>,
    pub(super) last_baseline: Option<FlexCrossOffset>,
    pub(super) collapsed_struts: Vec<FlexCollapsedStrut>,
}

/// Immutable membership of one flex line, collected before flexible lengths
/// and cross-axis reconciliation mutate item rectangles.
///
/// CSS Flexbox collects consecutive flex items in order-modified document
/// order using their outer hypothetical main sizes.  The record intentionally
/// contains no physical rectangle: later baseline, stretch, fragmentation,
/// and paint passes must not infer membership again from their corrected
/// geometry:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FlexLineTopology {
    pub(super) item_indices: Vec<usize>,
    pub(super) source_start: usize,
    pub(super) source_end: usize,
}

impl FlexLineLayout {
    pub(super) fn cross_size(&self) -> FlexCrossSize {
        self.cross_end
            .relative_to(self.cross_start)
            .non_negative_size()
    }

    pub(super) fn largest_collapsed_strut(&self) -> FlexCrossSize {
        self.collapsed_struts
            .iter()
            .map(|strut| strut.cross_size)
            .max_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or_else(|| FlexCrossSize::new(0.0))
    }
}

/// Cross-size strut left by a collapsed flex item in flex cross-axis units.
///
/// CSS Flexbox removes collapsed items from main-axis layout while preserving
/// a cross-size strut for line sizing:
/// <https://www.w3.org/TR/css-flexbox-1/#visibility-collapse>.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FlexCollapsedStrut {
    pub(super) item_index: usize,
    pub(super) cross_size: FlexCrossSize,
    pub(super) source_start: usize,
    pub(super) source_end: usize,
}

/// A physical block-axis offset within the unfragmented flex container.
///
/// This is deliberately not a flex cross-axis offset: wrapped column flex
/// containers can use a physical main axis for page fragmentation.
#[derive(Debug, Clone, Copy, Default, PartialEq, PartialOrd)]
pub(super) struct FlexFragmentBlockOffset(f32);

impl FlexFragmentBlockOffset {
    pub(super) const fn new(points: f32) -> Self {
        Self(points)
    }

    pub(super) const fn points(self) -> f32 {
        self.0
    }
}

/// A non-negative physical block-axis extent within a flex fragment.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexFragmentBlockSize(f32);

impl Default for FlexFragmentBlockSize {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl FlexFragmentBlockSize {
    pub(super) fn new(points: f32) -> Self {
        Self(points.max(0.0))
    }

    pub(super) const fn points(self) -> f32 {
        self.0
    }
}

/// A signed difference between two source-local flex fragment offsets.
///
/// Offsets are positions, so their difference cannot be silently converted to
/// a non-negative fragment extent. Callers that require an extent must choose
/// the explicit `non_negative_size` conversion.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(super) struct FlexFragmentBlockLength(f32);

impl FlexFragmentBlockLength {
    pub(super) const fn new(points: f32) -> Self {
        Self(points)
    }

    pub(super) const fn points(self) -> f32 {
        self.0
    }

    pub(super) fn abs(self) -> f32 {
        self.0.abs()
    }

    pub(super) fn non_negative_size(self) -> FlexFragmentBlockSize {
        FlexFragmentBlockSize::new(self.0)
    }
}

/// A source-local physical block range selected for one flex item or page
/// fragment. The endpoints are offsets, not sizes: keeping them together
/// avoids passing a fragment start as a block extent during pagination.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FlexFragmentBlockBounds {
    start: FlexFragmentBlockOffset,
    end: FlexFragmentBlockOffset,
}

impl FlexFragmentBlockBounds {
    pub(super) fn new(start: FlexFragmentBlockOffset, end: FlexFragmentBlockOffset) -> Self {
        debug_assert!(end >= start);
        Self { start, end }
    }

    pub(super) fn from_start_and_size(
        start: FlexFragmentBlockOffset,
        size: FlexFragmentBlockSize,
    ) -> Self {
        Self::new(start, start + size)
    }

    pub(super) const fn start(self) -> FlexFragmentBlockOffset {
        self.start
    }

    pub(super) const fn end(self) -> FlexFragmentBlockOffset {
        self.end
    }

    pub(super) fn size(self) -> FlexFragmentBlockSize {
        (self.end - self.start).non_negative_size()
    }
}

impl Add<FlexFragmentBlockSize> for FlexFragmentBlockOffset {
    type Output = Self;

    fn add(self, size: FlexFragmentBlockSize) -> Self {
        Self::new(self.0 + size.0)
    }
}

impl Sub for FlexFragmentBlockOffset {
    type Output = FlexFragmentBlockLength;

    fn sub(self, other: Self) -> Self::Output {
        FlexFragmentBlockLength::new(self.0 - other.0)
    }
}

/// Page-fragment planning metadata for a flex container in physical page flow.
///
/// This is the internal bridge from unfragmented flex line layout to the full
/// CSS Flexbox pagination algorithm:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination> and
/// <https://www.w3.org/TR/css-break-3/>.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct FlexFragmentPlan {
    /// Source-layout projection retained for line/gap consumers that have not
    /// crossed the materialized-fragment boundary yet.
    pub(super) fragments: Vec<FlexFragmentLayout>,
    /// Canonical committed source-to-fragmentainer mappings.  Replay and
    /// paint consumers migrate here so they do not reconstruct destination
    /// geometry from source layout.
    pub(super) materialized_fragments: Vec<MaterializedFlexFragment>,
}

impl FlexFragmentPlan {
    /// Prepare a fragment while the flex break units are consumed.
    ///
    /// The initial Taffy result is source geometry, not a fragment plan.
    /// Item-fragment start kinds follow materialized fragmentainers, including
    /// a partial first fragmentainer and forced transitions.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(super) fn prepare_materialized_fragment(&self, fragment: &mut FlexFragmentLayout) {
        for item in &mut fragment.items {
            let preceding_slice_count = self
                .fragments
                .iter()
                .flat_map(|previous| &previous.items)
                .filter(|previous| {
                    previous.source_item_index == item.source_item_index
                        && previous.content_slice.block_end.points()
                            <= item.content_slice.block_start.points() + 0.01
                })
                .count();
            item.continuation.fragment_start =
                FlexItemFragmentStart::classify(preceding_slice_count, item.content_slice);
            debug_assert!(
                item.continuation
                    .fragment_start
                    .is_consistent_with(preceding_slice_count, item.content_slice,)
            );
            item.continuation.source_content_slice = item.content_slice;
            item.continuation.decoration_slice = item.decoration_slice;
            item.continuation.fragmentainer_index = fragment.page_index;
        }
    }

    /// Append a fragment after its paint and page-side effects are committed.
    pub(super) fn push_materialized_fragment(&mut self, fragment: MaterializedFlexFragment) {
        debug_assert!(
            fragment.source_bounds.end().points() >= fragment.source_bounds.start().points(),
            "a materialized flex fragment has a monotonic source range",
        );
        for item in &fragment.item_fragments {
            debug_assert!(
                item.continuation.source_content_slice.block_end.points()
                    >= item.continuation.source_content_slice.block_start.points(),
                "a materialized flex item continuation has a monotonic source range",
            );
            debug_assert!(
                fragment.layout.items.iter().any(|planned| {
                    planned.item_index == item.item_index
                        && planned.source_item_index == item.source_item_index
                        && planned.line_index == item.line_index
                        && planned.source_bounds == item.source_bounds
                        && planned.bounds == item.visible_bounds
                        && planned.content_slice == item.content_slice
                        && planned.decoration_slice == item.decoration_slice
                        && planned.continuation == item.continuation
                }),
                "each materialized item slice must retain exactly one planned source intersection",
            );
            debug_assert_eq!(
                fragment
                    .layout
                    .line_fragments
                    .iter()
                    .filter(|line| {
                        line.line_index == item.line_index
                            && line.item_indices.contains(&item.item_index)
                    })
                    .count(),
                1,
                "each materialized flex item slice must belong to exactly one retained line slice",
            );
        }
        debug_assert!(
            fragment.layout.items.iter().all(|planned| {
                fragment
                    .item_fragments
                    .iter()
                    .filter(|materialized| {
                        materialized.item_index == planned.item_index
                            && materialized.source_item_index == planned.source_item_index
                            && materialized.line_index == planned.line_index
                            && materialized.source_bounds == planned.source_bounds
                            && materialized.visible_bounds == planned.bounds
                            && materialized.content_slice == planned.content_slice
                            && materialized.decoration_slice == planned.decoration_slice
                            && materialized.continuation == planned.continuation
                    })
                    .count()
                    == 1
            }),
            "every planned flex item slice must materialize exactly once",
        );
        self.fragments.push(fragment.layout.clone());
        self.materialized_fragments.push(fragment);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }

    pub(super) fn planned_item_fragment_count(&self) -> usize {
        self.fragments
            .iter()
            .map(|fragment| {
                let _page_index = fragment.page_index;
                let _line_span = fragment.line_end.saturating_sub(fragment.line_start);
                let _block_span =
                    FlexFragmentBlockBounds::new(fragment.block_start, fragment.block_end).size();
                let _fragment_metadata = &fragment.metadata;
                fragment
                    .items
                    .iter()
                    .map(|item| {
                        let _item_index = item.item_index;
                        let _source_item_index = item.source_item_index;
                        let _bounds = &item.bounds;
                        let _content_slice = item.content_slice;
                        let _decoration_slice = item.decoration_slice;
                        let _continuation = item.continuation;
                        let _item_metadata = &item.metadata;
                        1
                    })
                    .sum::<usize>()
            })
            .sum()
    }
}

/// Decoration ownership for one committed flex container fragment.
///
/// Fragment start/end decorations belong to the container fragment, not to an
/// adjacent flex item. Keeping that distinction at commit time prevents a
/// trailing border or padding edge from changing item source ranges.
/// <https://www.w3.org/TR/css-break-3/#box-splitting>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) struct FlexFragmentDecorationState {
    pub(in crate::layout::flex) includes_block_start: bool,
    pub(in crate::layout::flex) includes_block_end: bool,
}

/// Page-local paint geometry for one item slice in a committed flex fragment.
///
/// This record owns the immutable source intersection as well as the local
/// border box and continuation state actually consumed by item replay. That
/// keeps source geometry separate from destination geometry without requiring
/// replay to reach back into the provisional plan.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout::flex) struct MaterializedFlexItemFragment {
    pub(in crate::layout::flex) item_index: usize,
    pub(in crate::layout::flex) source_item_index: usize,
    /// Source flex line that owns this item slice in the committed fragment.
    pub(in crate::layout::flex) line_index: usize,
    /// Complete source range, including fragmentable descendant overflow.
    pub(in crate::layout::flex) source_bounds: FlexItemLayout,
    /// Physical portion of `source_bounds` visible in this fragmentainer.
    pub(in crate::layout::flex) visible_bounds: FlexItemLayout,
    /// Source-content interval represented by this destination record.
    pub(in crate::layout::flex) content_slice: FlexFragmentSlice,
    /// Decoration interval, independently clamped to the used border box.
    pub(in crate::layout::flex) decoration_slice: FlexFragmentSlice,
    /// Frozen used geometry consumed by replay in this destination fragment.
    pub(in crate::layout::flex) replay_bounds: FlexItemLayout,
    pub(in crate::layout::flex) local_border_box: PaintClip,
    pub(in crate::layout::flex) continuation: FlexItemContinuation,
    pub(in crate::layout::flex) local_to_page_translation: PaintTranslation,
}

impl MaterializedFlexItemFragment {
    /// Materialize one already-planned item intersection in destination page
    /// coordinates. Source slices are copied from the plan instead of being
    /// inferred from the local paint rectangle.
    pub(in crate::layout::flex) fn from_planned(
        planned: &FlexItemFragmentLayout,
        local_border_box: PaintClip,
        local_to_page_translation: PaintTranslation,
    ) -> Self {
        Self {
            item_index: planned.item_index,
            source_item_index: planned.source_item_index,
            line_index: planned.line_index,
            source_bounds: planned.source_bounds.clone(),
            visible_bounds: planned.bounds.clone(),
            content_slice: planned.content_slice,
            decoration_slice: planned.decoration_slice,
            replay_bounds: planned.used_bounds.clone(),
            local_border_box,
            continuation: planned.continuation,
            local_to_page_translation,
        }
    }
}

/// Canonical committed mapping from one flex source slice to one destination
/// fragmentainer.
///
/// `layout` owns source line/item intersections; this record owns the final
/// destination geometry, decoration state, and all item-local replay boxes.
/// It is constructed only after the fragmentainer transition is materialized.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout::flex) struct MaterializedFlexFragment {
    pub(in crate::layout::flex) layout: FlexFragmentLayout,
    pub(in crate::layout::flex) source_bounds: FlexFragmentBlockBounds,
    pub(in crate::layout::flex) destination_border_box: Option<PaintClip>,
    /// Padding-box overflow clip resolved for this committed destination
    /// fragmentainer. It is assigned after the container's final used height
    /// is known, without altering the source range or decoration ownership.
    pub(in crate::layout::flex) contents_overflow_clip: Option<AxisSelectivePaintClip>,
    pub(in crate::layout::flex) decoration: FlexFragmentDecorationState,
    pub(in crate::layout::flex) local_to_page_translation: PaintTranslation,
    pub(in crate::layout::flex) item_fragments: Vec<MaterializedFlexItemFragment>,
}

impl MaterializedFlexFragment {
    pub(in crate::layout::flex) fn new(
        layout: FlexFragmentLayout,
        destination_border_box: Option<PaintClip>,
        decoration: FlexFragmentDecorationState,
        local_to_page_translation: PaintTranslation,
    ) -> Self {
        let source_bounds = FlexFragmentBlockBounds::new(layout.block_start, layout.block_end);
        debug_assert!(source_bounds.end().points() >= source_bounds.start().points());
        Self {
            layout,
            source_bounds,
            destination_border_box,
            contents_overflow_clip: None,
            decoration,
            local_to_page_translation,
            item_fragments: Vec::new(),
        }
    }
}

impl Deref for MaterializedFlexFragment {
    type Target = FlexFragmentLayout;

    fn deref(&self) -> &Self::Target {
        &self.layout
    }
}

impl DerefMut for MaterializedFlexFragment {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.layout
    }
}

/// One flex container fragment in paged layout.
///
/// CSS Flexbox fragmentation slices a flex container into page-local fragments
/// while preserving item geometry and fragment metadata:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FlexFragmentLayout {
    pub(super) page_index: usize,
    pub(super) line_start: usize,
    pub(super) line_end: usize,
    pub(super) block_start: FlexFragmentBlockOffset,
    pub(super) block_end: FlexFragmentBlockOffset,
    /// Explicit source intersections for every flex line represented by this
    /// fragment. A line-index span alone loses the local main-axis ranges of
    /// overlapping wrapped column lines.
    pub(super) line_fragments: Vec<FlexLineFragmentLayout>,
    pub(super) items: Vec<FlexItemFragmentLayout>,
    pub(super) metadata: FragmentPageMetadata,
}

/// One flex-line source slice owned by a committed container fragment.
///
/// Wrapped physical-column lines may overlap in the fragmentainer block axis,
/// so replay needs each line's selected source range instead of inferring it
/// from the enclosing fragment's union range.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FlexLineFragmentLayout {
    pub(super) line_index: usize,
    pub(super) source_bounds: FlexFragmentBlockBounds,
    pub(super) item_indices: Vec<usize>,
}

/// Page-local geometry for one flex item fragment in container coordinates.
///
/// CSS Fragmentation requires each visible piece to own its page-local paint,
/// link, assignment, and effect metadata:
/// <https://www.w3.org/TR/css-break-3/#box-splitting>.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FlexItemFragmentLayout {
    pub(super) item_index: usize,
    pub(super) source_item_index: usize,
    pub(super) line_index: usize,
    /// Source border box extended through fragmentable descendant overflow.
    ///
    /// This rectangle defines only the source interval that intersects a
    /// fragmentainer. It must not be used to re-layout the item, because CSS
    /// Flexbox retains the item's independently resolved used border box.
    pub(super) source_bounds: FlexItemLayout,
    /// The item's resolved used border box, frozen before source-overflow
    /// projection. Fragment replay uses this geometry for its percentage
    /// bases and nested formatting-context constraints.
    pub(super) used_bounds: FlexItemLayout,
    pub(super) bounds: FlexItemLayout,
    pub(super) content_slice: FlexFragmentSlice,
    pub(super) decoration_slice: FlexFragmentSlice,
    /// Replay input derived from the committed fragmentainer sequence.
    pub(super) continuation: FlexItemContinuation,
    pub(super) metadata: FragmentPageMetadata,
}

impl FlexItemFragmentLayout {
    /// Source-container block start of the interval selected for this item.
    ///
    /// The item position and its source-content slice live in different
    /// coordinate systems: the former is container-local and the latter is
    /// border-box-local. Fragment materialization must combine them before
    /// projecting the interval into a destination fragmentainer.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(super) fn selected_source_block_start(&self) -> FlexFragmentBlockOffset {
        FlexFragmentBlockOffset::new(
            self.source_bounds.y().points() + self.content_slice.block_start.points(),
        )
    }
}

/// Source and fragmentainer state for one flex item continuation.
///
/// Content and decoration slices are both retained because
/// `box-decoration-break` may choose a different decoration range without
/// changing descendant source flow.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
/// <https://www.w3.org/TR/css-break-3/#box-splitting>
///
/// `fragment_start` distinguishes a true fragment continuation from source
/// coordinates that begin after zero only because paint overflow preceded the
/// item's used border box. Keeping this classification with the committed
/// continuation prevents replay from mistaking a negative cross-start margin
/// for a prior fragment.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct FlexItemContinuation {
    pub(super) source_content_slice: FlexFragmentSlice,
    /// Block-start of this source canvas interval in the item's frozen
    /// formatting-context coordinate system. This is intentionally separate
    /// from [`Self::source_content_slice`], whose offsets are border-box-local
    /// for clipping and continuation-end accounting.
    pub(super) source_canvas_block_start: FlexFragmentBlockOffset,
    pub(super) decoration_slice: FlexFragmentSlice,
    /// Coordinate system selected when replaying this committed source
    /// interval. This belongs to the continuation itself so painting never
    /// re-infers it from the flex direction or wrapping mode.
    pub(super) replay_origin: FlexItemReplayOrigin,
    /// Remaining capacity in the first local fragmentainer for this item.
    pub(super) first_fragmentainer_capacity: FlexFragmentBlockSize,
    /// Capacity available to each later local fragmentainer.
    pub(super) continuation_fragmentainer_capacity: FlexFragmentBlockSize,
    pub(super) fragmentainer_index: usize,
    pub(super) fragment_start: FlexItemFragmentStart,
    /// Ordinal of a fragment committed by the item's own formatting context,
    /// when that child forced a page transition without splitting a flex break
    /// unit. The outer flex record retains this instead of recovering a child
    /// continuation from flex direction or source-height arithmetic.
    pub(super) child_fragment_ordinal: Option<usize>,
}

impl FlexItemContinuation {
    /// Whether this item record has a committed source predecessor.
    pub(super) fn continues_from_previous_fragment(self) -> bool {
        self.fragment_start.is_continuation()
    }

    /// Select the child-local fragment cached for this item record.
    ///
    /// A source item starts with child fragment zero even when its source slice
    /// is offset by leading paint overflow. Only a committed predecessor moves
    /// child replay to a later fragment.
    pub(super) fn child_fragment_replay_ordinal(self) -> usize {
        self.fragment_start.child_fragment_replay_ordinal()
    }

    /// Commit the source-canvas origin for a replayed flex-item slice.
    ///
    /// `source_content_slice` is measured from the frozen used border-box
    /// origin. A source-slice replay lays out one original formatting-context
    /// canvas, so its destination clip must translate that canvas by the
    /// slice's block start rather than restart it at zero on every page. The
    /// principal box remains independently frozen in `used_bounds`; this
    /// mapping neither changes its definite used size nor creates a child
    /// break opportunity.
    ///
    /// CSS size containment lays out contents normally after sizing the box
    /// as empty, and makes the box monolithic:
    /// <https://www.w3.org/TR/css-contain-2/#size-containment>
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>
    pub(super) fn materialize_source_canvas_block_start(
        &mut self,
        frozen_used_bounds: &FlexItemLayout,
    ) {
        debug_assert!(
            frozen_used_bounds.height().points() >= 0.0,
            "a frozen flex item border box has a non-negative block size",
        );
        if self.replay_origin == FlexItemReplayOrigin::SourceSlice {
            self.source_canvas_block_start = self.source_content_slice.block_start;
        }
    }
}

/// Classification of an item's start within the committed flex-fragment
/// sequence.
///
/// A positive local source-slice start alone does not establish a CSS
/// fragmentation continuation: a negative cross-start margin may paint before
/// the line's source interval without a preceding item fragment. The committed
/// plan therefore records that case separately from a true predecessor.
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum FlexItemFragmentStart {
    /// The first committed source slice begins at the item's source origin.
    #[default]
    ItemStart,
    /// The first committed slice begins after zero because leading paint
    /// overflow (such as a negative cross-start margin) lies before it.
    LeadingPaintOverflow,
    /// A prior committed slice of the same source item owns the earlier
    /// source interval.
    Continuation { ordinal: NonZeroUsize },
}

impl FlexItemFragmentStart {
    const SOURCE_SLICE_EPSILON: f32 = 0.01;

    fn classify(preceding_slice_count: usize, content_slice: FlexFragmentSlice) -> Self {
        if let Some(ordinal) = NonZeroUsize::new(preceding_slice_count) {
            Self::Continuation { ordinal }
        } else if content_slice.block_start.points() > Self::SOURCE_SLICE_EPSILON {
            Self::LeadingPaintOverflow
        } else {
            Self::ItemStart
        }
    }

    pub(super) fn is_continuation(self) -> bool {
        matches!(self, Self::Continuation { .. })
    }

    fn child_fragment_replay_ordinal(self) -> usize {
        match self {
            Self::ItemStart | Self::LeadingPaintOverflow => 0,
            Self::Continuation { ordinal } => ordinal.get(),
        }
    }

    fn is_consistent_with(
        self,
        preceding_slice_count: usize,
        content_slice: FlexFragmentSlice,
    ) -> bool {
        match self {
            Self::ItemStart => {
                preceding_slice_count == 0
                    && content_slice.block_start.points() <= Self::SOURCE_SLICE_EPSILON
            }
            Self::LeadingPaintOverflow => {
                preceding_slice_count == 0
                    && content_slice.block_start.points() > Self::SOURCE_SLICE_EPSILON
            }
            Self::Continuation { ordinal } => ordinal.get() == preceding_slice_count,
        }
    }
}

/// The child replay strategy owned by one materialized flex-item continuation.
///
/// Descendant overflow is a slice of one frozen child source canvas. A child
/// that creates its own fragmentainers instead supplies page-local fragments
/// selected by the continuation ordinal.
/// <https://www.w3.org/TR/css-break-3/#box-splitting>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum FlexItemReplayOrigin {
    #[default]
    ChildFragment,
    SourceSlice,
}

/// Block-axis slice of a flex fragment relative to the source border box.
///
/// CSS Fragmentation splits box content and cloned decorations into
/// fragment-local slices:
/// <https://www.w3.org/TR/css-break-3/#box-splitting>.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct FlexFragmentSlice {
    pub(super) block_start: FlexFragmentBlockOffset,
    pub(super) block_end: FlexFragmentBlockOffset,
}

/// CSS flex axes mapped into Quire's physical container coordinate system.
///
/// CSS Flexbox defines `row` as the inline axis and `column` as the block axis,
/// then CSS Writing Modes maps those axes to physical directions. Taffy only
/// accepts physical row/column flex directions plus a text direction switch, so
/// this value records the single mapping used at that adapter boundary:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FlexAxes {
    pub(super) flow: FlowAxes,
    pub(super) specified_direction: SpecifiedFlexDirection,
    pub(super) physical_direction: PhysicalFlexDirection,
}

impl FlexAxes {
    pub(super) fn for_style(style: &ComputedStyle) -> Self {
        Self {
            flow: FlowAxes::for_style(style),
            specified_direction: SpecifiedFlexDirection::new(style.flex_direction),
            physical_direction: PhysicalFlexDirection::new(physical_flex_direction(style)),
        }
    }

    pub(super) fn from_physical_direction(physical_direction: PhysicalFlexDirection) -> Self {
        Self {
            flow: FlowAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
            specified_direction: SpecifiedFlexDirection::new(physical_direction.taffy_direction()),
            physical_direction,
        }
    }

    pub(super) fn is_main_row_axis(self) -> bool {
        self.physical_direction.is_row_axis()
    }
}

/// Maps CSS flex main/cross axes into Quire's physical layout axes.
///
/// CSS Flexbox defines `row` from the inline axis and `column` from the block
/// axis, while Taffy lays out rows on physical X and columns on physical Y.
/// CSS Writing Modes maps those logical axes to physical axes:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(super) fn physical_flex_direction(style: &ComputedStyle) -> FlexDirection {
    // Taffy carries horizontal `direction` separately from `flex-direction`.
    // Keep an ordinary horizontal row's authored direction here so that
    // `row-reverse` is combined with RTL exactly once by Taffy. Vertical
    // writing has no equivalent logical-row switch in Taffy, so it is
    // projected to a fully physical direction below.
    if style.writing_mode == WritingMode::HorizontalTb {
        return style.flex_direction;
    }
    let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
    let logical_axis = if style.flex_direction.is_row_axis() {
        LogicalAxis::Inline
    } else {
        LogicalAxis::Block
    };
    let authored_reverse = matches!(
        style.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let reversed = axes.is_reversed(logical_axis) != authored_reverse;
    match (axes.physical_axis(logical_axis), reversed) {
        (PhysicalAxis::Horizontal, false) => FlexDirection::Row,
        (PhysicalAxis::Horizontal, true) => FlexDirection::RowReverse,
        (PhysicalAxis::Vertical, false) => FlexDirection::Column,
        (PhysicalAxis::Vertical, true) => FlexDirection::ColumnReverse,
    }
}

/// Returns physical row/column gaps for a flex container.
///
/// CSS Box Alignment maps `row-gap` to the block axis and `column-gap` to the
/// inline axis. Taffy expects physical X/Y gap values, so vertical writing
/// modes swap the physical row and column gap inputs:
/// <https://www.w3.org/TR/css-align-3/#gaps> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(super) fn physical_flex_gaps(style: &ComputedStyle) -> PhysicalFlexGaps {
    let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
    let (horizontal, vertical) =
        axes.physical_size(style.column_gap.clone(), style.row_gap.clone());
    PhysicalFlexGaps {
        horizontal,
        vertical,
    }
}

/// Returns whether a flex item is collapsed by `visibility: collapse`.
///
/// CSS Flexbox treats collapsed flex items as removed from flex layout while
/// leaving a cross-size strut behind:
/// <https://www.w3.org/TR/css-flexbox-1/#visibility-collapse>.
pub(super) fn flex_item_is_collapsed(style: &ComputedStyle) -> bool {
    style.visibility == Visibility::Collapse
}

/// Available physical container space passed to flex layout.
///
/// `width` and `height` are physical content-box dimensions, not logical
/// inline/block dimensions. Callers that need CSS percentage bases must map
/// through the item's writing mode before using these values:
/// <https://www.w3.org/TR/css-sizing-3/#definite> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexAvailableSpace {
    /// A physical content-box width, not a logical inline size.
    pub(super) width: PhysicalContentWidth,
    pub(super) width_basis: FlexAvailablePercentageBasis,
    /// A physical content-box height constraint, not a logical block size or
    /// a percentage basis.
    ///
    /// This can constrain the Flexbox algorithm even where it is not a
    /// definite CSS percentage basis. For example, an orthogonal block's
    /// automatic logical inline fill has a used physical height that must
    /// pack flex lines, while percentage descendants remain indefinite.
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
    /// <https://www.w3.org/TR/css-sizing-3/#definite>
    pub(super) height: Option<PhysicalContentHeight>,
    /// Definite physical-height percentage basis, kept independent from the
    /// numeric layout constraint above.
    pub(super) height_basis: FlexAvailablePercentageBasis,
}

impl FlexAvailableSpace {
    /// Definite physical width percentage basis in content-box space.
    pub(super) fn width_basis_content_box_length(self) -> Option<ContentBoxLength> {
        self.width_basis.value()
    }

    /// Definite physical height percentage basis in content-box space.
    pub(super) fn height_basis_content_box_length(self) -> Option<ContentBoxLength> {
        self.height_basis.value()
    }

    /// Physical cross-axis constraint supplied to Flexbox line layout.
    ///
    /// Unlike [`Self::height_basis`], this answers only whether the layout
    /// algorithm has a numeric height to use for sizing and line packing.
    pub(super) fn height_constraint(self) -> Option<PhysicalContentHeight> {
        self.height
    }

    pub(super) fn main_basis(self, direction: FlexDirection) -> FlexAvailablePercentageBasis {
        if direction.is_row_axis() {
            self.width_basis
        } else {
            self.height_basis
        }
    }

    pub(super) fn cross_basis(self, direction: FlexDirection) -> FlexAvailablePercentageBasis {
        if direction.is_row_axis() {
            self.height_basis
        } else {
            self.width_basis
        }
    }

    /// Project Flex's physical available-space record onto the container's
    /// logical inline axis for CSS Box percentage edges.
    pub(super) fn logical_inline_basis(
        self,
        style: &ComputedStyle,
    ) -> LogicalInlinePercentageBasis<FlexAvailableSizeSource> {
        let physical_basis = if WritingModeAxes::new(style.writing_mode, style.used_direction())
            .swaps_physical_axes()
        {
            self.height_basis
        } else {
            self.width_basis
        };
        physical_basis.map_value(LogicalInlineContentSize::new)
    }

    /// Return the definite physical content-box extent on the resolved main
    /// axis. A column Flex container has no main size when its physical height
    /// is indefinite.
    pub(super) fn definite_main_size(self, direction: FlexDirection) -> Option<FlexMainSize> {
        if direction.is_row_axis() {
            self.width_basis
                .value()
                .map(flex_main_size_from_content_box)
        } else {
            self.height_basis
                .value()
                .map(flex_main_size_from_content_box)
        }
    }

    /// Return the definite physical content-box extent on the resolved cross
    /// axis, if that physical axis has a definite available size.
    pub(super) fn definite_cross_size(self, direction: FlexDirection) -> Option<FlexCrossSize> {
        if direction.is_row_axis() {
            self.height_basis
                .value()
                .map(flex_cross_size_from_content_box)
        } else {
            self.width_basis
                .value()
                .map(flex_cross_size_from_content_box)
        }
    }

    /// Install a definite Flex cross size at the physical content-box
    /// boundary, retaining the source that makes percentage resolution legal.
    pub(super) fn with_definite_cross_size(
        mut self,
        direction: FlexDirection,
        cross_size: FlexCrossSize,
    ) -> Self {
        if direction.is_row_axis() {
            self.set_definite_height(
                PhysicalContentHeight::new(content_box_pt(cross_size.points())),
                FlexAvailableSizeSource::DefiniteCrossSize,
            );
        } else {
            self.set_definite_width(
                PhysicalContentWidth::new(content_box_pt(cross_size.points())),
                FlexAvailableSizeSource::DefiniteCrossSize,
            );
        }
        self
    }

    pub(super) fn set_definite_width(
        &mut self,
        width: PhysicalContentWidth,
        source: FlexAvailableSizeSource,
    ) {
        self.width = width;
        self.width_basis = PercentageBasis::definite_from(width.content_box_length(), source);
    }

    pub(super) fn set_definite_height(
        &mut self,
        height: PhysicalContentHeight,
        source: FlexAvailableSizeSource,
    ) {
        self.height = Some(height);
        self.height_basis = PercentageBasis::definite_from(height.content_box_length(), source);
    }
}

/// Returns the cross-axis space available while measuring a balanced flex item.
///
/// `flex-line-count` reserves the cross-axis gaps for that exact number of
/// balanced lines and divides the remainder between them. This affects the
/// available space used to measure each item, but it does not replace the
/// container percentage basis: percentages on the item itself continue to
/// resolve against the flex container's corresponding content box:
/// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>.
pub(in crate::layout::flex) fn balanced_flex_item_measure_available_space(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> FlexAvailableSpace {
    let css::FlexLineCount::Count(line_count) = style.flex_line_count else {
        return available;
    };
    if !style.flex_wrap.balances_lines() {
        return available;
    }
    let line_count = line_count.get();
    if line_count <= 1 {
        return available;
    }

    let PhysicalFlexGaps {
        horizontal,
        vertical,
    } = physical_flex_gaps(style);
    let (cross_size, cross_basis, cross_gap) = if physical_direction.is_row_axis() {
        let Some(height) = available.height else {
            return available;
        };
        (
            FlexCrossSize::new(height.points()),
            available.height_basis,
            vertical,
        )
    } else {
        (
            FlexCrossSize::new(available.width.points()),
            available.width_basis,
            horizontal,
        )
    };
    let cross_gap = if cross_basis.is_definite() {
        flex_cross_gap_size(used_flex_gap_with_basis(cross_gap, cross_basis))
    } else {
        match cross_gap {
            css::ComputedGap::Normal => FlexCrossSize::new(0.0),
            css::ComputedGap::LengthPercentage(value) => {
                flex_cross_gap_size(value.length_max_zero())
            }
        }
    };
    let gap_total = cross_gap.scale(line_count.saturating_sub(1) as f32);
    let item_cross_size = (cross_size - gap_total)
        .non_negative_size()
        .divide(std::num::NonZeroUsize::new(line_count).expect("line count is non-zero"));

    let mut item_available = available;
    if physical_direction.is_row_axis() {
        item_available.height = Some(PhysicalContentHeight::new(content_box_pt(
            item_cross_size.points(),
        )));
    } else {
        item_available.width = PhysicalContentWidth::new(content_box_pt(item_cross_size.points()));
    }
    item_available
}

/// Available physical container space used while estimating one flex item.
///
/// This is still physical width/height. The `inline_size` and `inline_basis`
/// helpers perform the CSS Writing Modes projection needed by percentage
/// resolution in descendants:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemAvailableSpace {
    pub(super) width: PhysicalContentWidth,
    pub(super) width_basis: FlexAvailablePercentageBasis,
    pub(super) height: Option<PhysicalContentHeight>,
    pub(super) height_basis: FlexAvailablePercentageBasis,
    pub(super) stretched_width: Option<PhysicalContentWidth>,
    pub(super) stretched_height: Option<PhysicalContentHeight>,
}

impl FlexItemAvailableSpace {
    pub(super) fn from_container(available: FlexAvailableSpace) -> Self {
        Self {
            width: available.width,
            width_basis: available.width_basis,
            height: available.height,
            height_basis: available.height_basis,
            stretched_width: None,
            stretched_height: None,
        }
    }

    pub(super) fn set_definite_width(
        &mut self,
        width: PhysicalContentWidth,
        source: FlexAvailableSizeSource,
    ) {
        self.width = width;
        self.width_basis = PercentageBasis::definite_from(width.content_box_length(), source);
    }

    pub(super) fn set_definite_height(
        &mut self,
        height: PhysicalContentHeight,
        source: FlexAvailableSizeSource,
    ) {
        self.height = Some(height);
        self.height_basis = PercentageBasis::definite_from(height.content_box_length(), source);
    }

    /// Project a resolved Flex cross size back into the physical content-box
    /// axis used by descendant measurement. This is the only Flex-axis to
    /// physical-content conversion for item-local available space.
    pub(super) fn set_definite_cross_size(
        &mut self,
        direction: FlexDirection,
        cross_size: FlexCrossSize,
        source: FlexAvailableSizeSource,
    ) {
        if direction.is_row_axis() {
            self.set_definite_height(
                PhysicalContentHeight::new(flex_cross_content_box_length(cross_size)),
                source,
            );
        } else {
            self.set_definite_width(
                PhysicalContentWidth::new(flex_cross_content_box_length(cross_size)),
                source,
            );
        }
    }

    /// Record the same cross-size conversion for the stretch-specific
    /// measurement path without conflating it with an authored physical size.
    pub(super) fn set_stretched_cross_size(
        &mut self,
        direction: FlexDirection,
        cross_size: FlexCrossSize,
    ) {
        if direction.is_row_axis() {
            self.stretched_height = Some(PhysicalContentHeight::new(
                flex_cross_content_box_length(cross_size),
            ));
        } else {
            self.stretched_width = Some(PhysicalContentWidth::new(flex_cross_content_box_length(
                cross_size,
            )));
        }
    }

    /// Returns the item's containing-block inline-size basis for percentage
    /// resolution during intrinsic flex item measurement.
    ///
    /// CSS Writing Modes maps logical inline size to physical height in
    /// vertical writing modes. Flexbox requires a stretched flex item's
    /// definite cross size to be used when laying out descendants for flex base
    /// sizing:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
    /// <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
    pub(super) fn inline_size(self, style: &ComputedStyle) -> LogicalInlineContentSize {
        let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
        let value = if axes.swaps_physical_axes() {
            self.height
                .map(PhysicalContentHeight::content_box_length)
                .unwrap_or_else(|| self.width.content_box_length())
        } else {
            self.width.content_box_length()
        };
        LogicalInlineContentSize::new(value)
    }

    pub(super) fn inline_basis(self, style: &ComputedStyle) -> FlexAvailablePercentageBasis {
        if WritingModeAxes::new(style.writing_mode, style.used_direction()).swaps_physical_axes() {
            self.height_basis
        } else {
            self.width_basis
        }
    }
}

/// Flex aliases for the shared physical baseline coordinates. Flex keeps the
/// names local to make the physical-axis intent of its adapters obvious.
pub(in crate::layout::flex) type FlexVerticalBaselineAxis = PhysicalTopBaselineAxis;
pub(in crate::layout::flex) type FlexHorizontalBaselineAxis = PhysicalLeftBaselineAxis;
pub(in crate::layout::flex) type FlexVerticalBaselineOffset = PhysicalTopBaselineOffset;
pub(in crate::layout::flex) type FlexHorizontalBaselineOffset = PhysicalLeftBaselineOffset;

/// Form a physical vertical baseline coordinate from a final flex item's
/// border-box origin.
///
/// [`FlexItemLayout::y`] is the final border-box origin after Flexbox has
/// resolved margin-box placement. The measured baseline is likewise relative
/// to that border box, so an item margin must not be added again here.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
pub(in crate::layout::flex) fn flex_item_vertical_border_box_baseline_coordinate(
    item: &FlexItemLayout,
    baseline: FlexVerticalBaselineOffset,
) -> FlexVerticalBaselineOffset {
    flex_vertical_baseline_from_points(item.y().points() + baseline.points())
}

/// Form a physical horizontal baseline coordinate from a final flex item's
/// border-box origin.
///
/// [`FlexItemLayout::x`] is the final border-box origin after Flexbox has
/// resolved margin-box placement. The measured baseline is likewise relative
/// to that border box, so an item margin must not be added again here.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
pub(in crate::layout::flex) fn flex_item_horizontal_border_box_baseline_coordinate(
    item: &FlexItemLayout,
    baseline: FlexHorizontalBaselineOffset,
) -> FlexHorizontalBaselineOffset {
    flex_horizontal_baseline_from_points(item.x().points() + baseline.points())
}

/// Construct a typed baseline from a scalar only at an existing legacy text,
/// Taffy, or test boundary.
pub(in crate::layout::flex) fn flex_vertical_baseline_from_points(
    points: f32,
) -> FlexVerticalBaselineOffset {
    FlexVerticalBaselineOffset::new(layout_pt(points))
}

/// Construct a typed baseline from a scalar only at an existing legacy text,
/// Taffy, or test boundary.
pub(in crate::layout::flex) fn flex_horizontal_baseline_from_points(
    points: f32,
) -> FlexHorizontalBaselineOffset {
    FlexHorizontalBaselineOffset::new(layout_pt(points))
}

/// Project a vertical item-baseline offset into a row flex line's cross-axis
/// displacement. Callers select this only when the physical cross axis is
/// vertical.
pub(in crate::layout::flex) fn flex_cross_length_from_vertical_baseline(
    baseline: FlexVerticalBaselineOffset,
) -> FlexCrossLength {
    FlexCrossLength::new(baseline.into_layout_length().points())
}

/// Project a horizontal item-baseline offset into a column flex line's
/// cross-axis displacement. Callers select this only when the physical cross
/// axis is horizontal.
pub(in crate::layout::flex) fn flex_cross_length_from_horizontal_baseline(
    baseline: FlexHorizontalBaselineOffset,
) -> FlexCrossLength {
    FlexCrossLength::new(baseline.into_layout_length().points())
}

/// Project a row flex line's cross-axis coordinate into its physical vertical
/// baseline coordinate. The caller has established that the physical cross
/// axis is vertical.
pub(in crate::layout::flex) fn flex_vertical_baseline_from_cross_offset(
    offset: FlexCrossOffset,
) -> FlexVerticalBaselineOffset {
    flex_vertical_baseline_from_points(offset.points())
}

/// Project a column flex line's cross-axis coordinate into its physical
/// horizontal baseline coordinate. The caller has established that the
/// physical cross axis is horizontal.
pub(in crate::layout::flex) fn flex_horizontal_baseline_from_cross_offset(
    offset: FlexCrossOffset,
) -> FlexHorizontalBaselineOffset {
    flex_horizontal_baseline_from_points(offset.points())
}

/// A synthesized baseline whose physical offset axis is selected at runtime
/// from CSS Writing Modes. Consumers must project it into the known vertical
/// or horizontal Flex baseline axis before using it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::flex) enum FlexPhysicalBaselineOffset {
    Vertical(FlexVerticalBaselineOffset),
    Horizontal(FlexHorizontalBaselineOffset),
}

impl FlexPhysicalBaselineOffset {
    pub(in crate::layout::flex) fn vertical(self) -> FlexVerticalBaselineOffset {
        match self {
            Self::Vertical(offset) => offset,
            Self::Horizontal(_) => {
                unreachable!("vertical baseline caller received horizontal offset")
            }
        }
    }

    pub(in crate::layout::flex) fn horizontal(self) -> FlexHorizontalBaselineOffset {
        match self {
            Self::Vertical(_) => {
                unreachable!("horizontal baseline caller received vertical offset")
            }
            Self::Horizontal(offset) => offset,
        }
    }
}

pub(in crate::layout::flex) type FlexItemBaselinePair<Axis> = BaselinePair<Axis>;

/// Estimated first/last baseline sets, separated by their physical axis.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout::flex) struct FlexItemBaselineEstimate {
    pub(in crate::layout::flex) vertical: FlexItemBaselinePair<FlexVerticalBaselineAxis>,
    pub(in crate::layout::flex) horizontal: FlexItemBaselinePair<FlexHorizontalBaselineAxis>,
}

/// Final first/last baseline sets exported by a flex container's content box.
///
/// This is deliberately distinct from [`FlexItemBaselineEstimate`]: item
/// baselines are intrinsic estimates, whereas these values come from the
/// reconciled flex lines and final item placement. Both preserve the same
/// physical-axis invariant required by CSS Flexbox baseline export:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct FlexContainerBaselineSets {
    pub(in crate::layout) vertical: FlexItemBaselinePair<FlexVerticalBaselineAxis>,
    // Parent inline layout projects this physical record only after its
    // writing mode selects a compatible logical block axis.
    pub(in crate::layout) horizontal: FlexItemBaselinePair<FlexHorizontalBaselineAxis>,
}

impl FlexContainerBaselineSets {
    /// Convert final content-box-relative Flex baselines to the physical
    /// border-box coordinates required by an atomic inline. This is the sole
    /// Flex-to-Inline box-model conversion boundary.
    pub(in crate::layout) fn into_inline_atom_baselines(
        self,
        border_top_and_padding: LayoutLength,
        border_left_and_padding: LayoutLength,
    ) -> PhysicalBaselineSets {
        PhysicalBaselineSets {
            vertical: BaselinePair {
                first: self.vertical.first.map(|baseline| {
                    PhysicalTopBaselineOffset::new(
                        border_top_and_padding + baseline.into_layout_length(),
                    )
                }),
                last: self.vertical.last.map(|baseline| {
                    PhysicalTopBaselineOffset::new(
                        border_top_and_padding + baseline.into_layout_length(),
                    )
                }),
            },
            horizontal: BaselinePair {
                first: self.horizontal.first.map(|baseline| {
                    PhysicalLeftBaselineOffset::new(
                        border_left_and_padding + baseline.into_layout_length(),
                    )
                }),
                last: self.horizontal.last.map(|baseline| {
                    PhysicalLeftBaselineOffset::new(
                        border_left_and_padding + baseline.into_layout_length(),
                    )
                }),
            },
        }
    }
}

/// Physical content-box metrics that Flex keeps typed until it reaches the
/// scalar intrinsic/Taffy metric adapter.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexPhysicalIntrinsicMetrics {
    pub(super) width: PhysicalContentWidth,
    pub(super) height: PhysicalContentHeight,
    pub(super) min_width: PhysicalContentWidth,
    pub(super) min_height: PhysicalContentHeight,
    pub(super) content_width: PhysicalContentWidth,
    pub(super) content_height: PhysicalContentHeight,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemEstimate {
    pub(super) metrics: IntrinsicItemMetrics,
    /// The CSS rule that produced the resolved flex basis used by Taffy.
    ///
    /// This travels with the estimate because a numeric flex rectangle cannot
    /// distinguish a content-derived provisional main size from a transferred
    /// aspect-ratio size. The final normal-flow span correction is valid only
    /// for the former.
    pub(super) main_size_provenance: FlexMainSizeProvenance,
    /// Descendant content extent measured from the flex item's border-box
    /// block start that must remain available to CSS Fragmentation replay.
    ///
    /// This is intentionally distinct from `metrics.content_height`: later
    /// Flexbox cross-size remeasurement can update the used/intrinsic metric
    /// without changing the source range of overflowing descendants.
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>
    pub(super) fragmentable_overflow_height: PhysicalContentHeight,
    /// Flex keeps physical vertical and horizontal exported baselines apart
    /// until a legacy consumer explicitly requests scalar intrinsic metrics.
    pub(super) baselines: FlexItemBaselineEstimate,
    /// The in-flow line-box span produced when the item is replayed at its
    /// resolved flex main size.
    ///
    /// Taffy's leaf rectangle is a sizing input, not a record of the line
    /// boxes selected by the item's independent formatting context.  Keep
    /// that final normal-flow measurement separate from fragmentable overflow
    /// so Flexbox line sizing can consume used geometry without deriving it
    /// from paint bounds or pagination state.
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
    /// <https://www.w3.org/TR/css-inline-3/#line-box>
    pub(super) normal_flow_line_box_span: Option<PhysicalContentHeight>,
}

impl FlexItemEstimate {
    pub(super) fn new(
        mut metrics: IntrinsicItemMetrics,
        baselines: FlexItemBaselineEstimate,
    ) -> Self {
        // `IntrinsicItemMetrics` is shared with scalar Taffy/intrinsic APIs.
        // Do not leave a second, untyped baseline representation in a Flex
        // estimate; `legacy_metrics` performs that adaptation on demand.
        metrics.clear_block_baselines();
        let fragmentable_overflow_height = PhysicalContentHeight::new(metrics.content_height);
        Self {
            metrics,
            main_size_provenance: FlexMainSizeProvenance::NormalFlowContent,
            fragmentable_overflow_height,
            baselines,
            normal_flow_line_box_span: None,
        }
    }

    /// Construct a fixed intrinsic estimate from physical content-box sizes.
    ///
    /// `IntrinsicItemMetrics` is a legacy scalar carrier, so this is the sole
    /// conversion point for fixed Flex estimates.
    pub(super) fn fixed(width: PhysicalContentWidth, height: PhysicalContentHeight) -> Self {
        Self::new(
            IntrinsicItemMetrics::fixed(width.points(), height.points()),
            FlexItemBaselineEstimate::default(),
        )
    }

    /// Adapt fully typed physical intrinsic metrics to the scalar carrier
    /// shared by Taffy and legacy intrinsic-measurement interfaces.
    pub(super) fn from_physical_intrinsic_metrics(
        metrics: FlexPhysicalIntrinsicMetrics,
        preferred_aspect_ratio: Option<f32>,
        baselines: FlexItemBaselineEstimate,
    ) -> Self {
        Self::new(
            IntrinsicItemMetrics {
                width: metrics.width.content_box_length(),
                height: metrics.height.content_box_length(),
                min_width: metrics.min_width.content_box_length(),
                min_height: metrics.min_height.content_box_length(),
                content_width: metrics.content_width.content_box_length(),
                content_height: metrics.content_height.content_box_length(),
                preferred_aspect_ratio,
                first_baseline: None,
                last_baseline: None,
            },
            baselines,
        )
    }

    pub(super) fn legacy_metrics(self) -> IntrinsicItemMetrics {
        let mut metrics = self.metrics;
        metrics.first_baseline = self
            .baselines
            .vertical
            .first
            .map(FlexVerticalBaselineOffset::points);
        metrics.last_baseline = self
            .baselines
            .vertical
            .last
            .map(FlexVerticalBaselineOffset::points);
        metrics
    }

    /// Record the flex-basis rule after the Taffy adapter has resolved it.
    pub(super) fn set_main_size_provenance(&mut self, provenance: FlexMainSizeProvenance) {
        self.main_size_provenance = provenance;
    }

    /// Record the measured descendant source extent independently of the
    /// estimate's ordinary used/intrinsic block metric.
    pub(super) fn set_fragmentable_overflow_height(&mut self, height: PhysicalContentHeight) {
        self.fragmentable_overflow_height = height;
    }

    /// Keep the larger source extent when a resolved main-size remeasurement
    /// reveals additional wrapped descendant overflow.
    pub(super) fn merge_fragmentable_overflow_height(&mut self, height: PhysicalContentHeight) {
        self.fragmentable_overflow_height = PhysicalContentHeight::new(content_box_pt(
            self.fragmentable_overflow_height
                .points()
                .max(height.points()),
        ));
    }

    /// Record the physical block span selected by the item's actual in-flow
    /// formatting context at its resolved flex main size.  This is not an
    /// overflow measurement: callers may use it only as a used line-box
    /// contribution while resolving flex-line geometry.
    pub(super) fn set_normal_flow_line_box_span(&mut self, span: PhysicalContentHeight) {
        self.normal_flow_line_box_span = Some(span);
    }

    pub(super) fn normal_flow_line_box_span(&self) -> Option<PhysicalContentHeight> {
        self.normal_flow_line_box_span
    }

    /// Replace a horizontal row item's normal-flow cross contribution without
    /// letting fragmentable descendant replay overflow become a flex-line
    /// sizing input.
    ///
    /// CSS Flexbox determines a row item's hypothetical cross size by laying
    /// it out as an in-flow block with its used main size. Fragmentation can
    /// retain a longer descendant source extent, but that replay-only extent
    /// must not inflate the line that owns the item's used border box:
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
    pub(super) fn replace_row_normal_flow_cross_contribution_preserving_fragmentable_overflow(
        &mut self,
        remeasured: Self,
    ) {
        let previous_fragmentable_overflow = self.fragmentable_overflow_height;
        self.metrics.height = remeasured.metrics.height;
        self.metrics.min_height = remeasured.metrics.min_height;
        self.metrics.content_height = remeasured.metrics.content_height;
        self.baselines = remeasured.baselines;
        self.fragmentable_overflow_height = remeasured.fragmentable_overflow_height;
        self.merge_fragmentable_overflow_height(previous_fragmentable_overflow);
    }
}

impl Deref for FlexItemEstimate {
    type Target = IntrinsicItemMetrics;

    fn deref(&self) -> &Self::Target {
        &self.metrics
    }
}

impl DerefMut for FlexItemEstimate {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.metrics
    }
}

/// Final flex item border-box geometry in physical container coordinates.
///
/// The rectangle is relative to the flex container content box and uses
/// physical x/y axes after the flex/Taffy adapter has mapped CSS main/cross
/// axes through writing mode and direction. Consumers should use the accessor
/// methods rather than reaching into the rect so that future logical-axis
/// refactors stay localized:
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FlexItemLayout {
    rect: ContainerRect,
    /// Physical block extent available to fragmentation replay.
    ///
    /// A flex item's used border box can be shorter than its non-scrollable
    /// descendant content. The flex algorithm retains that used size, while
    /// CSS Fragmentation needs the larger source extent to produce continuation
    /// slices for the overflowing content:
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
    fragmentation_height: PhysicalContentHeight,
    pub(super) metadata: FragmentPageMetadata,
    pub(super) percentage_height_basis: FlexPercentageBasis,
}

impl FlexItemLayout {
    pub(super) fn new(rect: ContainerRect) -> Self {
        Self::with_metadata(rect, FragmentPageMetadata::empty(0))
    }

    pub(super) fn with_metadata(rect: ContainerRect, metadata: FragmentPageMetadata) -> Self {
        let rect = ContainerRect::new(
            rect.origin,
            ContainerSize::new(rect.size.width.max(0.0), rect.size.height.max(0.0)),
        );
        Self {
            fragmentation_height: PhysicalContentHeight::new(content_box_pt(rect.size.height)),
            rect,
            metadata,
            percentage_height_basis: PercentageBasis::indefinite(),
        }
    }

    pub(super) fn from_taffy_rect(rect: TaffyRect, _axes: FlexAxes) -> Self {
        Self::new(ContainerRect::new(
            ContainerPoint::new(rect.origin.x, rect.origin.y),
            ContainerSize::new(rect.size.width, rect.size.height),
        ))
    }

    pub(super) fn x(&self) -> FlexPhysicalHorizontalOffset {
        FlexPhysicalHorizontalOffset::new(self.rect.origin.x)
    }

    pub(super) fn y(&self) -> FlexPhysicalVerticalOffset {
        FlexPhysicalVerticalOffset::new(self.rect.origin.y)
    }

    pub(super) fn width(&self) -> FlexPhysicalHorizontalSize {
        FlexPhysicalHorizontalSize::new(self.rect.size.width)
    }

    pub(super) fn height(&self) -> FlexPhysicalVerticalSize {
        FlexPhysicalVerticalSize::new(self.rect.size.height)
    }

    pub(super) fn replay_dimensions(&self) -> FlexItemReplayDimensions {
        FlexItemReplayDimensions {
            width: self.width(),
            height: self.height(),
        }
    }

    /// Return the laid-out physical border-box height for CSS percentage
    /// resolution below this flex item.
    pub(super) fn border_box_height(&self) -> BorderBoxLength {
        border_box_pt(self.rect.size.height)
    }

    pub(super) fn fragmentation_height(&self) -> PhysicalContentHeight {
        PhysicalContentHeight::new(content_box_pt(
            self.fragmentation_height
                .points()
                .max(self.height().points()),
        ))
    }

    pub(super) fn set_x(&mut self, x: FlexPhysicalHorizontalOffset) {
        self.rect.origin.x = x.points();
    }

    pub(super) fn set_y(&mut self, y: FlexPhysicalVerticalOffset) {
        self.rect.origin.y = y.points();
    }

    pub(super) fn set_width(&mut self, width: FlexPhysicalHorizontalSize) {
        self.rect.size.width = width.points();
    }

    pub(super) fn set_height(&mut self, height: FlexPhysicalVerticalSize) {
        self.rect.size.height = height.points();
        self.fragmentation_height = PhysicalContentHeight::new(content_box_pt(
            self.fragmentation_height.points().max(height.points()),
        ));
    }

    pub(super) fn set_fragmentation_height(&mut self, height: PhysicalContentHeight) {
        self.fragmentation_height = PhysicalContentHeight::new(content_box_pt(
            height.points().max(self.height().points()).max(0.0),
        ));
    }

    pub(super) fn main_start(&self, axes: FlexAxes) -> FlexMainOffset {
        if axes.is_main_row_axis() {
            FlexMainOffset::new(self.x().points())
        } else {
            FlexMainOffset::new(self.y().points())
        }
    }

    pub(super) fn set_main_start(&mut self, axes: FlexAxes, main_start: FlexMainOffset) {
        if axes.is_main_row_axis() {
            self.set_x(FlexPhysicalHorizontalOffset::new(main_start.points()));
        } else {
            self.set_y(FlexPhysicalVerticalOffset::new(main_start.points()));
        }
    }

    pub(super) fn main_size(&self, axes: FlexAxes) -> FlexMainSize {
        if axes.is_main_row_axis() {
            FlexMainSize::new(self.width().points())
        } else {
            FlexMainSize::new(self.height().points())
        }
    }

    pub(super) fn set_main_size(&mut self, axes: FlexAxes, size: FlexMainSize) {
        if axes.is_main_row_axis() {
            self.set_width(FlexPhysicalHorizontalSize::new(size.points()));
        } else {
            self.set_height(FlexPhysicalVerticalSize::new(size.points()));
        }
    }

    pub(super) fn cross_start(&self, axes: FlexAxes) -> FlexCrossOffset {
        if axes.is_main_row_axis() {
            FlexCrossOffset::new(self.y().points())
        } else {
            FlexCrossOffset::new(self.x().points())
        }
    }

    pub(super) fn set_cross_start(&mut self, axes: FlexAxes, cross_start: FlexCrossOffset) {
        if axes.is_main_row_axis() {
            self.set_y(FlexPhysicalVerticalOffset::new(cross_start.points()));
        } else {
            self.set_x(FlexPhysicalHorizontalOffset::new(cross_start.points()));
        }
    }

    pub(super) fn cross_size(&self, axes: FlexAxes) -> FlexCrossSize {
        if axes.is_main_row_axis() {
            FlexCrossSize::new(self.height().points())
        } else {
            FlexCrossSize::new(self.width().points())
        }
    }

    pub(super) fn set_cross_size(&mut self, axes: FlexAxes, size: FlexCrossSize) {
        if axes.is_main_row_axis() {
            self.set_height(FlexPhysicalVerticalSize::new(size.points()));
        } else {
            self.set_width(FlexPhysicalHorizontalSize::new(size.points()));
        }
    }

    pub(super) fn translate_cross(&mut self, axes: FlexAxes, delta: FlexCrossLength) {
        self.set_cross_start(axes, self.cross_start(axes) + delta);
    }

    pub(super) fn outer_main_bounds(
        &self,
        axes: FlexAxes,
        style: &ComputedStyle,
    ) -> (FlexMainOffset, FlexMainOffset) {
        if axes.is_main_row_axis() {
            (
                FlexMainOffset::new(self.x().points() - style.margin.left),
                FlexMainOffset::new(self.x().points() + self.width().points() + style.margin.right),
            )
        } else {
            (
                FlexMainOffset::new(self.y().points() - style.margin.top),
                FlexMainOffset::new(
                    self.y().points() + self.height().points() + style.margin.bottom,
                ),
            )
        }
    }

    pub(super) fn outer_cross_bounds(
        &self,
        axes: FlexAxes,
        style: &ComputedStyle,
    ) -> (FlexCrossOffset, FlexCrossOffset) {
        if axes.is_main_row_axis() {
            (
                FlexCrossOffset::new(self.y().points() - style.margin.top),
                FlexCrossOffset::new(
                    self.y().points() + self.height().points() + style.margin.bottom,
                ),
            )
        } else {
            (
                FlexCrossOffset::new(self.x().points() - style.margin.left),
                FlexCrossOffset::new(
                    self.x().points() + self.width().points() + style.margin.right,
                ),
            )
        }
    }
}

pub(super) type StyledChild<'a> = FormattingContextChild<'a>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::flex::layout::{
        flex_container_page_contents_overflow_clip, flex_container_page_fragment_bounds,
    };

    #[test]
    fn horizontal_direction_remains_a_taffy_layout_direction_input() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::HorizontalTb;
        style.direction = Direction::Rtl;
        style.flex_direction = FlexDirection::RowReverse;

        assert_eq!(physical_flex_direction(&style), FlexDirection::RowReverse);
    }

    #[test]
    fn maps_vertical_writing_flex_axes_to_physical_axes() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalRl;
        style.direction = Direction::Ltr;
        style.flex_direction = FlexDirection::Column;
        assert_eq!(physical_flex_direction(&style), FlexDirection::RowReverse);

        style.flex_direction = FlexDirection::ColumnReverse;
        assert_eq!(physical_flex_direction(&style), FlexDirection::Row);

        style.flex_direction = FlexDirection::Row;
        assert_eq!(physical_flex_direction(&style), FlexDirection::Column);

        style.direction = Direction::Rtl;
        assert_eq!(
            physical_flex_direction(&style),
            FlexDirection::ColumnReverse
        );

        let PhysicalFlexGaps {
            horizontal: physical_x_gap,
            vertical: physical_y_gap,
        } = physical_flex_gaps(&style);
        assert_eq!(physical_x_gap, style.row_gap);
        assert_eq!(physical_y_gap, style.column_gap);

        let specified = SpecifiedFlexDirection::new(style.flex_direction);
        let physical = PhysicalFlexDirection::new(physical_flex_direction(&style));
        assert_eq!(specified.0, FlexDirection::Row);
        assert_eq!(physical.taffy_direction(), FlexDirection::ColumnReverse);
    }

    #[test]
    fn logical_inline_basis_projects_vertical_available_height() {
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(80.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(100.0))),
            height_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
        };
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.direction = Direction::Rtl;

        assert_eq!(available.logical_inline_basis(&style).points(), Some(100.0));
    }

    #[test]
    fn sideways_lr_reverses_ltr_row_flex_progression_only() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::SidewaysLr;
        style.direction = Direction::Ltr;
        style.flex_direction = FlexDirection::Row;
        assert_eq!(
            physical_flex_direction(&style),
            FlexDirection::ColumnReverse
        );

        style.direction = Direction::Rtl;
        assert_eq!(physical_flex_direction(&style), FlexDirection::Column);

        style.flex_direction = FlexDirection::Column;
        assert_eq!(physical_flex_direction(&style), FlexDirection::Row);
    }

    #[test]
    fn flex_item_fixed_estimate_stores_content_box_lengths() {
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(24.0)),
            PhysicalContentHeight::new(content_box_pt(36.0)),
        );

        assert_eq!(estimate.width.points(), 24.0);
        assert_eq!(estimate.height.points(), 36.0);
        assert_eq!(estimate.min_width.points(), 24.0);
        assert_eq!(estimate.min_height.points(), 36.0);
        assert_eq!(estimate.content_width.points(), 24.0);
        assert_eq!(estimate.content_height.points(), 36.0);
        assert_eq!(estimate.fragmentable_overflow_height.points(), 36.0);
    }

    #[test]
    fn fragmentable_overflow_extent_survives_used_cross_size_remeasurement() {
        let mut estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(24.0)),
            PhysicalContentHeight::new(content_box_pt(36.0)),
        );
        estimate
            .set_fragmentable_overflow_height(PhysicalContentHeight::new(content_box_pt(120.0)));

        // This is the ordinary intrinsic/used metric updated during cross
        // sizing. It must not erase the longer descendant source range.
        estimate.content_height = content_box_pt(48.0);
        assert_eq!(estimate.fragmentable_overflow_height.points(), 120.0);

        estimate
            .merge_fragmentable_overflow_height(PhysicalContentHeight::new(content_box_pt(144.0)));
        assert_eq!(estimate.fragmentable_overflow_height.points(), 144.0);
    }

    #[test]
    fn physical_intrinsic_metrics_cross_the_legacy_adapter_without_axis_swaps() {
        let estimate = FlexItemEstimate::from_physical_intrinsic_metrics(
            FlexPhysicalIntrinsicMetrics {
                width: PhysicalContentWidth::new(content_box_pt(80.0)),
                height: PhysicalContentHeight::new(content_box_pt(90.0)),
                min_width: PhysicalContentWidth::new(content_box_pt(30.0)),
                min_height: PhysicalContentHeight::new(content_box_pt(40.0)),
                content_width: PhysicalContentWidth::new(content_box_pt(70.0)),
                content_height: PhysicalContentHeight::new(content_box_pt(60.0)),
            },
            Some(2.0),
            FlexItemBaselineEstimate::default(),
        );

        assert_eq!(estimate.width, content_box_pt(80.0));
        assert_eq!(estimate.height, content_box_pt(90.0));
        assert_eq!(estimate.min_width, content_box_pt(30.0));
        assert_eq!(estimate.min_height, content_box_pt(40.0));
        assert_eq!(estimate.content_width, content_box_pt(70.0));
        assert_eq!(estimate.content_height, content_box_pt(60.0));
        assert_eq!(estimate.preferred_aspect_ratio, Some(2.0));
    }

    #[test]
    fn flex_axis_size_difference_remains_signed_until_explicitly_clamped() {
        let difference = FlexMainSize::new(20.0) - FlexMainSize::new(35.0);

        assert_eq!(difference.points(), -15.0);
        assert_eq!(difference.non_negative_size(), FlexMainSize::new(0.0));
    }

    #[test]
    fn stretched_cross_size_projects_to_the_matching_physical_content_axis() {
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(80.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
        };

        let mut row_item = FlexItemAvailableSpace::from_container(available);
        row_item.set_definite_cross_size(
            FlexDirection::Row,
            FlexCrossSize::new(24.0),
            FlexAvailableSizeSource::DefiniteCrossSize,
        );
        row_item.set_stretched_cross_size(FlexDirection::Row, FlexCrossSize::new(24.0));
        assert_eq!(
            row_item.height,
            Some(PhysicalContentHeight::new(content_box_pt(24.0)))
        );
        assert_eq!(
            row_item.stretched_height,
            Some(PhysicalContentHeight::new(content_box_pt(24.0)))
        );
        assert_eq!(row_item.width, available.width);

        let mut column_item = FlexItemAvailableSpace::from_container(available);
        column_item.set_definite_cross_size(
            FlexDirection::Column,
            FlexCrossSize::new(36.0),
            FlexAvailableSizeSource::DefiniteCrossSize,
        );
        column_item.set_stretched_cross_size(FlexDirection::Column, FlexCrossSize::new(36.0));
        assert_eq!(
            column_item.width,
            PhysicalContentWidth::new(content_box_pt(36.0))
        );
        assert_eq!(
            column_item.stretched_width,
            Some(PhysicalContentWidth::new(content_box_pt(36.0)))
        );
        assert_eq!(column_item.height, None);
    }

    #[test]
    fn explicit_balanced_line_count_reserves_cross_measurement_space() {
        let mut style = ComputedStyle::initial();
        style.flex_wrap = FlexWrap::Balance;
        style.flex_line_count = css::FlexLineCount::Count(
            std::num::NonZeroUsize::new(2).expect("line count is non-zero"),
        );
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(100.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
        };

        let measurement =
            balanced_flex_item_measure_available_space(&style, FlexDirection::Column, available);

        assert_eq!(measurement.width.points(), 50.0);
        assert_eq!(measurement.width_basis, available.width_basis);
    }

    #[test]
    fn flex_cross_percentage_basis_follows_the_resolved_physical_axis() {
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(80.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(120.0))),
            height_basis: PercentageBasis::definite_from(
                content_box_pt(120.0),
                FlexAvailableSizeSource::DefiniteCrossSize,
            ),
        };

        assert_eq!(
            available.cross_basis(FlexDirection::Row),
            available.height_basis
        );
        assert_eq!(
            available.cross_basis(FlexDirection::Column),
            available.width_basis
        );
    }

    #[test]
    fn definite_flex_axis_sizes_do_not_promote_numeric_cyclic_constraints() {
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::indefinite(),
            height: Some(PhysicalContentHeight::new(content_box_pt(120.0))),
            height_basis: PercentageBasis::indefinite(),
        };

        // An automatic physical size can still constrain Taffy's layout, but
        // CSS Sizing does not make it a definite main/cross percentage basis.
        assert_eq!(available.definite_main_size(FlexDirection::Row), None);
        assert_eq!(available.definite_cross_size(FlexDirection::Row), None);
        assert_eq!(available.definite_main_size(FlexDirection::Column), None);
        assert_eq!(available.definite_cross_size(FlexDirection::Column), None);
    }

    #[test]
    fn flex_item_layout_projects_main_and_cross_axes() {
        let row_axes =
            FlexAxes::from_physical_direction(PhysicalFlexDirection::new(FlexDirection::Row));
        let column_axes =
            FlexAxes::from_physical_direction(PhysicalFlexDirection::new(FlexDirection::Column));
        let mut item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(10.0, 20.0),
            ContainerSize::new(30.0, 40.0),
        ));

        assert_eq!(item.main_start(row_axes), FlexMainOffset::new(10.0));
        assert_eq!(item.main_size(row_axes), FlexMainSize::new(30.0));
        assert_eq!(item.cross_start(row_axes), FlexCrossOffset::new(20.0));
        assert_eq!(item.cross_size(row_axes), FlexCrossSize::new(40.0));

        assert_eq!(item.main_start(column_axes), FlexMainOffset::new(20.0));
        assert_eq!(item.main_size(column_axes), FlexMainSize::new(40.0));
        assert_eq!(item.cross_start(column_axes), FlexCrossOffset::new(10.0));
        assert_eq!(item.cross_size(column_axes), FlexCrossSize::new(30.0));

        item.set_main_start(column_axes, FlexMainOffset::new(25.0));
        item.translate_cross(column_axes, FlexCrossLength::new(5.0));
        assert_eq!(item.y(), FlexPhysicalVerticalOffset::new(25.0));
        assert_eq!(item.x(), FlexPhysicalHorizontalOffset::new(15.0));
    }

    #[test]
    fn flex_item_layout_wraps_taffy_rects_at_boundary() {
        let axes =
            FlexAxes::from_physical_direction(PhysicalFlexDirection::new(FlexDirection::Row));
        let rect = TaffyRect::new(TaffyPoint::new(4.0, 8.0), TaffySize::new(16.0, 32.0));
        let item = FlexItemLayout::from_taffy_rect(rect, axes);

        assert_eq!(item.x(), FlexPhysicalHorizontalOffset::new(4.0));
        assert_eq!(item.y(), FlexPhysicalVerticalOffset::new(8.0));
        assert_eq!(item.width(), FlexPhysicalHorizontalSize::new(16.0));
        assert_eq!(item.height(), FlexPhysicalVerticalSize::new(32.0));

        let replay = item.replay_dimensions();
        let _: BorderBoxLength = replay.border_box_width();
        let _: PhysicalContentWidth = replay.available_width_for_replay();
        assert_eq!(replay.border_box_width(), border_box_pt(16.0));
        assert_eq!(
            replay.available_width_for_replay(),
            PhysicalContentWidth::new(content_box_pt(16.0))
        );
        assert_eq!(replay.border_box_height(), border_box_pt(32.0));
        assert_eq!(
            replay.available_height_for_replay(),
            PhysicalContentHeight::new(content_box_pt(32.0))
        );
    }

    #[test]
    fn flex_fragment_offset_difference_remains_signed_until_explicitly_clamped() {
        let start = FlexFragmentBlockOffset::new(20.0);
        let end = FlexFragmentBlockOffset::new(50.0);

        assert_eq!(end - start, FlexFragmentBlockLength::new(30.0));
        assert_eq!(start - end, FlexFragmentBlockLength::new(-30.0));
        assert_eq!(
            (start - end).non_negative_size(),
            FlexFragmentBlockSize::new(0.0)
        );

        let bounds =
            FlexFragmentBlockBounds::from_start_and_size(start, FlexFragmentBlockSize::new(30.0));
        assert_eq!(bounds.start(), start);
        assert_eq!(bounds.end(), end);
        assert_eq!(bounds.size(), FlexFragmentBlockSize::new(30.0));
    }

    #[test]
    fn materialized_fragment_plan_classifies_item_fragment_starts() {
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(20.0, 200.0),
        ));
        let fragment = |page_index, item_index, start, end| FlexFragmentLayout {
            page_index,
            line_start: 0,
            line_end: 1,
            block_start: FlexFragmentBlockOffset::new(start),
            block_end: FlexFragmentBlockOffset::new(end),
            line_fragments: vec![FlexLineFragmentLayout {
                line_index: 0,
                source_bounds: FlexFragmentBlockBounds::new(
                    FlexFragmentBlockOffset::new(start),
                    FlexFragmentBlockOffset::new(end),
                ),
                item_indices: vec![item_index],
            }],
            items: vec![FlexItemFragmentLayout {
                item_index,
                source_item_index: item_index,
                line_index: 0,
                source_bounds: item.clone(),
                used_bounds: item.clone(),
                bounds: item.clone(),
                content_slice: FlexFragmentSlice {
                    block_start: FlexFragmentBlockOffset::new(start),
                    block_end: FlexFragmentBlockOffset::new(end),
                },
                decoration_slice: FlexFragmentSlice {
                    block_start: FlexFragmentBlockOffset::new(start),
                    block_end: FlexFragmentBlockOffset::new(end),
                },
                continuation: FlexItemContinuation::default(),
                metadata: FragmentPageMetadata::empty(page_index),
            }],
            metadata: FragmentPageMetadata::empty(page_index),
        };
        let mut plan = FlexFragmentPlan::default();
        let mut first = fragment(3, 0, 0.0, 75.0);
        plan.prepare_materialized_fragment(&mut first);
        assert_eq!(
            first.items[0].continuation.fragment_start,
            FlexItemFragmentStart::ItemStart
        );
        assert_eq!(
            first.items[0].continuation.child_fragment_replay_ordinal(),
            0
        );
        let mut materialized_first = MaterializedFlexFragment::new(
            first,
            Some(PaintClip::new(0.0, 0.0, 20.0, 75.0)),
            FlexFragmentDecorationState {
                includes_block_start: true,
                includes_block_end: false,
            },
            PaintTranslation::identity(),
        );
        materialized_first
            .item_fragments
            .push(MaterializedFlexItemFragment::from_planned(
                &materialized_first.layout.items[0],
                PaintClip::new(0.0, 0.0, 20.0, 75.0),
                PaintTranslation::identity(),
            ));
        materialized_first.contents_overflow_clip = Some(AxisSelectivePaintClip::new(
            PaintClip::new(1.0, 2.0, 18.0, 70.0),
            true,
            false,
        ));
        plan.push_materialized_fragment(materialized_first);
        assert_eq!(plan.materialized_fragments.len(), 1);
        assert_eq!(
            plan.materialized_fragments[0].source_bounds,
            FlexFragmentBlockBounds::new(
                FlexFragmentBlockOffset::new(0.0),
                FlexFragmentBlockOffset::new(75.0),
            ),
        );
        assert!(
            plan.materialized_fragments[0]
                .decoration
                .includes_block_start
        );
        assert!(!plan.materialized_fragments[0].decoration.includes_block_end);
        assert_eq!(plan.materialized_fragments[0].item_fragments.len(), 1);
        let materialized_item = &plan.materialized_fragments[0].item_fragments[0];
        assert_eq!(
            materialized_item.content_slice,
            FlexFragmentSlice {
                block_start: FlexFragmentBlockOffset::new(0.0),
                block_end: FlexFragmentBlockOffset::new(75.0),
            },
            "replay consumes the committed source-content intersection",
        );
        assert_eq!(
            materialized_item.decoration_slice, materialized_item.content_slice,
            "decoration ownership is retained independently in the committed item record",
        );
        assert_eq!(
            materialized_item.source_bounds.height(),
            FlexPhysicalVerticalSize::new(200.0),
            "the committed source range is not inferred from its local border box",
        );
        assert_eq!(
            flex_container_page_fragment_bounds(&plan, 3),
            Some(PaintClip::new(0.0, 0.0, 20.0, 75.0)),
            "container decoration uses the committed destination fragment, not source metadata",
        );
        assert_eq!(
            flex_container_page_contents_overflow_clip(&plan, 3),
            Some(AxisSelectivePaintClip::new(
                PaintClip::new(1.0, 2.0, 18.0, 70.0),
                true,
                false,
            )),
            "overflow clipping follows the committed destination fragment",
        );

        let mut leading_paint_overflow = fragment(5, 1, 3.0, 75.0);
        plan.prepare_materialized_fragment(&mut leading_paint_overflow);
        assert_eq!(
            leading_paint_overflow.items[0].continuation.fragment_start,
            FlexItemFragmentStart::LeadingPaintOverflow,
            "a positive source-slice offset without a committed predecessor is leading paint overflow",
        );
        assert_eq!(
            leading_paint_overflow.items[0]
                .continuation
                .child_fragment_replay_ordinal(),
            0,
            "leading paint overflow must replay the child's initial fragment",
        );

        let mut second = fragment(7, 0, 75.0, 150.0);
        plan.prepare_materialized_fragment(&mut second);
        assert_eq!(
            second.items[0].continuation.fragment_start,
            FlexItemFragmentStart::Continuation {
                ordinal: NonZeroUsize::new(1).expect("continuation ordinal is non-zero"),
            }
        );
        assert!(
            second.items[0]
                .continuation
                .continues_from_previous_fragment()
        );
        assert_eq!(
            second.items[0].continuation.child_fragment_replay_ordinal(),
            1
        );
        assert_eq!(second.items[0].continuation.fragmentainer_index, 7);
        assert_eq!(
            second.items[0]
                .continuation
                .source_content_slice
                .block_start
                .points(),
            75.0
        );
    }
}
