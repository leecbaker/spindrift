use super::*;
use crate::layout::flex::alignment::effective_align_self;

/// Treat auto margins as zero for an abspos flex static-position probe.
///
/// Absolutely positioned flex children do not participate in flex layout, but
/// Flexbox defines their static-position rectangle by laying out a
/// hypothetical sole flex item:
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
pub(in crate::layout::flex) fn zero_auto_margins_for_static_flex_probe(style: &mut ComputedStyle) {
    let zero = css::ComputedLengthPercentageOrAuto::LengthPercentage(
        css::ComputedLengthPercentage::from_points(0.0),
    );
    if style.box_values.margin.left.is_auto() {
        style.box_values.margin.left = zero.clone();
        style.margin.left = 0.0;
    }
    if style.box_values.margin.right.is_auto() {
        style.box_values.margin.right = zero.clone();
        style.margin.right = 0.0;
    }
    if style.box_values.margin.top.is_auto() {
        style.box_values.margin.top = zero.clone();
        style.margin.top = 0.0;
    }
    if style.box_values.margin.bottom.is_auto() {
        style.box_values.margin.bottom = zero;
        style.margin.bottom = 0.0;
    }
}

/// Resolve distributed `justify-content` values for the hypothetical sole
/// flex item used to establish an absolutely positioned child's static
/// rectangle.
///
/// The static-position algorithm lays out exactly one hypothetical item.
/// CSS Box Alignment's fallback alignment for that item maps `space-between`
/// and `stretch` to start, and `space-around` and `space-evenly` to center.
/// Resolve that fallback before crossing the Taffy adapter, whose distributed
/// alignment does not model this flex static-position special case.
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
/// <https://www.w3.org/TR/css-align-3/#distribution-fallback>
pub(in crate::layout::flex) fn resolve_static_flex_probe_justify_content(
    style: &mut ComputedStyle,
) {
    style.justify_content.keyword = match style.justify_content.keyword {
        css::ContentAlignmentKeyword::Stretch | css::ContentAlignmentKeyword::SpaceBetween => {
            css::ContentAlignmentKeyword::FlexStart
        }
        css::ContentAlignmentKeyword::SpaceAround | css::ContentAlignmentKeyword::SpaceEvenly => {
            css::ContentAlignmentKeyword::Center
        }
        keyword => keyword,
    };
}
/// Input geometry for an abspos flex child's static-position calculation.
///
/// CSS Flexbox derives the static position of an absolutely positioned flex
/// child from the flex container's content box and hypothetical sole-item flex
/// placement:
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
pub(in crate::layout::flex) struct PositionedFlexStaticContext<'a> {
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) stylesheets: &'a Stylesheets<'a>,
    pub(in crate::layout::flex) available: FlexAvailableSpace,
    /// Physical page-inline span of the flex container's content box.
    pub(in crate::layout::flex) inner_inline_span: PageInlineSpan,
    /// Used physical content-box height of the flex container. This remains
    /// available even when the height was not a definite percentage basis for
    /// the flex sizing algorithm, because the abspos static rectangle uses
    /// the final cross-axis content edges.
    pub(in crate::layout::flex) content_height: PhysicalContentHeight,
    pub(in crate::layout::flex) content_top: PageTopBlockPosition,
    /// Source block offset of the temporary fragmentainer currently owning a
    /// deferred positioned child. Static flex geometry is initially expressed
    /// in the unfragmented flex source coordinate system, so it must be
    /// localized before multicolumn projection chooses its destination.
    pub(in crate::layout::flex) source_fragment_block_offset: FlexFragmentBlockOffset,
    /// Source block capacity of the first committed flex fragmentainer.
    /// A definite physical `top` inside this range remains in the original
    /// source fragment; only a later inset needs candidate projection.
    pub(in crate::layout::flex) first_fragment_source_block_size: FlexFragmentBlockSize,
}

