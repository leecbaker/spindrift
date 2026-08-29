use super::*;

/// How a replayed flex item's root formatting context obtains its descendant
/// percentage-height basis.
///
/// `Override(Indefinite)` is intentionally distinct from deriving a basis from
/// the temporary replayed style: Flexbox can assign a numeric used height
/// without making that height definite for percentage descendants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum FlexDescendantPercentageHeightBasis {
    DeriveFromContainer,
    Override(BlockSizePercentageBasis),
}

impl FlexDescendantPercentageHeightBasis {
    pub(in crate::layout::flex) fn available_height_basis(
        self,
        container_height: Option<ContentBoxLength>,
    ) -> FlexAvailablePercentageBasis {
        match self {
            Self::DeriveFromContainer => flex_available_percentage_basis(
                container_height,
                FlexAvailableSizeSource::ContainingBlock,
            ),
            Self::Override(basis) => basis.map_source(|_| FlexAvailableSizeSource::ContainingBlock),
        }
    }

    pub(in crate::layout::flex) fn override_basis(self) -> Option<BlockSizePercentageBasis> {
        match self {
            Self::DeriveFromContainer => None,
            Self::Override(basis) => Some(basis),
        }
    }
}

/// Input geometry for an abspos flex child's static-position calculation.
///
/// CSS Flexbox derives the static position of an absolutely positioned flex
pub(in crate::layout::flex) fn placed_flex_item_style(
    child_style: &ComputedStyle,
    item_width: BorderBoxLength,
    item_height: BorderBoxLength,
    physical_direction: PhysicalFlexDirection,
) -> ComputedStyle {
    // CSS used-value setters remain scalar legacy APIs. The flex layout
    // boundary nevertheless records the final dimensions as border-box
    // extents, so extract only after entering this style adapter.
    let item_border_box_width = item_width;
    let item_border_box_height = item_height;
    let mut placed_style =
        replayed_item_fragmentation_base_style(child_style, ReplayedItemFragmentationPolicy::Flex);
    let borders = used_border_widths(child_style);
    let horizontal_non_content =
        child_style.padding.left + child_style.padding.right + borders.left + borders.right;
    let vertical_non_content =
        child_style.padding.top + child_style.padding.bottom + borders.top + borders.bottom;
    let horizontal_content_size = border_box_to_content_box_length(
        item_border_box_width,
        non_content_pt(horizontal_non_content),
    );
    let vertical_content_size = border_box_to_content_box_length(
        item_border_box_height,
        non_content_pt(vertical_non_content),
    );
    // Taffy's final layout rectangle is a physical border box. Convert it
    // once, explicitly, to the content-box properties consumed by ordinary
    // block replay. This avoids relying on a later `box-sizing` interpretation
    // and keeps vertical auto block-size reconstruction from treating the
    // border box as its intrinsic content width.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-item-sizing>
    // <https://drafts.csswg.org/css-flexbox-1/#definite-sizes> and
    // <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>.
    let used_width = horizontal_content_size.points();
    let used_height = vertical_content_size.points();
    set_style_used_width(&mut placed_style, used_width);
    set_style_used_height(&mut placed_style, used_height);
    // Preserve the resolved main-axis bound while replaying the item. The
    // temporary formatting context must not reapply an authored min/max
    // percentage against a different containing block after Flexbox has
    // already resolved the item's used main size.
    // <https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths>
    if physical_direction.is_row_axis() {
        set_style_used_content_box_width_bounds(&mut placed_style, horizontal_content_size);
    } else {
        set_style_used_content_box_height_bounds(&mut placed_style, vertical_content_size);
    }
    placed_style.box_sizing = BoxSizing::ContentBox;
    placed_style
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::flex) fn layout_flex_item_contents(
        &mut self,
        child: &StyledChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        percentage_height_basis: FlexPercentageBasis,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) -> Option<InFlowFragmentEnd> {
        self.with_replayed_flex_item_percentage_height_basis(percentage_height_basis, |layout| {
            if child.style.display.is_table() {
                layout.layout_formatting_context_item_contents(
                    child,
                    placed_style,
                    stylesheets,
                    principal_box_paint_mode,
                );
                return layout.last_block_layout_outcome.in_flow_child_fragment_end;
            }
            let fragmentainer_kind = layout.active_fragmentainer_kind();
            let child_has_forced_fragment_break =
                child.element_parts().is_some_and(|(_, _, boxes)| {
                    boxes.is_some_and(|boxes| {
                        flex_item_contents_have_forced_break_in(boxes, fragmentainer_kind)
                    })
                });
            // The flex container owns fragmentation at flex-line/item
            // boundaries. Replaying a final, unsplit item through ordinary
            // block flow must therefore keep its descendants in the assigned
            // item fragment rather than letting a descendant manufacture an
            // independent page break before the item's used height is applied.
            // A forced descendant break is different: CSS Fragmentation must
            // consume it in the child's formatting context, even when the
            // flex item itself has not crossed a flex boundary. Its resulting
            // page fragments are then retained by the enclosing flex layout.
            // <https://drafts.csswg.org/css-flexbox-1/#pagination>
            if !child_has_forced_fragment_break {
                layout.fragmentation_suppression_depth += 1;
            }
            layout.layout_formatting_context_item_contents(
                child,
                placed_style,
                stylesheets,
                principal_box_paint_mode,
            );
            if !child_has_forced_fragment_break {
                layout.fragmentation_suppression_depth -= 1;
            }
            layout.last_block_layout_outcome.in_flow_child_fragment_end
        })
    }

    /// Lay out a nested continuation source for a split flex item.
    ///
    /// Unlike [`Self::layout_flex_item_contents`], this keeps child
    /// fragmentation enabled: the resulting local fragment sequence is
    /// committed by the flex item's replay record and later continuations
    /// select that sequence by ordinal. Suppressing fragmentation here would
    /// make every later flex slice reconstruct a single monolithic child tree.
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>
    pub(in crate::layout::flex) fn layout_split_flex_item_continuation_contents(
        &mut self,
        child: &StyledChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        percentage_height_basis: FlexPercentageBasis,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        self.with_replayed_flex_item_percentage_height_basis(percentage_height_basis, |layout| {
            layout.layout_formatting_context_item_contents(
                child,
                placed_style,
                stylesheets,
                principal_box_paint_mode,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendant_percentage_height_basis_preserves_override_definiteness() {
        assert_eq!(
            FlexDescendantPercentageHeightBasis::DeriveFromContainer
                .available_height_basis(Some(content_box_pt(24.0))),
            PercentageBasis::definite_from(
                content_box_pt(24.0),
                FlexAvailableSizeSource::ContainingBlock,
            )
        );

        let definite = FlexDescendantPercentageHeightBasis::Override(
            PercentageBasis::definite_from(content_box_pt(48.0), BlockSizeBasisSource::FlexItem),
        )
        .available_height_basis(None);
        assert_eq!(
            definite,
            PercentageBasis::definite_from(
                content_box_pt(48.0),
                FlexAvailableSizeSource::ContainingBlock,
            )
        );

        let indefinite =
            FlexDescendantPercentageHeightBasis::Override(PercentageBasis::indefinite())
                .available_height_basis(Some(content_box_pt(48.0)));
        assert_eq!(indefinite, PercentageBasis::indefinite());
    }

    #[test]
    fn placed_column_item_converts_taffy_border_box_height_to_content_box() {
        let mut child = ComputedStyle::initial();
        child.box_sizing = BoxSizing::ContentBox;
        child.padding.top = 2.0;
        child.padding.bottom = 3.0;

        let placed = placed_flex_item_style(
            &child,
            border_box_pt(40.0),
            border_box_pt(20.0),
            PhysicalFlexDirection::new(FlexDirection::ColumnReverse),
        );

        let height = used_length_percentage_or_auto_with_basis(
            placed.box_values.height.value().clone(),
            PercentageBasis::<ContentBoxLength>::indefinite(),
        )
        .expect("the replayed physical main size is definite");
        assert_eq!(height.points(), 15.0);
        assert_eq!(
            used_content_box_height_or_auto_with_basis(
                &placed,
                PercentageBasis::<ContentBoxLength>::indefinite(),
                non_content_pt(5.0),
            ),
            Some(content_box_pt(15.0)),
        );
    }

    #[test]
    fn placed_row_item_keeps_taffy_border_box_width() {
        let mut child = ComputedStyle::initial();
        child.box_sizing = BoxSizing::ContentBox;
        child.padding.left = 2.0;
        child.padding.right = 3.0;

        let placed = placed_flex_item_style(
            &child,
            border_box_pt(20.0),
            border_box_pt(40.0),
            PhysicalFlexDirection::new(FlexDirection::RowReverse),
        );

        let width = used_content_box_width_or_auto_with_basis(
            &placed,
            PercentageBasis::<ContentBoxLength>::indefinite(),
            non_content_pt(5.0),
        )
        .expect("the replayed physical main size is definite");
        assert_eq!(width, content_box_pt(15.0));
    }

    #[test]
    fn vertical_logical_row_projects_item_main_intervals_to_fragmentainers() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        for writing_mode in [
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            style.writing_mode = writing_mode;
            assert_eq!(
                FlexFragmentationBoundaryProjection::for_style(&style),
                FlexFragmentationBoundaryProjection::ItemMainAxis,
                "{writing_mode:?} logical row fragments along physical Y"
            );
        }

        style.writing_mode = WritingMode::HorizontalTb;
        assert_eq!(
            FlexFragmentationBoundaryProjection::for_style(&style),
            FlexFragmentationBoundaryProjection::LineCrossAxis,
        );
    }

    #[test]
    fn boundary_projection_returns_physical_fragmentainer_intervals() {
        let mut vertical_row = ComputedStyle::initial();
        vertical_row.writing_mode = WritingMode::VerticalRl;
        vertical_row.flex_direction = FlexDirection::Row;
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(40.0, 30.0),
            ContainerSize::new(20.0, 50.0),
        ));
        assert_eq!(
            FlexFragmentationBoundaryProjection::for_style(&vertical_row)
                .item_main_block_bounds(&item, false),
            FlexFragmentBlockBounds::new(
                FlexFragmentBlockOffset::new(30.0),
                FlexFragmentBlockOffset::new(80.0),
            ),
            "a vertical logical row fragments along its physical-Y main axis",
        );

        let horizontal_row = ComputedStyle::initial();
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(50.0),
            cross_start: FlexCrossOffset::new(30.0),
            cross_end: FlexCrossOffset::new(80.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        assert_eq!(
            FlexFragmentationBoundaryProjection::for_style(&horizontal_row)
                .line_cross_block_bounds(&line),
            FlexFragmentBlockBounds::new(
                FlexFragmentBlockOffset::new(30.0),
                FlexFragmentBlockOffset::new(80.0),
            ),
            "a horizontal physical row fragments along its physical-Y cross axis",
        );
    }

    #[test]
    fn intrinsic_flex_container_width_projects_to_physical_content_width() {
        let style = ComputedStyle::initial();
        let intrinsic = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(90.0)),
            PhysicalContentHeight::new(content_box_pt(20.0)),
        );

        let width = flex_container_content_width_from_intrinsic(
            &style,
            layout_pt(120.0),
            non_content_pt(20.0),
            intrinsic,
            false,
        );

        let _: PhysicalContentWidth = width;
        let _: ContentBoxLength = intrinsic.min_width;
        assert_eq!(width.points(), 100.0);
    }

    #[test]
    fn shrink_to_fit_compares_typed_outer_and_content_widths() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Column;
        style.flex_wrap = FlexWrap::Wrap;

        let width = flex_container_shrink_to_fit_max_content_width(
            &style,
            layout_pt(120.0),
            non_content_pt(20.0),
            PhysicalContentWidth::new(content_box_pt(30.0)),
            PhysicalContentWidth::new(content_box_pt(90.0)),
            true,
        );

        assert_eq!(width, PhysicalContentWidth::new(content_box_pt(30.0)));
    }
}
