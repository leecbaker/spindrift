use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockIntrinsicContentSizes {
    min_inline: LogicalInlineContentSize,
    max_inline: LogicalInlineContentSize,
    min_block: LogicalBlockContentSize,
    max_block: LogicalBlockContentSize,
    /// Block-axis size after laying out with the available max-content inline
    /// size. In an orthogonal flow this is the physical-width contribution;
    /// it is not ordered with the block size after min-content wrapping.
    block_size_at_max_inline: LogicalBlockContentSize,
}

/// A physical-width result together with the orthogonal inline measure that
/// selected it.
///
/// A vertical auto-width block first negotiates its logical inline measure to
/// determine its physical block contribution. The final inline pass must use
/// that exact same measure rather than collecting and fitting the same text a
/// second time.
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
#[derive(Debug, Clone, Copy)]
struct ResolvedBlockPhysicalContentWidth {
    content_width: PhysicalContentWidth,
    selected_logical_inline_size: Option<LogicalInlineContentSize>,
}

impl ResolvedBlockPhysicalContentWidth {
    fn ordinary(content_width: PhysicalContentWidth) -> Self {
        Self {
            content_width,
            selected_logical_inline_size: None,
        }
    }
}

impl BlockIntrinsicContentSizes {
    pub(in crate::layout) fn physical_width_min_max(
        self,
        axes: FlowAxes,
    ) -> (ContentBoxLength, ContentBoxLength) {
        if axes.writing_mode().has_vertical_lines() {
            // A vertical formatting context's logical block axis is physical
            // width. Its cross-size is selected after the inline content has
            // its max-content available space; using the block extent after
            // min-content wrapping would turn each wrapped line into a new
            // physical column and overstate an auto float's width.
            // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
            let width = self.block_size_at_max_inline.content_box_length();
            (width, width)
        } else {
            (
                axes.physical_width_from_logical_content_sizes(self.min_inline, self.min_block)
                    .content_box_length(),
                axes.physical_width_from_logical_content_sizes(self.max_inline, self.max_block)
                    .content_box_length(),
            )
        }
    }
}

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

    /// Resolve a block box's physical used content width, including intrinsic keywords.
    ///
    /// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
    /// intrinsic sizing keywords. CSS Writing Modes keeps `width` physical
    /// while applying sizing algorithms in logical axes, so vertical writing
    /// modes resolve physical width from logical block-size contributions:
    /// <https://www.w3.org/TR/css-sizing-3/#sizing-values>,
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>, and
    /// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
    pub(in crate::layout) fn used_block_physical_content_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        width_inputs: BlockContentWidthInputs,
    ) -> PhysicalContentWidth {
        self.resolved_block_physical_content_width(
            element,
            style,
            stylesheets,
            child_boxes,
            width_inputs,
        )
        .content_width
    }

    fn resolved_block_physical_content_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        width_inputs: BlockContentWidthInputs,
    ) -> ResolvedBlockPhysicalContentWidth {
        // In vertical writing modes, physical `width` maps to logical
        // block-size. An orthogonal root with an automatic block-size must
        // first select its fit-content inline size, then measure its wrapped
        // block contribution at that used line measure. Measuring only at
        // max-content inline size loses the extra line columns and lets the
        // root stretch across its containing block.
        // <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto>
        let vertical_auto_block_size =
            style.writing_mode.has_vertical_lines() && style.box_values.width.is_auto();
        // A horizontal child of a vertical formatting context is likewise an
        // orthogonal flow root. Its physical `width` is its own inline size,
        // but it occupies the parent's logical block axis; `width:auto` must
        // therefore use its intrinsic fit-content contribution rather than
        // the horizontal block model's fill-available default.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let horizontal_orthogonal_auto_inline_size = !style.writing_mode.has_vertical_lines()
            && self.containing_block_writing_mode.has_vertical_lines()
            && style.box_values.width.is_auto();
        if vertical_auto_block_size {
            if element.tag.eq_ignore_ascii_case("html") {
                // The root principal box is sized by the initial containing
                // block. Its propagated vertical writing mode changes the
                // logical axes used for layout, but does not turn the
                // viewport-sized document canvas into a shrink-to-fit box.
                //
                // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
                // <https://www.w3.org/TR/css-display-3/#initial-containing-block>
                return ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                    content_box_pt(
                        (width_inputs.available_outer_width.points()
                            - width_inputs.horizontal_non_content.points())
                        .max(0.0),
                    ),
                ));
            }
            if style.display.is_grid() {
                // A vertical grid's physical width is its logical block
                // size. Its track algorithm already computes that physical
                // contribution from the items; treating it as an ordinary
                // block stack instead measures a synthetic page-axis height
                // and expands the grid past its intrinsic tracks.
                // <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes>
                // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
                let (min_content, max_content) = self.estimate_grid_intrinsic_block_sizes(
                    element,
                    style,
                    stylesheets,
                    width_inputs.available_outer_width.points(),
                    child_boxes,
                );
                let content_width = crate::layout::intrinsic::content_box_width_from_intrinsic(
                    style,
                    width_inputs.available_outer_width,
                    width_inputs.horizontal_non_content,
                    content_box_pt(min_content),
                    content_box_pt(max_content),
                    crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                );
                return ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                    constrain_width_with_intrinsic(
                        style,
                        content_width,
                        content_box_pt(min_content),
                        content_box_pt(max_content),
                        width_inputs
                            .percentage_basis
                            .map_value(|basis| content_box_pt(basis.points())),
                        width_inputs.horizontal_non_content,
                    ),
                ));
            }
            // The auto physical width is the logical block contribution at
            // this box's *used* logical inline measure.  That measure is the
            // same fit-content negotiation used by final inline layout,
            // including an initial-containing-block fallback.  Measuring at
            // max-content here while final layout wraps at the ICB produces
            // columns outside the auto physical width.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
            let inline_size = width_inputs
                .definite_content_height
                .map(|height| LogicalInlineContentSize::new(height.content_box_length()))
                .unwrap_or_else(|| {
                    self.block_content_logical_inline_size(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        PhysicalContentWidth::new(content_box_pt(0.0)),
                        None,
                    )
                });
            let items =
                self.intrinsic_inline_items_for_element(element, style, stylesheets, child_boxes);
            let items_are_empty = items.is_empty();
            let block_size = if items_are_empty {
                self.estimate_block_child_intrinsic_logical_block_size(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    width_inputs.available_outer_width.points(),
                )
            } else {
                self.inline_items_logical_block_size(items, style, inline_size.points())
            };
            let content_width = content_box_pt(block_size.max(0.0));
            return ResolvedBlockPhysicalContentWidth {
                content_width: PhysicalContentWidth::new(constrain_width_with_intrinsic(
                    style,
                    content_width,
                    content_width,
                    content_width,
                    width_inputs
                        .percentage_basis
                        .map_value(|basis| content_box_pt(basis.points())),
                    width_inputs.horizontal_non_content,
                )),
                selected_logical_inline_size: width_inputs
                    .definite_content_height
                    .is_none()
                    .then_some(inline_size),
            };
        }
        let needs_intrinsic = vertical_auto_block_size
            || horizontal_orthogonal_auto_inline_size
            || matches!(
                style.box_values.width,
                css::ComputedLengthPercentageOrAuto::MinContent
                    | css::ComputedLengthPercentageOrAuto::MaxContent
                    | css::ComputedLengthPercentageOrAuto::FitContent(_)
            );
        if !needs_intrinsic {
            if style.display.is_table() && has_auto_width(style) {
                return ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                    self.used_intrinsic_or_shrink_to_fit_width(
                        element,
                        style,
                        stylesheets,
                        width_inputs.available_outer_width,
                        width_inputs.horizontal_non_content,
                        child_boxes,
                        None,
                    ),
                ));
            }
            return ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                used_normal_flow_block_content_box_width(
                    style,
                    width_inputs
                        .percentage_basis
                        .value()
                        .unwrap_or(width_inputs.available_outer_width),
                    width_inputs.horizontal_non_content,
                ),
            ));
        }

        let intrinsic_sizes = self.block_intrinsic_content_sizes(
            element,
            style,
            stylesheets,
            child_boxes,
            width_inputs.available_outer_width.points(),
        );
        let (min_content, max_content) =
            intrinsic_sizes.physical_width_min_max(FlowAxes::for_style(style));
        ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
            crate::layout::intrinsic::content_box_width_from_intrinsic(
                style,
                width_inputs.available_outer_width,
                width_inputs.horizontal_non_content,
                min_content,
                max_content,
                if vertical_auto_block_size || horizontal_orthogonal_auto_inline_size {
                    crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit
                } else {
                    crate::layout::intrinsic::IntrinsicAutoWidth::FillAvailable
                },
            ),
        ))
    }

    /// Estimate block min/max-content sizes in logical axes.
    ///
    /// CSS Sizing defines intrinsic sizes per axis, and CSS Writing Modes maps
    /// physical width/height properties through logical inline/block sizing
    /// rules. Keeping both axes typed prevents a vertical physical `width`
    /// from accidentally consuming inline-size contributions:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>.
    pub(in crate::layout) fn block_intrinsic_content_sizes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> BlockIntrinsicContentSizes {
        let (min_inline, max_inline) = self.block_intrinsic_content_inline_widths(
            element,
            style,
            stylesheets,
            child_boxes,
            available_outer_width,
        );
        let (min_block, block_size_at_max_inline) = self.block_intrinsic_content_block_sizes(
            element,
            style,
            stylesheets,
            child_boxes,
            min_inline,
            available_outer_width,
        );
        BlockIntrinsicContentSizes {
            min_inline: LogicalInlineContentSize::new(content_box_pt(min_inline)),
            max_inline: LogicalInlineContentSize::new(content_box_pt(max_inline)),
            min_block: LogicalBlockContentSize::new(content_box_pt(min_block)),
            max_block: LogicalBlockContentSize::new(content_box_pt(
                block_size_at_max_inline.max(min_block),
            )),
            block_size_at_max_inline: LogicalBlockContentSize::new(content_box_pt(
                block_size_at_max_inline,
            )),
        }
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
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        self.block_intrinsic_content_inline_widths(
            element,
            style,
            stylesheets,
            child_boxes,
            available_outer_width,
        )
    }

    fn block_intrinsic_content_inline_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        // CSS Containment sizes the content box as empty while preserving the
        // containment box's own formatting-context contributions. In
        // particular, an empty grid still contributes explicit tracks and an
        // empty multicol still contributes authored column geometry.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if intrinsic_inline_size_is_contained(style) {
            if let Some(width) = contained_intrinsic_logical_inline_size(style) {
                let width = used_length_percentage(
                    width,
                    PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
                )
                .points();
                return (width, width);
            }
            if style.display.is_grid() {
                return self.size_contained_grid_intrinsic_widths(style);
            }
            return size_contained_multicol_intrinsic_inline_sizes(style).unwrap_or((0.0, 0.0));
        }
        if style.display.is_flex() {
            let contributions = self.estimate_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                PhysicalContentWidth::new(content_box_pt(available_outer_width)),
                child_boxes,
            );
            return (
                contributions.min_content.points(),
                contributions.max_content.points(),
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
        // A block container's intrinsic inline contribution is the largest
        // contribution of its in-flow block formatting-context descendants.
        // Inline collection deliberately omits block children, so recursively
        // query every block child here. This is particularly important for an
        // atomic inline formatting context, whose shrink-to-fit width must
        // include its block descendants.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution> and
        // <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes>.
        let built_child_boxes;
        let child_boxes = match child_boxes {
            Some(child_boxes) => Some(child_boxes),
            None => {
                built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                    element,
                    stylesheets,
                    style,
                );
                Some(built_child_boxes.as_slice())
            }
        };
        if let Some(child_boxes) = child_boxes {
            let mut block_child_min = 0.0_f32;
            let mut block_child_max = 0.0_f32;
            for child in child_boxes {
                // A table's intrinsic inline contribution is its table-wrapper
                // margin box, after CSS Tables has clamped an auto-layout grid
                // to its min-content width.  The generic block path only sees
                // the table's preferred `inline-size`, which can incorrectly
                // make a `width: min-content` ancestor narrower than an
                // unbreakable table cell.
                // <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>
                // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
                if let box_tree::FormattingBox::Table(table) = child {
                    let (child_min, child_max) = self.table_outer_intrinsic_widths_from_fragment(
                        table.core.element,
                        &table.core.style,
                        stylesheets,
                        &table.fragment,
                        available_outer_width,
                    );
                    block_child_min = block_child_min.max(child_min);
                    block_child_max = block_child_max.max(child_max);
                    continue;
                }
                let Some((child_element, _, child_style, child_children)) = child.element_parts()
                else {
                    continue;
                };
                // Inline descendants participate together in the parent's
                // inline formatting context. Treating them independently as
                // block children would discard their unbreakable sequence's
                // max-content contribution (for example two adjacent atomic
                // inlines), so recurse here only for block-level children.
                // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
                if !child_style.display.is_block_level() {
                    continue;
                }
                let metrics = intrinsic_box_metrics(child_style);
                if style.writing_mode.has_vertical_lines() {
                    // A vertical parent's logical inline axis is physical
                    // height. Its block children therefore contribute their
                    // physical outer height, not their physical width. This
                    // is essential for nested orthogonal roots: a horizontal
                    // child can obtain its height from a vertical grandchild
                    // even though neither child has inline text of its own.
                    // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
                    let specified_width = used_content_box_size(
                        child_style.box_values.width.clone(),
                        child_style.box_sizing,
                        PercentageBasis::definite(content_box_pt(available_outer_width)),
                        metrics.horizontal_non_content_length(),
                    );
                    let horizontal_child_line_measure =
                        (child_style.writing_mode == WritingMode::HorizontalTb).then(|| {
                            specified_width.unwrap_or_else(|| {
                                // An auto-sized horizontal child of a vertical
                                // flow root shrink-to-fits its physical width.
                                // Its parent's logical inline contribution is
                                // therefore the height at that final line
                                // measure, not the min-content wrapped height.
                                // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
                                let child_sizes = self.block_intrinsic_content_sizes(
                                    child_element,
                                    child_style,
                                    stylesheets,
                                    Some(child_children),
                                    available_outer_width,
                                );
                                let child_axes = FlowAxes::for_style(child_style);
                                let min_width = child_axes
                                    .physical_width_from_logical_content_sizes(
                                        child_sizes.min_inline,
                                        child_sizes.min_block,
                                    )
                                    .content_box_length();
                                let max_width = child_axes
                                    .physical_width_from_logical_content_sizes(
                                        child_sizes.max_inline,
                                        child_sizes.block_size_at_max_inline,
                                    )
                                    .content_box_length();
                                let available_width = content_box_pt(
                                    (available_outer_width
                                        - metrics.margin.left.points()
                                        - metrics.margin.right.points()
                                        - metrics.horizontal_non_content_length().points())
                                    .max(0.0),
                                );
                                crate::layout::intrinsic::shrink_to_fit_width(
                                    min_width,
                                    max_width,
                                    available_width,
                                )
                            })
                        });
                    let (child_min, child_max) =
                        if let Some(line_measure) = horizontal_child_line_measure {
                            // A horizontal child's used line measure determines
                            // its physical height contribution. Measuring at
                            // min-content would wrap text into extra lines and
                            // incorrectly make the vertical parent taller.
                            // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
                            let items = self.intrinsic_inline_items_for_element(
                                child_element,
                                child_style,
                                stylesheets,
                                Some(child_children),
                            );
                            let height = if items.is_empty() {
                                self.estimate_block_child_intrinsic_content_height(
                                    child_element,
                                    child_style,
                                    stylesheets,
                                    Some(child_children),
                                    line_measure.points(),
                                    available_outer_width,
                                )
                            } else {
                                self.inline_items_logical_block_size(
                                    items,
                                    child_style,
                                    line_measure.points(),
                                )
                            };
                            (height, height)
                        } else {
                            let child_sizes = self.block_intrinsic_content_sizes(
                                child_element,
                                child_style,
                                stylesheets,
                                Some(child_children),
                                available_outer_width,
                            );
                            let child_axes = FlowAxes::for_style(child_style);
                            (
                                child_axes
                                    .physical_height_from_logical_content_sizes(
                                        child_sizes.min_inline,
                                        child_sizes.min_block,
                                    )
                                    .points(),
                                child_axes
                                    .physical_height_from_logical_content_sizes(
                                        child_sizes.max_inline,
                                        child_sizes.max_block,
                                    )
                                    .points(),
                            )
                        };
                    let vertical_non_content = metrics.vertical_non_content_length().points();
                    let vertical_margins =
                        metrics.margin.top.points() + metrics.margin.bottom.points();
                    block_child_min =
                        block_child_min.max(child_min + vertical_non_content + vertical_margins);
                    block_child_max =
                        block_child_max.max(child_max + vertical_non_content + vertical_margins);
                } else {
                    let (child_min, child_max) = if writing_modes_are_orthogonal(
                        style.writing_mode,
                        child_style.writing_mode,
                    ) {
                        // The parent's logical inline axis is physical
                        // horizontal here. Project an orthogonal child's two
                        // logical contributions through its own flow axes;
                        // using its logical inline size directly would use a
                        // vertical child's physical height as a width.
                        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
                        let specified_width = used_content_box_size(
                            child_style.box_values.width.clone(),
                            child_style.box_sizing,
                            PercentageBasis::definite(content_box_pt(available_outer_width)),
                            metrics.horizontal_non_content_length(),
                        )
                        .map(SemanticLengthExt::points);
                        if let Some(width) = specified_width {
                            (width, width)
                        } else {
                            let child_sizes = self.block_intrinsic_content_sizes(
                                child_element,
                                child_style,
                                stylesheets,
                                Some(child_children),
                                available_outer_width,
                            );
                            let child_axes = FlowAxes::for_style(child_style);
                            (
                                child_axes
                                    .physical_width_from_logical_content_sizes(
                                        child_sizes.min_inline,
                                        child_sizes.min_block,
                                    )
                                    .points(),
                                child_axes
                                    .physical_width_from_logical_content_sizes(
                                        child_sizes.max_inline,
                                        child_sizes.block_size_at_max_inline,
                                    )
                                    .points(),
                            )
                        }
                    } else {
                        self.block_intrinsic_content_widths(
                            child_element,
                            child_style,
                            stylesheets,
                            Some(child_children),
                            available_outer_width,
                        )
                    };
                    let horizontal_non_content = metrics.horizontal_non_content_length();
                    let (child_min, child_max) = non_replaced_intrinsic_width_contributions(
                        child_style,
                        content_box_pt(child_min),
                        content_box_pt(child_max),
                        horizontal_non_content,
                    );
                    block_child_min = block_child_min.max(
                        child_min.points()
                            + horizontal_non_content.points()
                            + metrics.margin.left.points()
                            + metrics.margin.right.points(),
                    );
                    block_child_max = block_child_max.max(
                        child_max.points()
                            + horizontal_non_content.points()
                            + metrics.margin.left.points()
                            + metrics.margin.right.points(),
                    );
                }
            }
            if block_child_min > 0.0 || block_child_max > 0.0 {
                return (block_child_min, block_child_max.max(block_child_min));
            }
        }
        let contribution =
            self.with_intrinsic_inline_percentage_basis(PercentageBasis::indefinite(), |layout| {
                layout.intrinsic_inline_contribution_for_element(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                )
            });
        if contribution.max_content.points() > 0.0 || contribution.min_content.points() > 0.0 {
            return (
                contribution.min_content.points(),
                contribution.max_content.points(),
            );
        }
        let shrink_to_fit = self
            .estimate_shrink_to_fit_width(
                element,
                style,
                stylesheets,
                content_box_pt(available_outer_width),
                child_boxes,
                None,
            )
            .points();
        (shrink_to_fit, shrink_to_fit)
    }

    fn block_intrinsic_content_block_sizes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        min_inline: f32,
        available_outer_width: f32,
    ) -> (f32, f32) {
        if used_property_containment(element, style).size {
            let height = style
                .contain_intrinsic_size
                .height
                .clone()
                .map(|height| {
                    used_length_percentage(
                        height,
                        PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
                    )
                    .points()
                })
                .unwrap_or(0.0);
            return (height, height);
        }
        let items =
            self.intrinsic_inline_items_for_element(element, style, stylesheets, child_boxes);
        if items.is_empty() {
            let block_size = if style.writing_mode.has_vertical_lines() {
                self.estimate_block_child_intrinsic_logical_block_size(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    available_outer_width,
                )
            } else {
                self.estimate_block_child_intrinsic_content_height(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    min_inline,
                    available_outer_width,
                )
            };
            return (block_size, block_size);
        }

        let min_block = self.inline_items_logical_block_size(
            items.clone(),
            style,
            // A zero font size produces no text advance or inline strut.
            // Intrinsic measurement must keep that zero available span rather
            // than manufacturing one pixel (or one em) of text contribution.
            // <https://drafts.csswg.org/css-fonts-4/#font-size-prop>
            min_inline.max(0.0),
        );
        let max_block = self.inline_items_logical_block_size(items, style, f32::MAX);
        if element.tag.eq_ignore_ascii_case("html") {
            // The root's tree-abiding inline pseudo-elements coexist with its
            // block-level body canvas. Inline collection intentionally omits
            // that block child, but its intrinsic block contribution still
            // determines the root's used principal-flow span.
            // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
            // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
            let block_child_size = self.estimate_block_child_intrinsic_content_height(
                element,
                style,
                stylesheets,
                child_boxes,
                min_inline,
                available_outer_width,
            );
            (
                min_block.max(block_child_size),
                max_block.max(block_child_size),
            )
        } else {
            (min_block, max_block)
        }
    }

    /// Estimate a vertical block's logical block-size from its block children.
    ///
    /// The block axis of a vertical formatting context is physical horizontal.
    /// Reusing the ordinary auto-height estimator here would instead add each
    /// child's physical height, which is its logical inline contribution and
    /// can double the physical width of nested orthogonal boxes.
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
    fn estimate_block_child_intrinsic_logical_block_size(
        &mut self,
        _element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> f32 {
        let Some(child_boxes) = child_boxes else {
            return 0.0;
        };
        let parent_logical_inline_measure = used_content_box_size(
            style.box_values.height.value().clone(),
            style.box_sizing,
            PercentageBasis::definite(content_box_pt(available_outer_width)),
            intrinsic_box_metrics(style).vertical_non_content_length(),
        );
        let mut block_size = 0.0;
        for child in child_boxes {
            let Some((child_element, _, child_style, child_children)) = child.element_parts()
            else {
                continue;
            };
            if !child_style.display.is_block_level()
                || child_style.float != Float::None
                || matches!(child_style.position, Position::Absolute | Position::Fixed)
            {
                continue;
            }
            let metrics = intrinsic_box_metrics(child_style);
            let physical_width_percentage_basis =
                if writing_modes_are_orthogonal(style.writing_mode, child_style.writing_mode) {
                    self.current_child_available_space()
                        .orthogonal_physical_width_percentage_basis
                        .points()
                } else {
                    available_outer_width
                };
            let wrapped_vertical_child_width = (child_style.writing_mode.has_vertical_lines()
                && child_style.box_values.width.is_auto())
            .then(|| {
                parent_logical_inline_measure.and_then(|inline_measure| {
                    let items = self.intrinsic_inline_items_for_element(
                        child_element,
                        child_style,
                        stylesheets,
                        Some(child_children),
                    );
                    (!items.is_empty()).then(|| {
                        self.inline_items_logical_block_size(
                            items,
                            child_style,
                            inline_measure.points(),
                        )
                    })
                })
            })
            .flatten();
            let specified_width = used_content_box_size(
                child_style.box_values.width.clone(),
                child_style.box_sizing,
                PercentageBasis::definite(content_box_pt(physical_width_percentage_basis)),
                metrics.horizontal_non_content_length(),
            )
            .map(SemanticLengthExt::points);
            let child_width = specified_width
                .or(wrapped_vertical_child_width)
                .unwrap_or_else(|| {
                    let child_sizes = self.block_intrinsic_content_sizes(
                        child_element,
                        child_style,
                        stylesheets,
                        Some(child_children),
                        available_outer_width,
                    );
                    FlowAxes::for_style(child_style)
                        .physical_width_from_logical_content_sizes(
                            child_sizes.max_inline,
                            child_sizes.block_size_at_max_inline,
                        )
                        .points()
                });
            block_size += child_width
                + metrics.horizontal_non_content_length().points()
                + metrics.margin.left.points()
                + metrics.margin.right.points();
        }
        block_size
    }

    fn estimate_block_child_intrinsic_content_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        min_inline: f32,
        available_outer_width: f32,
    ) -> f32 {
        let mut intrinsic_style = style.clone();
        intrinsic_style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(min_inline.max(0.0)),
        );
        intrinsic_style.box_values.min_width = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style.box_values.max_width = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style
            .box_values
            .height
            .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
        intrinsic_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;

        let mut used_style = self.style_with_current_used_lengths(&intrinsic_style);
        let inline_basis = available_outer_width.max(min_inline).max(0.0);
        let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            PercentageBasis::definite(LogicalInlineContentSize::new(content_box_pt(inline_basis))),
        );
        let outer_height = self.estimate_block_like_height(
            element,
            &intrinsic_style,
            stylesheets,
            inline_basis,
            child_boxes,
        );
        (outer_height
            - used_style.margin.top
            - box_metrics.border.top.points()
            - used_style.padding.top
            - used_style.padding.bottom
            - box_metrics.border.bottom.points()
            - used_style.margin.bottom)
            .max(0.0)
    }

    fn inline_items_logical_block_size(
        &mut self,
        items: Vec<InlineItem>,
        style: &ComputedStyle,
        available_inline_size: f32,
    ) -> f32 {
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_inline_size,
            0.0,
            0.0,
        );
        // A baseline-aligned atomic inline's block-start margin moves the
        // whole line in the logical block direction.  Auto block sizing must
        // use that placed line extent, rather than treating every line as if
        // it started at the content edge.  This matters particularly for a
        // vertical `vertical-rl` block: the shift is physical-leftward and
        // must not make overflow at the logical block start widen the box.
        // Paint applies the identical per-line placement in
        // `prepare_inline_line_fragment`.
        // <https://drafts.csswg.org/css-inline-3/#line-layout>
        let mut block_cursor = 0.0;
        let mut block_end = 0.0_f32;
        for record in &sequence.records {
            block_cursor += record.block_before;
            let line_block_start_margin = record
                .fragment
                .as_ref()
                .map(|fragment| {
                    fragment
                        .items()
                        .iter()
                        .filter_map(|item| match item.item.as_ref() {
                            InlineLineItem::Atom(atom) => {
                                Some(inline_atom_logical_block_start_margin(atom, style))
                            }
                            InlineLineItem::Fragment(_) | InlineLineItem::Float(_) => None,
                        })
                        .fold(0.0_f32, f32::max)
                })
                .unwrap_or(0.0);
            let line_height = record.height();
            block_end = block_end.max(block_cursor + line_height - line_block_start_margin);
            block_cursor += line_height;
        }
        block_end.max(0.0)
    }

    pub(in crate::layout) fn block_layout_geometry(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> BlockLayoutGeometry {
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let child_available_space = self.current_child_available_space();
        // CSS Box percentages use the containing block's logical inline
        // size, even when this block establishes an orthogonal flow.
        // <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
        let percentage_basis = child_available_space
            .logical_inline_percentage_basis_for(child_available_space.writing_mode);
        let physical_width_percentage_basis = if crate::layout::block::writing_modes_are_orthogonal(
            child_available_space.writing_mode,
            style.writing_mode,
        ) {
            child_available_space
                .orthogonal_physical_width_percentage_basis
                .points()
        } else if self.active_fragmentainer_kind() == FragmentainerKind::Column
            && crate::layout::block::writing_modes_are_orthogonal(
                self.containing_block_writing_mode,
                style.writing_mode,
            )
        {
            // An orthogonal flow root fragments through the multicol
            // container's block axis. Its auto physical width is therefore
            // resolved against the multicol content box exported to its
            // children, rather than against one anonymous column slice.
            // <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto>
            child_available_space.physical_content_width.points()
        } else {
            containing_inline_size
        };
        self.block_layout_geometry_in_inline_span(
            element,
            style,
            stylesheets,
            child_boxes,
            BlockLayoutInlineConstraint {
                containing_inline_span: PageInlineSpan::from_edges(
                    self.content_left,
                    self.content_right,
                ),
                percentage_basis,
                physical_width_percentage_basis: PhysicalContentWidth::new(content_box_pt(
                    physical_width_percentage_basis,
                )),
                auto_border_box_width: None,
            },
        )
    }

    pub(in crate::layout) fn block_layout_geometry_in_inline_span(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        constraint: BlockLayoutInlineConstraint,
    ) -> BlockLayoutGeometry {
        let containing_inline_span = constraint.containing_inline_span;
        let percentage_basis = constraint.percentage_basis;
        let physical_width_percentage_basis = constraint.physical_width_percentage_basis.points();
        let containing_inline_size = containing_inline_span.width();
        let mut used_style = self.style_with_current_used_lengths(style);
        let box_metrics =
            apply_used_box_metrics_for_logical_inline_basis(&mut used_style, percentage_basis);
        let relative_offset = self.normal_flow_relative_position_offset(&used_style);
        let has_indefinite_orthogonal_containing_width =
            crate::layout::block::writing_modes_are_orthogonal(
                self.containing_block_writing_mode,
                used_style.writing_mode,
            ) && !self
                .current_child_available_space()
                .physical_width_is_definite;
        // An auto-sized vertical containing block has an indefinite physical
        // width while its horizontal child is being sized.  The child's auto
        // inline measure therefore uses the Writing Modes fallback, rather
        // than the parent's eventual content-derived zero width.  This is an
        // available-space constraint only: it does not make percentages
        // definite.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let auto_inline_constraint =
            if has_indefinite_orthogonal_containing_width && has_auto_width(&used_style) {
                physical_width_percentage_basis
            } else {
                containing_inline_size
            };
        let available_outer_width =
            normal_flow_block_available_outer_width(&used_style, layout_pt(auto_inline_constraint));
        // Intrinsic sizing normally treats a percentage-dependent width as
        // auto until its containing-block basis is known. At this layout
        // boundary the physical width basis is known separately from the
        // available inline span for an orthogonal flow, so retain that basis
        // when resolving the specified physical width.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let intrinsic_width_percentage_basis = matches!(
            used_style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(ref value)
                if value.needs_percentage_basis()
        )
        .then_some(layout_pt(physical_width_percentage_basis))
        .unwrap_or(available_outer_width);
        let border_edges = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content_length();
        let vertical_extras = box_metrics.vertical_non_content_length();
        let containing_block_content_height = self
            .definite_block_size_stack
            .last()
            .cloned()
            .unwrap_or_else(PercentageBasis::indefinite);
        let containing_block_stretch_height = containing_block_content_height
            .map_value(crate::units::IntoLayoutLength::into_layout_length)
            .value()
            .unwrap_or_else(|| layout_pt(0.0));
        let height_depends_on_intrinsic_content =
            needs_intrinsic_height_contribution(used_style.box_values.height.value().clone())
                || needs_intrinsic_height_contribution(used_style.box_values.min_height.clone())
                || needs_intrinsic_height_contribution(used_style.box_values.max_height.clone());
        // An `auto`-basis calc-size on the dependent block axis can be made
        // definite by a definite inline size and preferred aspect ratio. Do
        // not confuse that transfer with an intrinsic sizing query: the
        // transfer establishes the `auto` basis before calc-size applies its
        // arithmetic.
        // <https://drafts.csswg.org/css-values-5/#calc-size> and
        // <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
        let aspect_ratio_resolves_auto_basis_height = used_style
            .box_values
            .height
            .calc_size_with_auto_basis()
            .is_some()
            && !has_auto_width(&used_style)
            && !needs_intrinsic_width_contribution(used_style.box_values.width.clone());
        let explicit_content_height = (!height_depends_on_intrinsic_content
            || aspect_ratio_resolves_auto_basis_height)
            .then(|| {
                used_content_box_height_or_auto_with_basis(
                    &used_style,
                    containing_block_content_height,
                    vertical_extras,
                )
                .map(SemanticLengthExt::points)
            })
            .flatten();
        // At this normal-flow layout boundary the physical-width percentage
        // basis is already definite.  Percentages need intrinsic fallback
        // only during an intrinsic query with an indefinite containing block;
        // treating ordinary `width`/`min-width: 0%` as such a query forces
        // every block descendant through min/max-content measurement.
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        let width_needs_intrinsic_sizes = |value: &css::ComputedLengthPercentageOrAuto| {
            !matches!(
                value,
                css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
            ) && needs_intrinsic_width_contribution(value.clone())
        };
        let intrinsic_sizes = (width_needs_intrinsic_sizes(&used_style.box_values.width)
            || width_needs_intrinsic_sizes(&used_style.box_values.min_width)
            || width_needs_intrinsic_sizes(&used_style.box_values.max_width)
            || (used_style.box_values.width.is_auto()
                && used_style.box_values.min_width.is_auto()
                && used_style
                    .aspect_ratio
                    .preferred_ratio_for_non_replaced(false)
                    .is_some()))
        .then(|| {
            self.block_intrinsic_content_sizes(
                element,
                &used_style,
                stylesheets,
                child_boxes,
                available_outer_width.points(),
            )
        });
        let width_resolution = if let Some(auto_border_box_width) = constraint
            .auto_border_box_width
            .filter(|_| has_auto_width(&used_style))
        {
            ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(content_box_pt(
                (auto_border_box_width.points() - horizontal_extras.points()).max(0.0),
            )))
        } else if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) =
                intrinsic_sizes.physical_width_min_max(FlowAxes::for_style(&used_style));
            ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                crate::layout::intrinsic::content_box_width_from_intrinsic(
                    &used_style,
                    intrinsic_width_percentage_basis,
                    horizontal_extras,
                    min_content,
                    max_content,
                    crate::layout::intrinsic::IntrinsicAutoWidth::FillAvailable,
                ),
            ))
        } else {
            self.resolved_block_physical_content_width(
                element,
                &used_style,
                stylesheets,
                child_boxes,
                BlockContentWidthInputs {
                    available_outer_width,
                    percentage_basis: PercentageBasis::definite(layout_pt(
                        physical_width_percentage_basis,
                    )),
                    horizontal_non_content: horizontal_extras,
                    definite_content_height: explicit_content_height
                        .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                },
            )
        };
        let selected_logical_inline_size = width_resolution.selected_logical_inline_size;
        let requested_content_width = width_resolution.content_width;
        let requested_content_width = if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) =
                intrinsic_sizes.physical_width_min_max(FlowAxes::for_style(&used_style));
            PhysicalContentWidth::new(constrain_width_with_intrinsic(
                &used_style,
                requested_content_width.content_box_length(),
                min_content,
                max_content,
                PercentageBasis::definite(content_box_pt(
                    intrinsic_width_percentage_basis.points(),
                )),
                horizontal_extras,
            ))
        } else {
            requested_content_width
        };
        let requested_content_width = explicit_content_height
            .and_then(|height| {
                non_replaced_aspect_ratio_content_width(
                    &used_style,
                    height,
                    horizontal_extras.points(),
                    vertical_extras.points(),
                )
            })
            .map(|width| {
                // CSS Sizing's automatic content-based minimum does not
                // apply to the ratio-dependent inline axis of a scroll
                // container. The transferred preferred width still applies,
                // but overflow owns excess inline content instead.
                // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
                let automatic_minimum = if !used_style.overflow_x.is_scrollable() {
                    intrinsic_sizes
                        .map(|sizes| {
                            let (min_content, max_content) =
                                sizes.physical_width_min_max(FlowAxes::for_style(&used_style));
                            intrinsic_width_constraint(
                                used_style.box_values.min_width.clone(),
                                used_style.box_sizing,
                                PercentageBasis::definite(content_box_pt(containing_inline_size)),
                                horizontal_extras,
                                min_content,
                                max_content,
                            )
                            .unwrap_or(min_content)
                            .points()
                        })
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                PhysicalContentWidth::new(content_box_pt(width.max(automatic_minimum)))
            })
            .unwrap_or(requested_content_width);
        let mut width = resolve_normal_flow_block_inline_geometry(
            &mut used_style,
            containing_inline_span,
            requested_content_width,
            horizontal_extras,
            self.containing_block_direction,
            true,
        );
        // In a parallel vertical block flow, a physical width is the logical
        // block size. Anchor its resolved border box at logical block-start.
        // An auto-sized orthogonal horizontal child contributes its intrinsic
        // physical width along the vertical parent's block axis, so its
        // static position begins at that block-start edge. A specified
        // physical width remains an ordinary horizontal block width and uses
        // the normal static position instead.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let root_pseudo_projection = self
            .root_pseudo_block_projection
            .filter(|projection| projection.element == element.id);
        if WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes()
            && (root_pseudo_projection.is_some()
                || !crate::layout::block::writing_modes_are_orthogonal(
                    self.containing_block_writing_mode,
                    style.writing_mode,
                ))
        {
            let border_box_width = width.border_box_inline_span.width();
            let block_start = root_pseudo_projection
                .map(|projection| projection.block_start)
                .unwrap_or_else(|| block_start_side(style.writing_mode));
            let block_end_inset = root_pseudo_projection
                .map(|projection| projection.block_end_inset.points())
                .unwrap_or(0.0);
            let start = match block_start {
                PhysicalSide::Left => containing_inline_span.left_x() + used_style.margin.left,
                PhysicalSide::Right => {
                    containing_inline_span.right_x()
                        - used_style.margin.right
                        - block_end_inset
                        - border_box_width
                }
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    unreachable!("a vertical writing mode must have a horizontal block axis")
                }
            };
            width.border_box_inline_span = PageInlineSpan::new(start, border_box_width);
        } else if !style.writing_mode.has_vertical_lines()
            && self.containing_block_writing_mode.has_vertical_lines()
        {
            // The child's physical horizontal border-box span is the
            // containing vertical flow's logical block span. Its static
            // position therefore starts at the containing block's logical
            // block-start edge, regardless of whether CSS `width` is `auto`
            // or explicitly specified.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
            let border_box_width = width.border_box_inline_span.width();
            let start = match block_start_side(self.containing_block_writing_mode) {
                PhysicalSide::Left => containing_inline_span.left_x() + used_style.margin.left,
                PhysicalSide::Right => {
                    containing_inline_span.right_x() - used_style.margin.right - border_box_width
                }
                PhysicalSide::Top | PhysicalSide::Bottom => {
                    unreachable!("a vertical containing block must have a horizontal block axis")
                }
            };
            width.border_box_inline_span = PageInlineSpan::new(start, border_box_width);
        }
        let mut content_width = width.content_width;
        let mut content_width_points = content_width.points();
        let mut unconstrained_aspect_height = None;
        let mut definite_content_height = (!height_depends_on_intrinsic_content
            || aspect_ratio_resolves_auto_basis_height)
            .then(|| {
                explicit_content_height.or_else(|| {
                    non_replaced_aspect_ratio_content_height(
                        &used_style,
                        content_width_points,
                        horizontal_extras.points(),
                        vertical_extras.points(),
                    )
                })
            })
            .flatten()
            .map(|height| {
                unconstrained_aspect_height = Some(height);
                constrain_height_with_stretch_fit(
                    &used_style,
                    content_box_pt(height),
                    containing_block_stretch_height,
                    layout_pt(used_style.margin.top + used_style.margin.bottom),
                    vertical_extras,
                )
                .points()
            });
        // A min/max block-size constraint can change the auto axis selected
        // through the preferred aspect ratio. Re-resolve the dependent inline
        // axis through the normal block-width equation, so its own min/max
        // constraints and auto margins remain authoritative.
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
        if unconstrained_aspect_height.is_some_and(|unconstrained| {
            definite_content_height
                .is_some_and(|constrained| (constrained - unconstrained).abs() > 0.01)
        }) && (has_auto_width(&used_style)
            || needs_intrinsic_width_contribution(used_style.box_values.width.clone()))
            && let Some(height) = definite_content_height
            && let Some(transferred_width) = non_replaced_aspect_ratio_content_width(
                &used_style,
                height,
                horizontal_extras.points(),
                vertical_extras.points(),
            )
        {
            width = resolve_normal_flow_block_inline_geometry(
                &mut used_style,
                containing_inline_span,
                PhysicalContentWidth::new(content_box_pt(transferred_width)),
                horizontal_extras,
                self.containing_block_direction,
                true,
            );
            content_width = width.content_width;
            content_width_points = content_width.points();
            definite_content_height = non_replaced_aspect_ratio_content_height(
                &used_style,
                content_width_points,
                horizontal_extras.points(),
                vertical_extras.points(),
            )
            .map(|height| {
                constrain_height_with_stretch_fit(
                    &used_style,
                    content_box_pt(height),
                    containing_block_stretch_height,
                    layout_pt(used_style.margin.top + used_style.margin.bottom),
                    vertical_extras,
                )
                .points()
            });
        }
        let definite_content_height = definite_content_height
            .map(|height| PhysicalContentHeight::new(content_box_pt(height)));
        let content_logical_inline_size = selected_logical_inline_size
            .filter(|_| definite_content_height.is_none())
            .unwrap_or_else(|| {
                self.block_content_logical_inline_size(
                    element,
                    &used_style,
                    stylesheets,
                    child_boxes,
                    PhysicalContentWidth::new(content_width),
                    definite_content_height,
                )
            });
        let outer_inline_span = PageInlineSpan::new(
            width.border_box_inline_span.left_x() + relative_offset.x(),
            width.border_box_inline_span.width(),
        );
        let inner_x =
            outer_inline_span.left_x() + border_edges.left.points() + used_style.padding.left;
        let content_inline_span = PageInlineSpan::new(inner_x, content_width.points());
        BlockLayoutGeometry {
            style: used_style.into_computed(),
            relative_offset,
            border_edges,
            vertical_non_content: vertical_extras,
            containing_block_content_height,
            definite_content_height,
            content_logical_inline_size,
            outer_inline: BlockBorderBoxInlineBounds::new(outer_inline_span),
            content_inline: BlockContentBoxInlineBounds::new(content_inline_span),
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
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        content_width: PhysicalContentWidth,
        definite_content_height: Option<PhysicalContentHeight>,
    ) -> LogicalInlineContentSize {
        let points = if WritingModeAxes::new(style.writing_mode, style.direction)
            .swaps_physical_axes()
        {
            let containing_space = self.current_child_available_space();
            if let Some(definite_content_height) = definite_content_height {
                return LogicalInlineContentSize::new(definite_content_height.content_box_length());
            }
            if !writing_modes_are_orthogonal(containing_space.writing_mode, style.writing_mode) {
                // `height` is the logical inline size in vertical writing.
                // An auto-sized normal-flow block stretches through its
                // containing block's available inline size just as a
                // horizontal block's auto width does.
                // https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping
                let available = containing_space.logical_inline_size_for(style.writing_mode);
                let percentage_basis = containing_space
                    .logical_inline_percentage_basis_for(style.writing_mode)
                    .map_value(LogicalInlineContentSize::content_box_length);
                return LogicalInlineContentSize::new(constrain_content_height(
                    style,
                    available.content_box_length(),
                    percentage_basis,
                ));
            }
            // The orthogonal available size is the containing block's
            // available *outer* inline size.  Fit-content line layout needs
            // this box's content inline size, so remove its physical
            // top/bottom margin, padding, and border before choosing the
            // line-fitting measure.  Treating the available outer measure as
            // content-box space makes a constrained parent overflow by this
            // amount in nested orthogonal flows.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
            // <https://drafts.csswg.org/css-sizing-3/#fit-content-size>
            let borders = used_border_widths(style);
            let logical_inline_outer_non_content = style.margin.top
                + style.padding.top
                + borders.top
                + style.padding.bottom
                + borders.bottom
                + style.margin.bottom;
            let containing_stretch_fit = (containing_space
                .logical_inline_size_for(style.writing_mode)
                .points()
                - logical_inline_outer_non_content)
                .max(0.0);
            // The box's own physical block-size constraints map to the
            // logical inline axis in vertical writing. They therefore bound
            // the fit-content measure before its inline contents are laid
            // out. Constraining only the final physical height would let a
            // vertical block first lay out against the ICB and then clip that
            // unwrapped line, instead of reflowing it into the constrained
            // inline measure.
            // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
            // <https://drafts.csswg.org/css-sizing-3/#fit-content-size>
            let own_inline_constraint = orthogonal_fallback_physical_content_height(
                style,
                containing_space
                    .physical_height_percentage_basis()
                    .map_value(|height| height.content_box_length()),
            )
            .map(PhysicalContentHeight::points);
            let stretch_fit = own_inline_constraint
                .map(|constraint| containing_stretch_fit.min(constraint))
                .unwrap_or(containing_stretch_fit);
            // DOM-backed blocks normally defer formatting-box construction to
            // final layout. Orthogonal fit-content sizing needs the same
            // atomic-inline classification as that final pass, however: a
            // durable table box records table structure independently of its
            // outer `display`, and a raw DOM probe would otherwise lose an
            // `inline-table` entirely.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
            // <https://drafts.csswg.org/css-display-3/#valdef-display-inline-table>
            let owned_child_boxes = child_boxes.is_none().then(|| {
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style)
            });
            let child_boxes = child_boxes.or(owned_child_boxes.as_deref());
            let (min_content, max_content) =
                if child_boxes.is_some_and(has_non_inline_formatting_box) {
                    // Inline collection deliberately omits block children. For a
                    // vertical flow root its logical inline measure is physical
                    // height, so a nested horizontal block must be measured by
                    // the block intrinsic model before fit-content negotiation.
                    // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
                    let sizes = self.block_intrinsic_content_sizes(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        content_width.points(),
                    );
                    (sizes.min_inline.points(), sizes.max_inline.points())
                } else {
                    let contribution = self.intrinsic_inline_contribution_for_element(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                    );
                    (
                        contribution.min_content.points(),
                        contribution.max_content.points(),
                    )
                };
            max_content.min(min_content.max(stretch_fit)).max(1.0)
        } else {
            content_width.points().max(1.0)
        };
        LogicalInlineContentSize::new(content_box_pt(points))
    }
}