/// Convert Flexbox's main-axis content alignment into the equivalent
/// self-alignment used to retain an abspos child's static-position rectangle.
/// Distributed values use the one-item fallback required by Flexbox.
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
/// <https://drafts.csswg.org/css-align-3/#distribution-fallback>
fn flex_static_main_alignment(alignment: css::ContentAlignment) -> css::SelfAlignment {
    let keyword = match alignment.keyword {
        css::ContentAlignmentKeyword::Normal
        | css::ContentAlignmentKeyword::Start
        | css::ContentAlignmentKeyword::FlexStart
        | css::ContentAlignmentKeyword::Stretch
        | css::ContentAlignmentKeyword::SpaceBetween
        | css::ContentAlignmentKeyword::Baseline
        | css::ContentAlignmentKeyword::LastBaseline => SelfAlignmentKeyword::Start,
        css::ContentAlignmentKeyword::End | css::ContentAlignmentKeyword::FlexEnd => {
            SelfAlignmentKeyword::End
        }
        css::ContentAlignmentKeyword::Left => SelfAlignmentKeyword::Left,
        css::ContentAlignmentKeyword::Right => SelfAlignmentKeyword::Right,
        css::ContentAlignmentKeyword::Center
        | css::ContentAlignmentKeyword::SpaceAround
        | css::ContentAlignmentKeyword::SpaceEvenly => SelfAlignmentKeyword::Center,
    };
    css::SelfAlignment {
        keyword,
        safety: alignment.safety,
    }
}

/// Final static-position geometry for an absolutely positioned flex child.
///
/// Flexbox first places a hypothetical sole item, but CSS Positioned Layout
/// subsequently resolves the real abspos box against its actual containing
/// block. Keep those coordinate systems separate: the hypothetical margin box
/// supplies only the flex main-axis edges, while the flex content box supplies
/// the cross-axis edges of the static-position rectangle.
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
/// <https://drafts.csswg.org/css-position-3/#static-position-rectangle>
#[derive(Debug, Clone, Copy)]
struct FlexAbsposStaticGeometry {
    flex_content_box: PageTopRect,
    hypothetical_margin_box: PageTopRect,
    flex_axes: FlexAxes,
    inline_alignment: css::SelfAlignment,
    block_alignment: css::SelfAlignment,
    container_writing_mode: WritingMode,
    container_direction: Direction,
    subject_writing_mode: WritingMode,
    subject_direction: Direction,
    source_block_interval: (LayoutLength, LayoutLength),
}

impl FlexAbsposStaticGeometry {
    /// Compose the static-position alignment container from the two distinct
    /// geometry sources required by Flexbox.
    fn static_area(self) -> PageTopRect {
        if self.flex_axes.is_main_row_axis() {
            PageTopRect::new(
                self.hypothetical_margin_box.x(),
                self.flex_content_box.top_y(),
                self.hypothetical_margin_box.width(),
                self.flex_content_box.height(),
            )
        } else {
            PageTopRect::new(
                self.flex_content_box.x(),
                self.hypothetical_margin_box.top_y(),
                self.flex_content_box.width(),
                self.hypothetical_margin_box.height(),
            )
        }
    }

    /// Project the flex-owned static area into the generic positioned-layout
    /// handoff. `PositionedChildStaticRect` intentionally carries no actual
    /// containing-block override here; positioned layout retains the real
    /// ancestor containing block when resolving insets and safe overflow.
    fn positioned_static_rect(self) -> PositionedChildStaticRect {
        let area = self.static_area();
        PositionedChildStaticRect::new(area.x(), area.x() + area.width(), area.top_y())
            .with_static_alignment(AbsposStaticAlignment::new(
                area,
                self.container_writing_mode,
                self.container_direction,
                self.subject_writing_mode,
                self.subject_direction,
                self.inline_alignment,
                self.block_alignment,
            ))
    }

