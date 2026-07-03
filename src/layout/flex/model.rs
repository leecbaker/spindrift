use super::*;

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
    pub(super) main_start: f32,
    pub(super) main_end: f32,
    pub(super) cross_start: f32,
    pub(super) cross_end: f32,
    pub(super) first_baseline: Option<f32>,
    pub(super) last_baseline: Option<f32>,
    pub(super) collapsed_struts: Vec<FlexCollapsedStrut>,
}

impl FlexLineLayout {
    pub(super) fn main_size(&self) -> f32 {
        (self.main_end - self.main_start).max(0.0)
    }

    pub(super) fn cross_size(&self) -> f32 {
        (self.cross_end - self.cross_start).max(0.0)
    }

    pub(super) fn largest_collapsed_strut(&self) -> f32 {
        self.collapsed_struts
            .iter()
            .map(|strut| strut.cross_size)
            .fold(0.0f32, f32::max)
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
    pub(super) cross_size: f32,
    pub(super) source_start: usize,
    pub(super) source_end: usize,
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
    pub(super) fn from_unfragmented_lines(
        lines: &[FlexLineLayout],
        items: &[FlexItemLayout],
    ) -> Self {
        if lines.is_empty() {
            return Self::default();
        }
        let _unfragmented_main_extent = lines
            .iter()
            .map(FlexLineLayout::main_size)
            .fold(0.0f32, f32::max);

        Self {
            fragments: vec![FlexFragmentLayout {
                page_index: 0,
                line_start: 0,
                line_end: lines.len(),
                block_start: 0.0,
                block_end: lines
                    .iter()
                    .map(|line| line.cross_end)
                    .fold(0.0f32, f32::max),
                items: items
                    .iter()
                    .enumerate()
                    .map(|(item_index, item)| FlexItemFragmentLayout {
                        item_index,
                        source_item_index: item_index,
                        original_bounds: item.clone(),
                        bounds: item.clone(),
                        content_slice: FlexFragmentSlice::full(item.height()),
                        decoration_slice: FlexFragmentSlice::full(item.height()),
                        metadata: item.metadata.clone(),
                    })
                    .collect(),
                metadata: FragmentPageMetadata::empty(0),
            }],
        }
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
                let _block_span = (fragment.block_end - fragment.block_start).max(0.0);
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
    pub(super) block_start: f32,
    pub(super) block_end: f32,
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
    pub(super) metadata: FragmentPageMetadata,
}

/// Block-axis slice of a flex fragment relative to the source border box.
///
/// CSS Fragmentation splits box content and cloned decorations into
/// fragment-local slices:
/// <https://www.w3.org/TR/css-break-3/#box-splitting>.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct FlexFragmentSlice {
    pub(super) block_start: f32,
    pub(super) block_end: f32,
}

impl FlexFragmentSlice {
    pub(super) fn full(block_size: f32) -> Self {
        Self {
            block_start: 0.0,
            block_end: block_size.max(0.0),
        }
    }
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
    pub(super) specified_direction: FlexDirection,
    pub(super) physical_direction: FlexDirection,
}

impl FlexAxes {
    pub(super) fn for_style(style: &ComputedStyle) -> Self {
        Self {
            flow: FlowAxes::for_style(style),
            specified_direction: style.flex_direction,
            physical_direction: physical_flex_direction(style),
        }
    }

    pub(super) fn from_physical_direction(physical_direction: FlexDirection) -> Self {
        Self {
            flow: FlowAxes::new(WritingMode::HorizontalTb, Direction::Ltr),
            specified_direction: physical_direction,
            physical_direction,
        }
    }

    pub(super) fn is_main_row_axis(self) -> bool {
        self.physical_direction.is_row_axis()
    }
}

/// Maps CSS flex main/cross axes into Reasyprint's physical layout axes.
///
/// CSS Flexbox defines `row` from the inline axis and `column` from the block
/// axis, while Taffy lays out rows on physical X and columns on physical Y.
/// CSS Writing Modes maps those logical axes to physical axes:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(super) fn physical_flex_direction(style: &ComputedStyle) -> FlexDirection {
    match style.writing_mode {
        WritingMode::HorizontalTb => style.flex_direction,
        WritingMode::VerticalRl => match (style.flex_direction, style.direction) {
            (FlexDirection::Row, Direction::Ltr) => FlexDirection::Column,
            (FlexDirection::Row, Direction::Rtl) => FlexDirection::ColumnReverse,
            (FlexDirection::RowReverse, Direction::Ltr) => FlexDirection::ColumnReverse,
            (FlexDirection::RowReverse, Direction::Rtl) => FlexDirection::Column,
            (FlexDirection::Column, _) => FlexDirection::RowReverse,
            (FlexDirection::ColumnReverse, _) => FlexDirection::Row,
        },
        WritingMode::VerticalLr => match (style.flex_direction, style.direction) {
            (FlexDirection::Row, Direction::Ltr) => FlexDirection::Column,
            (FlexDirection::Row, Direction::Rtl) => FlexDirection::ColumnReverse,
            (FlexDirection::RowReverse, Direction::Ltr) => FlexDirection::ColumnReverse,
            (FlexDirection::RowReverse, Direction::Rtl) => FlexDirection::Column,
            (FlexDirection::Column, _) => FlexDirection::Row,
            (FlexDirection::ColumnReverse, _) => FlexDirection::RowReverse,
        },
    }
}

