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
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        width_inputs: BlockContentWidthInputs,
    ) -> PhysicalContentWidth {
        let needs_intrinsic = matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        );
        if !needs_intrinsic {
            if style.display.is_table() && has_auto_width(style) {
                return PhysicalContentWidth::new(self.used_intrinsic_or_shrink_to_fit_width(
                    element,
                    style,
                    stylesheets,
                    width_inputs.available_outer_width,
                    width_inputs.horizontal_non_content,
                    child_boxes,
                    None,
                ));
            }
            return PhysicalContentWidth::new(used_normal_flow_block_content_box_width(
                style,
                width_inputs
                    .percentage_basis
                    .value()
                    .unwrap_or(width_inputs.available_outer_width),
                width_inputs.horizontal_non_content,
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
        PhysicalContentWidth::new(crate::layout::intrinsic::content_box_width_from_intrinsic(
            style,
            width_inputs.available_outer_width,
            width_inputs.horizontal_non_content,
            min_content,
            max_content,
            crate::layout::intrinsic::IntrinsicAutoWidth::FillAvailable,
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
        stylesheets: &[Stylesheet],
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
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        let sizes = self.block_intrinsic_content_sizes(
            element,
            style,
            stylesheets,
            child_boxes,
            available_outer_width,
        );
        (sizes.min_inline.points(), sizes.max_inline.points())
    }

    fn block_intrinsic_content_inline_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
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
        // A block container's intrinsic inline contribution is the largest
        // contribution of its in-flow block formatting-context descendants.
        // Inline collection deliberately omits block children, so recursively
        // query every block child here. This is particularly important for an
        // atomic inline formatting context, whose shrink-to-fit width must
        // include its block descendants.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution> and
        // <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes>.
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
                        table.element,
                        &table.style,
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
                let (child_min, child_max) = self.block_intrinsic_content_widths(
                    child_element,
                    child_style,
                    stylesheets,
                    Some(child_children),
                    available_outer_width,
                );
                let metrics = intrinsic_box_metrics(child_style);
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
                        + metrics.margin.left
                        + metrics.margin.right,
                );
                block_child_max = block_child_max.max(
                    child_max.points()
                        + horizontal_non_content.points()
                        + metrics.margin.left
                        + metrics.margin.right,
                );
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
        if contribution.max_content > 0.0 || contribution.min_content > 0.0 {
            return (contribution.min_content, contribution.max_content);
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
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        min_inline: f32,
        available_outer_width: f32,
    ) -> (f32, f32) {
        if style.contain.size {
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
            let block_size = self.estimate_block_child_intrinsic_content_height(
                element,
                style,
                stylesheets,
                child_boxes,
                min_inline,
                available_outer_width,
            );
            return (block_size, block_size);
        }

        let min_block = self.inline_items_logical_block_size(
            items.clone(),
            style,
            min_inline.max(style.font_size).max(1.0),
        );
        let max_block = self.inline_items_logical_block_size(items, style, f32::MAX);
        (min_block, max_block)
    }

    fn estimate_block_child_intrinsic_content_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
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
        intrinsic_style.box_values.height = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;

        let mut used_style = self.style_with_current_used_lengths(&intrinsic_style);
        let inline_basis = available_outer_width.max(min_inline).max(0.0);
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(inline_basis)),
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
            - box_metrics.border.top
            - used_style.padding.top
            - used_style.padding.bottom
            - box_metrics.border.bottom
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
        sequence.total_height().max(0.0)
    }

    pub(in crate::layout) fn block_layout_geometry(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> BlockLayoutGeometry {
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let child_available_space = self.current_child_available_space();
        // CSS Box percentages use the containing block's logical inline
        // size, even when this block establishes an orthogonal flow.
        // <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
        let percentage_basis = child_available_space
            .logical_inline_percentage_basis_for(child_available_space.writing_mode);
        let physical_width_percentage_basis = if self.active_fragmentainer_kind()
            == FragmentainerKind::Column
            && crate::layout::block::writing_modes_are_orthogonal(
                self.containing_block_writing_mode,
                style.writing_mode,
            ) {
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
                containing_left: self.content_left,
                containing_right: self.content_right,
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
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        constraint: BlockLayoutInlineConstraint,
    ) -> BlockLayoutGeometry {
        let containing_left = constraint.containing_left;
        let containing_right = constraint.containing_right;
        let percentage_basis = constraint.percentage_basis.map_value(|size| {
            crate::units::IntoLayoutLength::into_layout_length(size.content_box_length())
        });
        let physical_width_percentage_basis = constraint.physical_width_percentage_basis.points();
        let containing_inline_size = (containing_right - containing_left).max(0.0);
        let mut used_style = self.style_with_current_used_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, percentage_basis);
        let relative_offset = self.normal_flow_relative_position_offset(&used_style);
        let available_outer_width =
            normal_flow_block_available_outer_width(&used_style, layout_pt(containing_inline_size));
        let border_widths = box_metrics.border;
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
            needs_intrinsic_height_contribution(used_style.box_values.height.clone())
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
        let intrinsic_sizes =
            (needs_intrinsic_width_contribution(used_style.box_values.width.clone())
                || needs_intrinsic_width_contribution(used_style.box_values.min_width.clone())
                || needs_intrinsic_width_contribution(used_style.box_values.max_width.clone())
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
        let requested_content_width = if let Some(auto_border_box_width) = constraint
            .auto_border_box_width
            .filter(|_| has_auto_width(&used_style))
        {
            PhysicalContentWidth::new(content_box_pt(
                (auto_border_box_width.points() - horizontal_extras.points()).max(0.0),
            ))
        } else if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) =
                intrinsic_sizes.physical_width_min_max(FlowAxes::for_style(&used_style));
            PhysicalContentWidth::new(crate::layout::intrinsic::content_box_width_from_intrinsic(
                &used_style,
                available_outer_width,
                horizontal_extras,
                min_content,
                max_content,
                crate::layout::intrinsic::IntrinsicAutoWidth::FillAvailable,
            ))
        } else {
            self.used_block_physical_content_width(
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
                },
            )
        };
        let requested_content_width = if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) =
                intrinsic_sizes.physical_width_min_max(FlowAxes::for_style(&used_style));
            PhysicalContentWidth::new(constrain_width_with_intrinsic(
                &used_style,
                requested_content_width.content_box_length(),
                min_content,
                max_content,
                PercentageBasis::definite(content_box_pt(containing_inline_size)),
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
        let mut width = resolve_normal_flow_block_width(
            &mut used_style,
            containing_left,
            containing_right,
            requested_content_width,
            horizontal_extras,
            self.containing_block_direction,
            true,
        );
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
            width = resolve_normal_flow_block_width(
                &mut used_style,
                containing_left,
                containing_right,
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
        let content_logical_inline_size = self.block_content_logical_inline_size(
            element,
            &used_style,
            stylesheets,
            child_boxes,
            PhysicalContentWidth::new(content_width),
            definite_content_height,
        );
        let outer_width = width.border_box_width.points();
        let outer_x = width.border_box_x + relative_offset.x();
        let inner_x = outer_x + border_widths.left + used_style.padding.left;

        BlockLayoutGeometry {
            style: used_style,
            relative_offset,
            border_widths,
            vertical_extras: vertical_extras.points(),
            containing_block_content_height,
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
                return LogicalInlineContentSize::new(content_box_pt(
                    containing_space
                        .logical_inline_size_for(style.writing_mode)
                        .points()
                        .max(1.0),
                ));
            }
            let stretch_fit = containing_space
                .logical_inline_size_for(style.writing_mode)
                .points();
            let contribution = self.intrinsic_inline_contribution_for_element(
                element,
                style,
                stylesheets,
                child_boxes,
            );
            contribution
                .max_content
                .min(contribution.min_content.max(stretch_fit))
                .max(1.0)
        } else {
            content_width.points().max(1.0)
        };
        LogicalInlineContentSize::new(content_box_pt(points))
    }
}