    fn source_block_interval(self) -> (LayoutLength, LayoutLength) {
        self.source_block_interval
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod flex_abspos_static_geometry_tests {
    use super::*;

    fn geometry(direction: FlexDirection) -> FlexAbsposStaticGeometry {
        let mut container_style = ComputedStyle::initial();
        container_style.flex_direction = direction;
        FlexAbsposStaticGeometry {
            flex_content_box: PageTopRect::new(100.0, 500.0, 200.0, 80.0),
            hypothetical_margin_box: PageTopRect::new(130.0, 470.0, 40.0, 20.0),
            flex_axes: FlexAxes::for_style(&container_style),
            inline_alignment: css::SelfAlignment::NORMAL,
            block_alignment: css::SelfAlignment::NORMAL,
            container_writing_mode: WritingMode::HorizontalTb,
            container_direction: Direction::Ltr,
            subject_writing_mode: WritingMode::HorizontalTb,
            subject_direction: Direction::Ltr,
            source_block_interval: (layout_pt(12.0), layout_pt(32.0)),
        }
    }

    #[test]
    fn row_static_area_uses_hypothetical_main_interval_and_content_cross_edges() {
        let geometry = geometry(FlexDirection::Row);
        let area = geometry.static_area();
        assert_eq!(
            (area.x(), area.top_y(), area.width(), area.height()),
            (130.0, 500.0, 40.0, 80.0)
        );

        let _static_rect = geometry.positioned_static_rect();
    }

    #[test]
    fn column_static_area_uses_content_cross_edges_and_hypothetical_main_interval() {
        let geometry = geometry(FlexDirection::Column);
        let area = geometry.static_area();
        assert_eq!(
            (area.x(), area.top_y(), area.width(), area.height()),
            (100.0, 470.0, 200.0, 20.0)
        );

        let _static_rect = geometry.positioned_static_rect();
        assert_eq!(
            geometry.source_block_interval(),
            (layout_pt(12.0), layout_pt(32.0))
        );
    }

    #[test]
    fn reverse_directions_preserve_their_physical_main_axis() {
        let row_reverse = geometry(FlexDirection::RowReverse).static_area();
        assert_eq!(
            (
                row_reverse.x(),
                row_reverse.top_y(),
                row_reverse.width(),
                row_reverse.height()
            ),
            (130.0, 500.0, 40.0, 80.0)
        );

        let column_reverse = geometry(FlexDirection::ColumnReverse).static_area();
        assert_eq!(
            (
                column_reverse.x(),
                column_reverse.top_y(),
                column_reverse.width(),
                column_reverse.height()
            ),
            (100.0, 470.0, 200.0, 20.0)
        );
    }

    #[test]
    fn vertical_rtl_static_geometry_keeps_the_flex_area_in_physical_coordinates() {
        let mut geometry = geometry(FlexDirection::Row);
        geometry.container_writing_mode = WritingMode::VerticalRl;
        geometry.container_direction = Direction::Rtl;
        geometry.subject_writing_mode = WritingMode::VerticalRl;
        geometry.subject_direction = Direction::Rtl;

        let area = geometry.static_area();
        assert_eq!(
            (area.x(), area.top_y(), area.width(), area.height()),
            (130.0, 500.0, 40.0, 80.0)
        );
        let _ = geometry.positioned_static_rect();
    }
}

/// Record the visible block-end of one committed nested table fragment.
///
/// Flex owns the outer source range, but a table's repeated chrome can make
/// its final child fragment shorter than that range. The following flex sibling
/// resumes after the visible child fragment rather than after unused scratch
/// fragmentainer capacity.
/// <https://www.w3.org/TR/css-break-3/#box-splitting>
impl<'a> LayoutBuilder<'a> {
    /// Lays out an absolutely positioned flex child from its flex static position.
    ///
    /// CSS Flexbox says an absolutely positioned child of a flex container does
    /// not participate in flex layout, but its static-position rectangle is
    /// derived from where it would be positioned as the sole flex item:
    /// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
    pub(in crate::layout::flex) fn layout_positioned_flex_child(
        &mut self,
        child: &StyledChild<'_>,
        context: PositionedFlexStaticContext<'_>,
    ) {
        let (static_rect, source_static_block_interval) =
            self.positioned_flex_child_static_rect(child, &context);
        if self.multicol_positioned_replay_capture_depth > 0 {
            // Only a physical-column flex progression encodes the
            // hypothetical item's main-axis static position in the same
            // physical-Y source coordinate that multicolumn fragmentation
            // slices. A physical row's static Y is cross-axis geometry and
            // already belongs to the local source fragmentainer.
            // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
            let positioning_containing_block =
                PositionedContainingBlockMode::for_style(context.container_style)
                    .zip(self.containing_blocks.last().copied());
            let fragment =
                PositionedFragmentReplay::unfragmented(static_rect, positioning_containing_block);
            // A physical-column flex child has a main-axis static position
            // in the same source block coordinate as the materialized flex
            // fragments, so its owner is known at capture time. For a
            // physical row, the static rectangle only describes cross-axis
            // placement; a definite inset can select a different source
            // fragment. Leave that record unresolved until positioned layout
            // has its final geometry instead of guessing the last temporary
            // multicolumn fragmentainer.
            // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            let physical_direction = physical_flex_direction(context.container_style);
            let final_block_inset_from_start = positioning_containing_block
                .and_then(|(_, containing_block)| used_inset_top(&child.style, containing_block))
                .or_else(|| child.style.box_values.inset_top.length_if_no_percent())
                .map(layout_pt);
            let final_block_inset_starts_later_fragment =
                final_block_inset_from_start.is_some_and(|inset| {
                    inset.points() > context.first_fragment_source_block_size.points() - 0.01
                });
            let fragment = if physical_direction.is_column_axis() {
                // Candidate selection must use the resolved physical block
                // interval when a definite inset moves the box away from its
                // hypothetical flex static position. The static rectangle
                // remains the positioned-layout fallback, but its source
                // interval no longer describes the painted box.
                // <https://www.w3.org/TR/css-position-3/#inset-properties>
                // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
                let source_block_interval = final_block_inset_from_start
                    .map(|inset| {
                        let source_size =
                            source_static_block_interval.1 - source_static_block_interval.0;
                        (inset, inset + source_size)
                    })
                    .unwrap_or(source_static_block_interval);
                let fragment = fragment
                    .with_source_fragment_block_offset(layout_pt(
                        context.source_fragment_block_offset.points(),
                    ))
                    .resolving_owner_from_source_block_interval(
                        source_block_interval.0,
                        source_block_interval.1,
                    );
                if final_block_inset_from_start.is_some() {
                    fragment.with_definite_block_inset_source_coordinates()
                } else {
                    fragment
                }
            } else if final_block_inset_starts_later_fragment {
                fragment.resolving_owner_from_final_block_inset(final_block_inset_from_start)
            } else {
                fragment
            };
            self.defer_multicol_positioned_fragment_child(child, fragment);
            return;
        }
        self.layout_positioned_formatting_context_child(child, context.stylesheets, static_rect);
    }

    /// Compute a flex positioned child's static rectangle before choosing
    /// whether normal positioned layout happens immediately or is deferred by
    /// an enclosing temporary multicolumn fragmentainer sequence.
    fn positioned_flex_child_static_rect(
        &mut self,
        child: &StyledChild<'_>,
        context: &PositionedFlexStaticContext<'_>,
    ) -> (PositionedChildStaticRect, (LayoutLength, LayoutLength)) {
        let mut hypothetical_child = child.clone();
        hypothetical_child.style.position = Position::Static;
        hypothetical_child.style.flex_grow = css::FlexGrowFactor::ZERO;
        hypothetical_child.style.flex_shrink = css::FlexShrinkFactor::ZERO;
        hypothetical_child.style.flex_basis = css::ComputedFlexBasis::Auto;
        zero_auto_margins_for_static_flex_probe(&mut hypothetical_child.style);
        // The hypothetical sole item resolves auto margins to zero. Retain
        // these used probe edges so the outer rectangle and its fragment
        // interval cannot accidentally mix probe geometry with the eventual
        // positioned child's authored margins.
        let probe_margins = hypothetical_child.style.margin;
        if hypothetical_child.style.display.is_inline_level() {
            hypothetical_child.style.display = hypothetical_child.style.display.blockified();
        }
        let mut hypothetical_container_style = context.container_style.clone();
        resolve_static_flex_probe_justify_content(&mut hypothetical_container_style);
        let hypothetical = self
            .compute_flex_layout(
                std::slice::from_ref(&hypothetical_child),
                &hypothetical_container_style,
                context.stylesheets,
                context.available,
            )
            .and_then(|layout| layout.items.into_iter().next())
            .unwrap_or_else(|| {
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(context.inner_inline_span.width(), child.style.line_height),
                ))
            });

        // CSS Flexbox aligns the hypothetical sole item by its margin box.
        // The static-position rectangle must therefore preserve those outer
        // bounds; starting from the border box shifts an abspos child by its
        // own margins a second time when normal positioned layout resolves
        // the final margin box.
        // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
        let static_left =
            context.inner_inline_span.left_x() + hypothetical.x().points() - probe_margins.left;
        let static_right = context.inner_inline_span.left_x()
            + hypothetical.x().points()
            + hypothetical.width().points()
            + probe_margins.right;
        let static_top =
            context.content_top.points() - hypothetical.y().points() + probe_margins.top;
        let flex_axes = FlexAxes::for_style(context.container_style);
        let main_alignment =
            flex_static_main_alignment(hypothetical_container_style.justify_content);
        let cross_alignment = effective_align_self(&child.style, context.container_style);
        let container_axes = WritingModeAxes::new(
            context.container_style.writing_mode,
            context.container_style.used_direction(),
        );
        let main_is_inline = if flex_axes.is_main_row_axis() {
            !container_axes.swaps_physical_axes()
        } else {
            container_axes.swaps_physical_axes()
        };
        let (inline_alignment, block_alignment) = if main_is_inline {
            (main_alignment, cross_alignment)
        } else {
            (cross_alignment, main_alignment)
        };
        let hypothetical_outer_height =
            (hypothetical.height().points() + probe_margins.top + probe_margins.bottom).max(0.0);
        // Flexbox gives the static-position rectangle the container's content
        // edges in the cross axis, while the sole hypothetical item's margin
        // edges determine it in the main axis. This distinction is essential
        // when the eventual abspos size differs from the sizing probe.
        // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
        let source_static_block_start = layout_pt(hypothetical.y().points().max(0.0));
        let source_static_block_end = layout_pt(
            (hypothetical.y().points()
                + hypothetical.height().points()
                + probe_margins.top
                + probe_margins.bottom)
                .max(source_static_block_start.points()),
        );
        let geometry = FlexAbsposStaticGeometry {
            flex_content_box: PageTopRect::new(
                context.inner_inline_span.left_x(),
                context.content_top.points(),
                context.inner_inline_span.width(),
                context.content_height.points(),
            ),
            hypothetical_margin_box: PageTopRect::new(
                static_left,
                static_top,
                (static_right - static_left).max(0.0),
                hypothetical_outer_height,
            ),
            flex_axes,
            inline_alignment,
            block_alignment,
            container_writing_mode: context.container_style.writing_mode,
            container_direction: context.container_style.used_direction(),
            subject_writing_mode: child.style.writing_mode,
            subject_direction: child.style.used_direction(),
            source_block_interval: (source_static_block_start, source_static_block_end),
        };
        (
            geometry.positioned_static_rect(),
            geometry.source_block_interval(),
        )
    }
}