/// Returns physical row/column gaps for a flex container.
///
/// CSS Box Alignment maps `row-gap` to the block axis and `column-gap` to the
/// inline axis. Taffy expects physical X/Y gap values, so vertical writing
/// modes swap the physical row and column gap inputs:
/// <https://www.w3.org/TR/css-align-3/#gaps> and
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
pub(super) fn physical_flex_gaps(style: &ComputedStyle) -> (css::ComputedGap, css::ComputedGap) {
    match style.writing_mode {
        WritingMode::HorizontalTb => (style.column_gap, style.row_gap),
        WritingMode::VerticalRl | WritingMode::VerticalLr => (style.row_gap, style.column_gap),
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
    pub(super) width: f32,
    /// Whether `width` is the flex container's definite physical content width.
    ///
    /// CSS Sizing resolves percentages only when the corresponding containing
    /// block size is definite. Flex intrinsic sizing may have an available
    /// width constraint without having a definite container width:
    /// <https://www.w3.org/TR/css-sizing-3/#definite>.
    pub(super) width_is_definite: bool,
    pub(super) height: Option<f32>,
    /// Whether `height` is the flex container's definite physical content height.
    ///
    /// CSS Flexbox wraps against the available main size, but CSS Sizing keeps
    /// `max-height` as a constraint on an automatic used height rather than a
    /// definite height:
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-line-break> and
    /// <https://www.w3.org/TR/css-sizing-3/#preferred-size-properties>.
    pub(super) height_is_definite: bool,
}

/// Available physical container space used while estimating one flex item.
///
/// This is still physical width/height. The `inline_size` and `inline_basis`
/// helpers perform the CSS Writing Modes projection needed by percentage
/// resolution in descendants:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemAvailableSpace {
    pub(super) width: f32,
    pub(super) width_is_definite: bool,
    pub(super) height: Option<f32>,
    pub(super) height_is_definite: bool,
    pub(super) stretched_width: Option<f32>,
    pub(super) stretched_height: Option<f32>,
}

impl FlexItemAvailableSpace {
    pub(super) fn from_container(available: FlexAvailableSpace) -> Self {
        Self {
            width: available.width,
            width_is_definite: available.width_is_definite,
            height: available.height,
            height_is_definite: available.height_is_definite,
            stretched_width: None,
            stretched_height: None,
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
    pub(super) fn inline_size(self, style: &ComputedStyle) -> f32 {
        match style.writing_mode {
            WritingMode::HorizontalTb => self.width,
            WritingMode::VerticalRl | WritingMode::VerticalLr => self.height.unwrap_or(self.width),
        }
    }

    pub(super) fn inline_basis(self, style: &ComputedStyle) -> Option<f32> {
        match style.writing_mode {
            WritingMode::HorizontalTb => self.width_is_definite.then_some(self.width),
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                self.height.filter(|_| self.height_is_definite)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FlexItemEstimate {
    pub(super) width: ContentBoxLength,
    pub(super) height: ContentBoxLength,
    pub(super) min_width: ContentBoxLength,
    pub(super) min_height: ContentBoxLength,
    pub(super) content_width: ContentBoxLength,
    pub(super) content_height: ContentBoxLength,
    pub(super) preferred_aspect_ratio: Option<f32>,
    pub(super) first_baseline: Option<f32>,
    pub(super) last_baseline: Option<f32>,
    pub(super) first_horizontal_baseline: Option<f32>,
    pub(super) last_horizontal_baseline: Option<f32>,
}

impl FlexItemEstimate {
    pub(super) fn fixed(width: f32, height: f32) -> Self {
        let width = content_box_pt(width);
        let height = content_box_pt(height);
        Self {
            width,
            height,
            min_width: width,
            min_height: height,
            content_width: width,
            content_height: height,
            preferred_aspect_ratio: None,
            first_baseline: None,
            last_baseline: None,
            first_horizontal_baseline: None,
            last_horizontal_baseline: None,
        }
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
    pub(super) metadata: FragmentPageMetadata,
}

impl FlexItemLayout {
    pub(super) fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self::with_metadata(x, y, width, height, FragmentPageMetadata::empty(0))
    }

    pub(super) fn with_metadata(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        metadata: FragmentPageMetadata,
    ) -> Self {
        Self {
            rect: ContainerRect::new(
                ContainerPoint::new(x, y),
                ContainerSize::new(width.max(0.0), height.max(0.0)),
            ),
            metadata,
        }
    }

    pub(super) fn from_taffy_rect(rect: TaffyRect, _axes: FlexAxes) -> Self {
        Self::new(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
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

        let (physical_x_gap, physical_y_gap) = physical_flex_gaps(&style);
        assert_eq!(physical_x_gap, style.row_gap);
        assert_eq!(physical_y_gap, style.column_gap);
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
        let row_axes = FlexAxes::from_physical_direction(FlexDirection::Row);
        let column_axes = FlexAxes::from_physical_direction(FlexDirection::Column);
        let mut item = FlexItemLayout::new(10.0, 20.0, 30.0, 40.0);

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
        let axes = FlexAxes::from_physical_direction(FlexDirection::Row);
        let rect = TaffyRect::new(TaffyPoint::new(4.0, 8.0), TaffySize::new(16.0, 32.0));
        let item = FlexItemLayout::from_taffy_rect(rect, axes);

        assert_eq!(item.x(), 4.0);
        assert_eq!(item.y(), 8.0);
        assert_eq!(item.width(), 16.0);
        assert_eq!(item.height(), 32.0);
    }
}
