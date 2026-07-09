use super::*;
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
pub(super) struct FlexUsedStyle(ComputedStyle);

impl FlexUsedStyle {
    pub(super) fn from_normalized(style: ComputedStyle) -> Self {
        debug_assert!(style.zoom_applied);
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
}

impl<Axis> Add<FlexAxisSize<Axis>> for FlexAxisOffset<Axis> {
    type Output = Self;

    fn add(self, size: FlexAxisSize<Axis>) -> Self {
        Self::new(self.0 + size.0)
    }
}

impl<Axis> Sub for FlexAxisOffset<Axis> {
    type Output = FlexAxisSize<Axis>;

    fn sub(self, other: Self) -> Self::Output {
        FlexAxisSize::new(self.0 - other.0)
    }
}

pub(super) type FlexMainOffset = FlexAxisOffset<FlexMainAxis>;
pub(super) type FlexCrossOffset = FlexAxisOffset<FlexCrossAxis>;
pub(super) type FlexCrossSize = FlexAxisSize<FlexCrossAxis>;

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
    pub(super) height: f32,
    pub(super) first_baseline: Option<f32>,
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
    DefinitePreferredMainSize,
    DefinitePreferredCrossSize,
}

pub(super) type FlexAvailablePercentageBasis =
    PercentageBasis<ContentBoxLength, FlexAvailableSizeSource>;

