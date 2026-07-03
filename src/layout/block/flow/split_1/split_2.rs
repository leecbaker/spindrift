use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn translate_aligned_block_descendant_bookmarks(
        &mut self,
        descendant_bookmark_start: usize,
        page_index: usize,
        x_offset: f32,
        y_offset: f32,
    ) {
        if x_offset.abs() <= 0.01 && y_offset.abs() <= 0.01 {
            return;
        }
        for bookmark in self.bookmarks.iter_mut().skip(descendant_bookmark_start) {
            if bookmark.page_index == page_index {
                bookmark.translate_target(x_offset, y_offset);
            }
        }
    }

    /// Resolve a block box's used content width, including intrinsic keywords.
    ///
    /// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
    /// intrinsic sizing keywords for the `width` property. Normal block
    /// width-auto handling still follows CSS 2.2, but intrinsic keywords need
    /// the box contents before they can be converted to a used content width:
    /// <https://www.w3.org/TR/css-sizing-3/#sizing-values> and
    /// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
    pub(in crate::layout) fn used_block_content_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        width_inputs: BlockContentWidthInputs,
    ) -> ContentBoxLength {
        let needs_intrinsic = matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        );
        if !needs_intrinsic {
            return used_normal_flow_block_content_box_width(
                style,
                width_inputs.percentage_basis,
                width_inputs.horizontal_non_content,
            );
        }

        let (min_content, max_content) = self.block_intrinsic_content_widths(
            element,
            style,
            stylesheets,
            child_boxes,
            width_inputs.available_outer_width,
        );
        content_box_pt(intrinsic::content_width_from_intrinsic(
            style,
            width_inputs.available_outer_width,
            width_inputs.horizontal_non_content.points(),
            min_content,
            max_content,
            intrinsic::IntrinsicAutoWidth::FillAvailable,
        ))
    }

    /// Estimate block min-content and max-content content-box inline sizes.
    ///
    /// CSS Sizing computes intrinsic contributions from text soft-wrap
    /// opportunities and descendant intrinsic widths. This helper covers the
    /// normal block text paths used by block layout and falls back to the
    /// existing shrink-to-fit estimator for non-inline descendants until block
    /// intrinsic sizing is fully structured across every formatting context:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic>.
    pub(in crate::layout) fn block_intrinsic_content_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        if style.display.is_flex() {
            return self.estimate_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                available_outer_width,
                child_boxes,
            );
        }
        if style.display.is_grid() {
            return self.estimate_grid_intrinsic_widths(
                element,
                style,
                stylesheets,
                available_outer_width,
                child_boxes,
            );
        }
        let contribution = self.intrinsic_inline_contribution_for_element(
            element,
            style,
            stylesheets,
            child_boxes,
        );
        if contribution.max_content > 0.0 || contribution.min_content > 0.0 {
            return (contribution.min_content, contribution.max_content);
        }
        let shrink_to_fit = self.estimate_shrink_to_fit_width(
            element,
            style,
            stylesheets,
            available_outer_width,
            child_boxes,
            None,
        );
        (shrink_to_fit, shrink_to_fit)
    }

    pub(in crate::layout) fn block_layout_geometry(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> BlockLayoutGeometry {
        self.block_layout_geometry_in_inline_span(
            element,
            style,
            stylesheets,
            child_boxes,
            BlockLayoutInlineConstraint {
                containing_left: self.content_left,
                containing_right: self.content_right,
                percentage_basis: (self.content_right - self.content_left).max(0.0),
                auto_border_box_width: None,
            },
        )
    }

    pub(in crate::layout) fn block_layout_geometry_in_inline_span(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        constraint: BlockLayoutInlineConstraint,
    ) -> BlockLayoutGeometry {
        let containing_left = constraint.containing_left;
        let containing_right = constraint.containing_right;
        let percentage_basis = constraint.percentage_basis;
        let containing_inline_size = (containing_right - containing_left).max(0.0);
        let mut used_style = self.style_with_current_used_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, percentage_basis);
        let relative_offset =
            relative_position_offset(&used_style, self.current_containing_block());
        let available_outer_width =
            normal_flow_block_available_outer_width(&used_style, containing_inline_size);
        let border_widths = box_metrics.border;
        let horizontal_extras = non_content_pt(box_metrics.horizontal_non_content());
        let vertical_extras = box_metrics.vertical_non_content();
        let requested_content_width = if let Some(auto_border_box_width) = constraint
            .auto_border_box_width
            .filter(|_| has_auto_width(&used_style))
        {
            content_box_pt((auto_border_box_width - horizontal_extras.points()).max(0.0))
        } else {
            self.used_block_content_width(
                element,
                &used_style,
                stylesheets,
                child_boxes,
                BlockContentWidthInputs {
                    available_outer_width,
                    percentage_basis,
                    horizontal_non_content: horizontal_extras,
                },
            )
        };
        let width = resolve_normal_flow_block_width(
            &mut used_style,
            containing_left,
            containing_right,
            requested_content_width,
            horizontal_extras,
            self.containing_block_direction,
            true,
        );
        let content_width = width.content_width;
        let content_width_points = content_width.points();
        let containing_block_content_height =
            self.definite_block_size_stack.last().copied().flatten();
        let definite_content_height = used_content_height_or_auto_with_optional_basis(
            &used_style,
            containing_block_content_height,
            vertical_extras,
        )
        .map(|height| constrain_height(&used_style, height, content_width_points));
        let content_logical_inline_size = self.block_content_logical_inline_size(
            element,
            &used_style,
            stylesheets,
            child_boxes,
            content_width_points,
            definite_content_height,
        );
        let outer_width = width.border_box_width.points();
        let outer_x = width.border_box_x + relative_offset.x;
        let inner_x = outer_x + border_widths.left + used_style.padding.left;

        BlockLayoutGeometry {
            style: used_style,
            relative_offset,
            border_widths,
            vertical_extras,
            definite_content_height,
            content_logical_inline_size,
            outer_inline: BlockInlineBounds::new(outer_x, outer_width),
            content_inline: BlockInlineBounds::new(inner_x, content_width_points),
        }
    }

    /// Resolve the logical inline content size used by this block's inline layout.
    ///
    /// CSS Writing Modes defines orthogonal-flow auto inline sizing as a
    /// fit-content calculation against the containing block's available size.
    /// In vertical writing modes the logical inline axis is the physical
    /// height, so normal block layout must not reuse the physical content
    /// width as the text wrapping measure:
    /// <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto>.
    pub(in crate::layout) fn block_content_logical_inline_size(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        content_width: f32,
        definite_content_height: Option<f32>,
    ) -> f32 {
        match style.writing_mode {
            WritingMode::HorizontalTb => content_width.max(1.0),
            WritingMode::VerticalRl | WritingMode::VerticalLr => {
                let containing_space = self.current_child_available_space();
                if !writing_modes_are_orthogonal(containing_space.writing_mode, style.writing_mode)
                {
                    return content_width.max(1.0);
                }
                definite_content_height
                    .unwrap_or_else(|| {
                        let stretch_fit =
                            containing_space.logical_inline_size_for(style.writing_mode);
                        let contribution = self.intrinsic_inline_contribution_for_element(
                            element,
                            style,
                            stylesheets,
                            child_boxes,
                        );
                        contribution
                            .max_content
                            .min(contribution.min_content.max(stretch_fit))
                    })
                    .max(1.0)
            }
        }
    }
}
