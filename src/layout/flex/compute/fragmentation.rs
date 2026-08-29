use super::*;

pub(in crate::layout::flex) fn assign_flex_item_fragmentation_heights(
    states: &mut [FlexItemSizingState],
    children: &[StyledChild<'_>],
) {
    for (state, child) in states.iter_mut().zip(children) {
        let estimate = state.estimate();
        let item = state.allocation_mut();
        if !child.style.overflow_y.is_scrollable() {
            // Scrollable overflow remains inside the flex item's scrollport.
            // It may contribute visual/clipped descendant paint, but it must
            // not manufacture additional page-fragment slices beyond the used
            // flex item border box; doing so turns `overflow: hidden` into an
            // overflowing, page-long item after the flex algorithm correctly
            // resolved its automatic minimum to zero.
            // <https://www.w3.org/TR/css-overflow-3/#scrollable>
            // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
            let content_overflow = estimate.fragmentable_overflow_height.points().max(0.0);
            // The intrinsic content extent is already measured from the item's
            // border-box block start. Do not append its padding or border here:
            // the used box owns those decorations, and appending its block-end
            // edge would manufacture a later source continuation after descendant
            // overflow has been consumed.
            // <https://www.w3.org/TR/css-break-3/#box-splitting>
            item.set_fragmentation_height(PhysicalContentHeight::new(content_box_pt(
                content_overflow,
            )));
        }
        let decoration = FragmentDecoration::for_box_decoration_break(
            child.style.box_decoration_break,
            false,
            false,
        );
        if decoration.is_clone() {
            let borders = used_border_widths(&child.style);
            let reservation = FragmentDecorationReservation::new(
                decoration,
                non_content_pt(borders.top + child.style.padding.top),
                non_content_pt(child.style.padding.bottom + borders.bottom),
            );
            let source_height = (item.fragmentation_height().points()
                - reservation.block_start().points()
                - reservation.block_end().points())
            .max(0.0);
            item.configure_cloned_fragment_source(
                PhysicalContentHeight::new(content_box_pt(source_height)),
                reservation,
            );
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Re-measure an auto-height horizontal row item after flexible lengths
    /// have fixed its main size.
    ///
    /// The outer line must use this normal in-flow cross contribution when it
    /// establishes its hypothetical cross size. In particular, a stretched
    /// item does not receive its final used cross size until *after* that
    /// line-sizing step, so treating the remeasurement as fragmentation-only
    /// leaves later nested lines at the first line's block position.
    ///
    /// This is deliberately limited to horizontal rows. Other writing-mode
    /// and axis combinations have separate percentage-basis and line
    /// constraints that cannot safely reuse this physical-height
    /// remeasurement.
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
    #[allow(clippy::too_many_arguments)]
    pub(super) fn remeasure_auto_height_row_item_cross_contributions(
        &mut self,
        items: &[FlexItemLayout],
        estimates: &mut [FlexItemEstimate],
        children: &[StyledChild<'_>],
        container_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        physical_direction: FlexDirection,
        available: FlexAvailableSpace,
    ) {
        if !physical_direction.is_row_axis()
            || container_style.writing_mode != WritingMode::HorizontalTb
        {
            return;
        }

        for ((item, estimate), child) in items.iter().zip(estimates).zip(children) {
            if !child.style.box_values.height.is_auto() {
                continue;
            }

            let borders = used_border_widths(&child.style);
            let horizontal_non_content =
                child.style.padding.left + child.style.padding.right + borders.left + borders.right;
            let used_content_width = (item.width().points() - horizontal_non_content).max(0.0);
            let mut item_available = flex_item_estimate_available_space(
                &child.style,
                container_style,
                physical_direction,
                available,
            );
            item_available.set_definite_width(
                PhysicalContentWidth::new(content_box_pt(used_content_width)),
                FlexAvailableSizeSource::PostFlexingMainSize,
            );
            let remeasured = self.estimate_flex_item_size(
                child,
                stylesheets,
                item_available,
                physical_direction,
            );
            estimate.replace_row_normal_flow_cross_contribution_preserving_fragmentable_overflow(
                remeasured,
            );
        }
    }

    /// Re-measure nested row-flex items at their resolved main size for their
    /// fragmentable descendant extent.
    ///
    /// Flex base sizing may measure an auto-width nested flex container with
    /// an indefinite main-size basis. Once the outer algorithm has assigned a
    /// narrower used main size, its wrapping descendants can occupy more
    /// physical block space. The used outer cross size remains unchanged here;
    /// only the source extent used by CSS Fragmentation is refreshed.
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>
    #[allow(clippy::too_many_arguments)]
    pub(super) fn remeasure_nested_flex_fragmentable_overflow_extents(
        &mut self,
        states: &mut [FlexItemSizingState],
        children: &[StyledChild<'_>],
        container_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        physical_direction: FlexDirection,
        available: FlexAvailableSpace,
    ) {
        if !physical_direction.is_row_axis() {
            return;
        }
        for (state, child) in states.iter_mut().zip(children) {
            if !child.style.display.is_flex() || !child.style.box_values.height.is_auto() {
                continue;
            }
            let estimate = state.estimate();
            let item = state.allocation();
            let borders = used_border_widths(&child.style);
            let horizontal_non_content =
                child.style.padding.left + child.style.padding.right + borders.left + borders.right;
            let used_content_width = (item.width().points() - horizontal_non_content).max(0.0);
            if (used_content_width - estimate.width.points()).abs() <= 0.01 {
                continue;
            }
            let mut item_available = flex_item_estimate_available_space(
                &child.style,
                container_style,
                physical_direction,
                available,
            );
            item_available.set_definite_width(
                PhysicalContentWidth::new(content_box_pt(used_content_width)),
                FlexAvailableSizeSource::PostFlexingMainSize,
            );
            let remeasured = self.estimate_flex_item_size(
                child,
                stylesheets,
                item_available,
                physical_direction,
            );
            state
                .estimate_mut()
                .merge_fragmentable_overflow_height(remeasured.fragmentable_overflow_height);
        }
    }
}