pub(super) fn flex_available_percentage_basis_from_points(
    value: Option<f32>,
    source: FlexAvailableSizeSource,
) -> FlexAvailablePercentageBasis {
    value
        .map(|value| PercentageBasis::definite_from(content_box_pt(value), source))
        .unwrap_or_else(PercentageBasis::indefinite)
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

impl FlexLineLayout {
    pub(super) fn cross_size(&self) -> FlexCrossSize {
        self.cross_end - self.cross_start
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

impl FlexFragmentBlockSize {
    pub(super) fn new(points: f32) -> Self {
        Self(points.max(0.0))
    }
}

impl Add<FlexFragmentBlockSize> for FlexFragmentBlockOffset {
    type Output = Self;

    fn add(self, size: FlexFragmentBlockSize) -> Self {
        Self::new(self.0 + size.0)
    }
}

impl Sub for FlexFragmentBlockOffset {
    type Output = FlexFragmentBlockSize;

    fn sub(self, other: Self) -> Self::Output {
        FlexFragmentBlockSize::new(self.0 - other.0)
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
    pub(super) fragments: Vec<FlexFragmentLayout>,
}

impl FlexFragmentPlan {
    /// Prepare a fragment while the flex break units are consumed.
    ///
    /// The initial Taffy result is source geometry, not a fragment plan.
    /// Continuation ordinals follow materialized fragmentainers, including a
    /// partial first fragmentainer and forced transitions.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(super) fn prepare_materialized_fragment(&self, fragment: &mut FlexFragmentLayout) {
        for item in &mut fragment.items {
            let continuation_ordinal = self
                .fragments
                .iter()
                .flat_map(|previous| &previous.items)
                .filter(|previous| {
                    previous.source_item_index == item.source_item_index
                        && previous.content_slice.block_end.points()
                            <= item.content_slice.block_start.points() + 0.01
                })
                .count();
            item.continuation.source_content_slice = item.content_slice;
            item.continuation.decoration_slice = item.decoration_slice;
            item.continuation.fragmentainer_index = fragment.page_index;
            item.continuation.continuation_ordinal = continuation_ordinal;
        }
    }

    /// Append a fragment after its paint and page-side effects are committed.
    pub(super) fn push_materialized_fragment(&mut self, fragment: FlexFragmentLayout) {
        self.fragments.push(fragment);
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
                let _block_span = fragment.block_end - fragment.block_start;
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
    pub(super) items: Vec<FlexItemFragmentLayout>,
    pub(super) metadata: FragmentPageMetadata,
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
    pub(super) original_bounds: FlexItemLayout,
    pub(super) bounds: FlexItemLayout,
    pub(super) content_slice: FlexFragmentSlice,
    pub(super) decoration_slice: FlexFragmentSlice,
    /// Replay input derived from the committed fragmentainer sequence.
    pub(super) continuation: FlexItemContinuation,
    pub(super) metadata: FragmentPageMetadata,
}

/// Source and fragmentainer state for one flex item continuation.
///
/// Content and decoration slices are both retained because
/// `box-decoration-break` may choose a different decoration range without
/// changing descendant source flow.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct FlexItemContinuation {
    pub(super) source_content_slice: FlexFragmentSlice,
    pub(super) decoration_slice: FlexFragmentSlice,
    /// Remaining capacity in the first local fragmentainer for this item.
    pub(super) first_fragmentainer_capacity: f32,
    /// Capacity available to each later local fragmentainer.
    pub(super) continuation_fragmentainer_capacity: f32,
    pub(super) fragmentainer_index: usize,
    pub(super) continuation_ordinal: usize,
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
    let axes = WritingModeAxes::new(style.writing_mode, style.direction);
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
    let axes = WritingModeAxes::new(style.writing_mode, style.direction);
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
    /// A physical content-box height, not a logical block size.
    pub(super) height: Option<PhysicalContentHeight>,
    pub(super) height_basis: FlexAvailablePercentageBasis,
}

impl FlexAvailableSpace {
    pub(super) fn width_basis_points(self) -> Option<f32> {
        self.width_basis.points()
    }

    pub(super) fn height_basis_points(self) -> Option<f32> {
        self.height_basis.points()
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
    let Some(line_count) = style
        .flex_wrap
        .balances_lines()
        .then_some(style.flex_line_count)
        .flatten()
    else {
        return available;
    };
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
        (height.points(), available.height_basis, vertical)
    } else {
        (available.width.points(), available.width_basis, horizontal)
    };
    let cross_gap = if cross_basis.is_definite() {
        used_flex_gap_with_basis(cross_gap, cross_basis).points()
    } else {
        match cross_gap {
            css::ComputedGap::Normal => 0.0,
            css::ComputedGap::LengthPercentage(value) => value.length_max_zero().points(),
        }
    };
    let item_cross_size =
        (cross_size - cross_gap * line_count.saturating_sub(1) as f32).max(0.0) / line_count as f32;

    let mut item_available = available;
    if physical_direction.is_row_axis() {
        item_available.height = Some(PhysicalContentHeight::new(content_box_pt(item_cross_size)));
    } else {
        item_available.width = PhysicalContentWidth::new(content_box_pt(item_cross_size));
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
        let axes = WritingModeAxes::new(style.writing_mode, style.direction);
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
        if WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes() {
            self.height_basis
        } else {
            self.width_basis
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemEstimate {
    pub(super) metrics: IntrinsicItemMetrics,
    pub(super) first_horizontal_baseline: Option<f32>,
    pub(super) last_horizontal_baseline: Option<f32>,
}

impl FlexItemEstimate {
    pub(super) fn fixed(width: f32, height: f32) -> Self {
        Self {
            metrics: IntrinsicItemMetrics::fixed(width, height),
            first_horizontal_baseline: None,
            last_horizontal_baseline: None,
        }
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
    fragmentation_height: f32,
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
            fragmentation_height: rect.size.height,
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

    pub(super) fn x(&self) -> f32 {
        self.rect.origin.x
    }

    pub(super) fn y(&self) -> f32 {
        self.rect.origin.y
    }

    pub(super) fn width(&self) -> f32 {
        self.rect.size.width
    }

    pub(super) fn height(&self) -> f32 {
        self.rect.size.height
    }

    pub(super) fn fragmentation_height(&self) -> f32 {
        self.fragmentation_height.max(self.height())
    }

    pub(super) fn set_x(&mut self, x: f32) {
        self.rect.origin.x = x;
    }

    pub(super) fn set_y(&mut self, y: f32) {
        self.rect.origin.y = y;
    }

    pub(super) fn set_width(&mut self, width: f32) {
        self.rect.size.width = width.max(0.0);
    }

    pub(super) fn set_height(&mut self, height: f32) {
        self.rect.size.height = height.max(0.0);
        self.fragmentation_height = self.fragmentation_height.max(self.rect.size.height);
    }

    pub(super) fn set_fragmentation_height(&mut self, height: f32) {
        self.fragmentation_height = height.max(self.height()).max(0.0);
    }

    pub(super) fn main_start(&self, axes: FlexAxes) -> f32 {
        if axes.is_main_row_axis() {
            self.x()
        } else {
            self.y()
        }
    }

    pub(super) fn set_main_start(&mut self, axes: FlexAxes, main_start: f32) {
        if axes.is_main_row_axis() {
            self.set_x(main_start);
        } else {
            self.set_y(main_start);
        }
    }

    pub(super) fn main_size(&self, axes: FlexAxes) -> f32 {
        if axes.is_main_row_axis() {
            self.width()
        } else {
            self.height()
        }
    }

    pub(super) fn set_main_size(&mut self, axes: FlexAxes, size: f32) {
        if axes.is_main_row_axis() {
            self.set_width(size);
        } else {
            self.set_height(size);
        }
    }

    pub(super) fn cross_start(&self, axes: FlexAxes) -> f32 {
        if axes.is_main_row_axis() {
            self.y()
        } else {
            self.x()
        }
    }

    pub(super) fn set_cross_start(&mut self, axes: FlexAxes, cross_start: f32) {
        if axes.is_main_row_axis() {
            self.set_y(cross_start);
        } else {
            self.set_x(cross_start);
        }
    }

    pub(super) fn cross_size(&self, axes: FlexAxes) -> f32 {
        if axes.is_main_row_axis() {
            self.height()
        } else {
            self.width()
        }
    }

    pub(super) fn set_cross_size(&mut self, axes: FlexAxes, size: f32) {
        if axes.is_main_row_axis() {
            self.set_height(size);
        } else {
            self.set_width(size);
        }
    }

    pub(super) fn translate_cross(&mut self, axes: FlexAxes, delta: f32) {
        self.set_cross_start(axes, self.cross_start(axes) + delta);
    }

    pub(super) fn outer_main_bounds(&self, axes: FlexAxes, style: &ComputedStyle) -> (f32, f32) {
        if axes.is_main_row_axis() {
            (
                self.x() - style.margin.left,
                self.x() + self.width() + style.margin.right,
            )
        } else {
            (
                self.y() - style.margin.top,
                self.y() + self.height() + style.margin.bottom,
            )
        }
    }

    pub(super) fn outer_cross_bounds(&self, axes: FlexAxes, style: &ComputedStyle) -> (f32, f32) {
        if axes.is_main_row_axis() {
            (
                self.y() - style.margin.top,
                self.y() + self.height() + style.margin.bottom,
            )
        } else {
            (
                self.x() - style.margin.left,
                self.x() + self.width() + style.margin.right,
            )
        }
    }
}

pub(super) type StyledChild<'a> = FormattingContextChild<'a>;

#[cfg(test)]
mod tests {
    use super::*;

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
        let estimate = FlexItemEstimate::fixed(24.0, 36.0);

        assert_eq!(estimate.width.points(), 24.0);
        assert_eq!(estimate.height.points(), 36.0);
        assert_eq!(estimate.min_width.points(), 24.0);
        assert_eq!(estimate.min_height.points(), 36.0);
        assert_eq!(estimate.content_width.points(), 24.0);
        assert_eq!(estimate.content_height.points(), 36.0);
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

        assert_eq!(item.main_start(row_axes), 10.0);
        assert_eq!(item.main_size(row_axes), 30.0);
        assert_eq!(item.cross_start(row_axes), 20.0);
        assert_eq!(item.cross_size(row_axes), 40.0);

        assert_eq!(item.main_start(column_axes), 20.0);
        assert_eq!(item.main_size(column_axes), 40.0);
        assert_eq!(item.cross_start(column_axes), 10.0);
        assert_eq!(item.cross_size(column_axes), 30.0);

        item.set_main_start(column_axes, 25.0);
        item.translate_cross(column_axes, 5.0);
        assert_eq!(item.y(), 25.0);
        assert_eq!(item.x(), 15.0);
    }

    #[test]
    fn flex_item_layout_wraps_taffy_rects_at_boundary() {
        let axes =
            FlexAxes::from_physical_direction(PhysicalFlexDirection::new(FlexDirection::Row));
        let rect = TaffyRect::new(TaffyPoint::new(4.0, 8.0), TaffySize::new(16.0, 32.0));
        let item = FlexItemLayout::from_taffy_rect(rect, axes);

        assert_eq!(item.x(), 4.0);
        assert_eq!(item.y(), 8.0);
        assert_eq!(item.width(), 16.0);
        assert_eq!(item.height(), 32.0);
    }

    #[test]
    fn materialized_fragment_plan_assigns_item_local_continuation_ordinals() {
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(20.0, 200.0),
        ));
        let fragment = |page_index, start, end| FlexFragmentLayout {
            page_index,
            line_start: 0,
            line_end: 1,
            block_start: FlexFragmentBlockOffset::new(start),
            block_end: FlexFragmentBlockOffset::new(end),
            items: vec![FlexItemFragmentLayout {
                item_index: 0,
                source_item_index: 0,
                original_bounds: item.clone(),
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
        let mut first = fragment(3, 0.0, 75.0);
        plan.prepare_materialized_fragment(&mut first);
        assert_eq!(first.items[0].continuation.continuation_ordinal, 0);
        plan.push_materialized_fragment(first);

        let mut second = fragment(7, 75.0, 150.0);
        plan.prepare_materialized_fragment(&mut second);
        assert_eq!(second.items[0].continuation.continuation_ordinal, 1);
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
