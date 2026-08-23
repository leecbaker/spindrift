use super::*;

/// The physical containing space supplied while measuring an automatic
/// positioned block axis. Keeping both axes together prevents a vertical
/// writing-mode measurement from accidentally replacing its logical inline
/// containing size with the unbounded horizontal block-axis sentinel.
/// <https://www.w3.org/TR/css-position-3/#abspos-layout>
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PositionedAutoBlockMeasurementSpace {
    pub(in crate::layout) content_width: PhysicalContentWidth,
    pub(in crate::layout) available_physical_height: PhysicalContentHeight,
}

/// The percentage-resolution boundary for the principal box of an automatic
/// positioned-block measurement.
///
/// The principal resolves its percentages against its absolute-position
/// containing block even while this probe determines an automatic size. Normal
/// block layout creates the later descendant boundary, where the principal's
/// automatic block size remains cyclic.
/// <https://drafts.csswg.org/css-position-3/#abspos-layout>
/// <https://drafts.csswg.org/css-sizing-3/#behaving-as-auto>
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
#[derive(Debug, Clone, Copy)]
struct PositionedAutoBlockMeasurementPrincipalContext {
    principal_block_size_basis: BlockSizePercentageBasis,
    child_available_space: ChildAvailableSpace,
}

impl PositionedAutoBlockMeasurementPrincipalContext {
    fn new(
        containing_writing_mode: WritingMode,
        space: PositionedAutoBlockMeasurementSpace,
    ) -> Self {
        Self {
            // CSS Positioned Layout resolves percentages on this principal
            // against the absolute-position containing block. The principal
            // formatting context then establishes its own descendant basis.
            principal_block_size_basis: PercentageBasis::definite_from(
                space.available_physical_height.content_box_length(),
                BlockSizeBasisSource::AbsolutePositioned,
            ),
            child_available_space: ChildAvailableSpace::new(
                containing_writing_mode,
                space.content_width,
                true,
                Some(space.available_physical_height),
                space.available_physical_height,
            ),
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn measure_auto_positioned_block_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        measurement_space: PositionedAutoBlockMeasurementSpace,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> ContentBoxLength {
        let vertical_border_width_for_positioning =
            self.positioned_vertical_border_width(element, style, stylesheets, table_fragment);
        let snapshot = self.snapshot();
        let principal_context = PositionedAutoBlockMeasurementPrincipalContext::new(
            self.containing_block_writing_mode,
            measurement_space,
        );
        // Flex and grid intrinsic sizing may recursively use this same
        // surrogate for an in-flow descendant. The snapshot below restores
        // the enclosing measurement mode when that nested probe completes.
        self.layout_pass_kind = LayoutPassKind::PositionedAutoSizeMeasurement;
        self.content_left = 0.0;
        self.content_right = measurement_space
            .content_width
            .points()
            .max(style.font_size);
        let start_page_index = self.pages.len();
        let start_page_context = self.current_page_context;
        self.cursor_y = self.page_top();
        // A horizontal auto-height measurement may traverse arbitrary page
        // fragments, so its synthetic block-axis extent remains effectively
        // unbounded. In a vertical writing mode, however, physical height is
        // the logical inline axis. It is the definite inline containing size
        // used to fit the positioned box's lines; replacing it with the
        // measurement sentinel would make a one-glyph abspos box stretch to
        // that sentinel instead of shrink-wrapping to its containing block.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        // <https://www.w3.org/TR/css-position-3/#abspos-layout>
        let measurement_height = if style.writing_mode.has_vertical_lines() {
            measurement_space
                .available_physical_height
                .points()
                .max(0.0)
        } else {
            10_000.0
        };
        self.containing_blocks
            .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                self.content_left,
                self.cursor_y,
                self.content_right - self.content_left,
                measurement_height,
            )));
        // Scope the established absolute-position containing block for the
        // principal. Entering its automatic formatting context creates the
        // separate descendant basis, preserving cyclic descendant percentages.
        self.definite_block_size_stack
            .push(principal_context.principal_block_size_basis);
        self.child_available_space_stack
            .push(principal_context.child_available_space);
        // Match final absolute-positioned replay: the box is an independent
        // block formatting context, so ambient source-float exclusions cannot
        // inflate its measured auto height.
        // <https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo>
        self.push_float_context();
        // Automatic abspos sizing measures the principal formatting context,
        // not the sequence of fragmentainers that its final layout may later
        // occupy. Suppress both forced and automatic fragmentation here so a
        // descendant `break-after` cannot turn page-area gaps into used
        // content height.
        // <https://drafts.csswg.org/css-position-3/#abspos-layout>
        self.fragmentation_suppression_depth += 1;
        self.layout_element_inner(
            element,
            style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
        );
        self.fragmentation_suppression_depth -= 1;
        self.pop_float_context();
        self.child_available_space_stack.pop();
        self.definite_block_size_stack.pop();
        self.containing_blocks.pop();
        let consumed = self
            .positioned_measurement_fragmented_block_extent(start_page_index, start_page_context);
        self.restore(snapshot);
        // CSS 2.2 absolute positioning equations use content height as the
        // `height` term and add padding/borders separately. Collapsed table
        // borders contribute resolved outer grid insets rather than authored
        // full border widths, so use the same vertical non-content size that
        // will be used by the absolute-position equation.
        content_box_pt(
            (consumed
                - style.padding.top
                - style.padding.bottom
                - vertical_border_width_for_positioning)
                .max(0.0),
        )
    }

    /// Returns the continuous block-axis extent traversed by a positioned
    /// auto-height measurement that crossed rollback-only fragmentainers.
    ///
    /// Descendant break controls still affect the final fragments of an
    /// absolutely positioned box. The measurement therefore retains their
    /// continuous geometric advance while the enclosing snapshot discards the
    /// temporary pages and all page-local output.
    /// <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
    /// <https://drafts.csswg.org/css-break-3/#break-between>
    fn positioned_measurement_fragmented_block_extent(
        &mut self,
        start_page_index: usize,
        start_page_context: PageContext,
    ) -> f32 {
        let completed_page_area_height = if self.pages.len() <= start_page_index {
            0.0
        } else {
            let later_page_sizes = self
                .pages
                .iter()
                .skip(start_page_index + 1)
                .map(|page| PageSize::from_points(page.width(), page.height()))
                .collect::<Vec<_>>();
            let mut height = start_page_context.area_height();
            for (offset, page_size) in later_page_sizes.into_iter().enumerate() {
                let page_index = start_page_index + offset + 1;
                height += self
                    .finished_page_context(page_index + 1, page_size)
                    .area_height();
            }
            height
        };

        completed_page_area_height + (self.page_top() - self.cursor_y).max(0.0)
    }

    pub(in crate::layout) fn positioned_vertical_border_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        if is_html_table_element(element) {
            self.collapsed_table_outer_vertical_insets(style, stylesheets, table_fragment)
                .map(NonContentLength::points)
                .unwrap_or_else(|| vertical_border_width(style))
        } else {
            vertical_border_width(style)
        }
    }

    pub(in crate::layout) fn page_containing_block(&self) -> ContainingBlock {
        ContainingBlock::from_page_top_rect(PageTopRect::new(
            self.page_left(),
            self.page_top(),
            self.page_area_width(),
            // Out-of-flow layout suppresses fragmentation while collecting
            // descendants, but it must not enlarge the initial containing
            // block. Its physical dimensions are always the current page
            // area's dimensions, including for percentage block sizes:
            // <https://www.w3.org/TR/css-display-3/#initial-containing-block>
            // and <https://www.w3.org/TR/css-position-3/#def-cb>.
            self.current_page_context.area_height(),
        ))
        // This is the initial containing block, whose rectangle is established
        // by the first page area. Explicit absolute offsets remain anchored to
        // it even when ordinary flow has generated later pages before this
        // out-of-flow element is collected. CSS Positioned Layout resolves the
        // box against that containing block as a continuous flow, then permits
        // printers latitude when its resolved position crosses pages. Any
        // WeasyPrint-style source-page anchoring must therefore be an explicit
        // compatibility policy, never an implicit replacement here. Auto static
        // positions retain their source fragment separately in
        // `layout_positioned_block`.
        // <https://www.w3.org/TR/css-page-3/#page-model>
        // <https://drafts.csswg.org/css-position-3/#fragmentation>
        .on_page(0)
    }

    pub(in crate::layout) fn current_containing_block(&self) -> ContainingBlock {
        self.containing_blocks
            .last()
            .cloned()
            .unwrap_or_else(|| self.page_containing_block())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measurement_space() -> PositionedAutoBlockMeasurementSpace {
        PositionedAutoBlockMeasurementSpace {
            content_width: PhysicalContentWidth::new(content_box_pt(320.0)),
            available_physical_height: PhysicalContentHeight::new(content_box_pt(180.0)),
        }
    }

    #[test]
    fn horizontal_principal_measurement_exports_absolute_containing_block_bases() {
        let context = PositionedAutoBlockMeasurementPrincipalContext::new(
            WritingMode::HorizontalTb,
            measurement_space(),
        );

        assert!(context.principal_block_size_basis.is_definite());
        assert_eq!(context.principal_block_size_basis.points(), Some(180.0));
        assert!(context.child_available_space.physical_width_is_definite);
        assert!(
            context
                .child_available_space
                .physical_height_percentage_basis()
                .is_definite()
        );
        assert_eq!(
            context
                .child_available_space
                .available_physical_height()
                .points(),
            180.0
        );
    }

    #[test]
    fn vertical_principal_measurement_projects_absolute_containing_block_bases() {
        let context = PositionedAutoBlockMeasurementPrincipalContext::new(
            WritingMode::HorizontalTb,
            measurement_space(),
        );

        assert_eq!(
            context.child_available_space.writing_mode,
            WritingMode::HorizontalTb
        );

        assert!(context.principal_block_size_basis.is_definite());
        assert!(context.child_available_space.physical_width_is_definite);
        assert_eq!(
            context
                .child_available_space
                .available_physical_height()
                .points(),
            180.0
        );
        assert_eq!(
            context
                .child_available_space
                .logical_inline_size_for(WritingMode::VerticalLr)
                .points(),
            180.0
        );
        assert!(
            context
                .child_available_space
                .physical_height_percentage_basis()
                .is_definite()
        );
        assert!(
            context
                .child_available_space
                .logical_inline_percentage_basis_for(WritingMode::VerticalLr)
                .is_definite()
        );
    }

    #[test]
    fn automatic_principal_establishes_an_indefinite_descendant_block_basis() {
        let context = PositionedAutoBlockMeasurementPrincipalContext::new(
            WritingMode::HorizontalTb,
            measurement_space(),
        );
        let principal = ComputedStyle::initial();
        let descendant_space = crate::layout::block::child_available_space_for_block(
            &principal,
            measurement_space().content_width,
            None,
            context.child_available_space.orthogonal_available_height,
            measurement_space().available_physical_height,
        );

        assert!(context.principal_block_size_basis.is_definite());
        assert!(
            !descendant_space
                .physical_height_percentage_basis()
                .is_definite()
        );
    }
}
