use super::*;
use crate::layout::assets::PositionedAutoBlockMeasurementSpace;
use crate::units::{IntoLayoutLength, layout_to_content_box_length};

pub(in crate::layout::flex) struct FlexMinContentBlockContainingSpace {
    pub(in crate::layout::flex) inline_size: LogicalInlineContentSize,
    pub(in crate::layout::flex) height_percentage_basis: BlockSizePercentageBasis,
}

/// Signed inline-axis extras added to a flex item's intrinsic content
/// contribution. Margins may be negative, so this is deliberately not a
/// `MarginBoxLength`.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexIntrinsicInlineOuterExtras(LayoutLength);

impl FlexIntrinsicInlineOuterExtras {
    pub(in crate::layout::flex) fn new(value: LayoutLength) -> Self {
        Self(value)
    }

    /// Add this signed outer contribution to an intrinsic content-box
    /// contribution. The result remains an intrinsic logical-inline
    /// contribution, rather than becoming a margin-box size: negative
    /// margins are deliberately preserved until the contribution merge.
    pub(in crate::layout::flex) fn add_to(
        self,
        contribution: LogicalInlineContentSize,
    ) -> LogicalInlineContentSize {
        LogicalInlineContentSize::new(layout_to_content_box_length(
            contribution.content_box_length().into_layout_length() + self.0,
        ))
    }
}

/// A signed, margin-inclusive child contribution accumulated into a flex
/// item's logical block-size estimate.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlexBlockStackContribution(LayoutLength);

impl FlexBlockStackContribution {
    pub(super) fn zero() -> Self {
        Self(layout_pt(0.0))
    }

    pub(super) fn from_content_size(value: LogicalBlockContentSize) -> Self {
        Self(value.content_box_length().into_layout_length())
    }

    pub(super) fn from_outer_extent(value: LayoutLength) -> Self {
        Self(value)
    }

    pub(super) fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    pub(super) fn max(self, other: Self) -> Self {
        if self.0 >= other.0 { self } else { other }
    }

    pub(super) fn as_content_size(self) -> LogicalBlockContentSize {
        LogicalBlockContentSize::new(layout_to_content_box_length(self.0))
    }
}

pub(in crate::layout::flex) fn merge_outer_intrinsic_widths(
    contribution: &mut inline_layout::InlineIntrinsicContribution,
    child_contribution: inline_layout::InlineIntrinsicContribution,
    child_style: &ComputedStyle,
    containing_inline_size: LogicalInlineContentSize,
) {
    let outer_edges = intrinsic_horizontal_outer_edges(child_style, containing_inline_size);
    contribution.include_max(inline_layout::InlineIntrinsicContribution::new(
        outer_edges.add_to(child_contribution.min_content),
        outer_edges.add_to(child_contribution.max_content),
    ));
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::flex) fn estimate_child_inline_measurement(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &Stylesheets<'_>,
        available_inline_size: LogicalInlineContentSize,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return child
                .anonymous_content()
                .map(|children| {
                    self.intrinsic_inline_measurement_for_boxes(
                        children,
                        &child.style,
                        stylesheets,
                        available_inline_size.content_box_length().points(),
                    )
                })
                .unwrap_or_default();
        };
        self.with_ancestor_signature(signature.clone(), |layout| {
            layout.intrinsic_inline_measurement_for_element(
                element,
                &child.style,
                stylesheets,
                child_boxes,
                available_inline_size.content_box_length().points(),
            )
        })
    }

    /// Estimate the balanced content block-size of simple inline multicol flex items.
    ///
    /// CSS Flexbox computes a flex item's hypothetical cross size by laying the
    /// item out as an in-flow block-level box, while CSS Multi-column layout
    /// balances auto-height columns across their used column count:
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
    /// <https://www.w3.org/TR/css-multicol-1/#filling-columns>.
    pub(in crate::layout::flex) fn estimate_child_multicol_inline_height(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &Stylesheets<'_>,
        content_width: LogicalInlineContentSize,
    ) -> Option<LogicalBlockContentSize> {
        let content_width_points = content_width.points();
        let multicol_style = self.multicol_used_style(&child.style);
        let style = &multicol_style;
        let (_, _, child_boxes) = child.element_parts()?;
        let child_boxes = child_boxes?;
        if has_non_inline_formatting_box(child_boxes) {
            // This graph shortcut models a single inline formatting context.
            // Block children and spanners require the block-flow multicol
            // measurement performed earlier; a trailing inline line must not
            // replace the height of preceding column sets.
            // <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
            return self
                .estimate_multicol_auto_block_size(
                    style,
                    stylesheets,
                    child_boxes,
                    content_width_points,
                )
                .map(|height| LogicalBlockContentSize::new(content_box_pt(height)));
        }
        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_width.content_box_length()),
            style.font_size,
        )
        .points();
        let column_count = used_multicol_column_count(style, content_width_points, gap)
            .filter(|count| *count > 1)?;
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((content_width_points - total_gap) / column_count as f32).max(1.0);
        let available_column_width =
            (column_width - style.padding.left - style.padding.right).max(1.0);
        let measurement = self.estimate_child_inline_measurement(
            child,
            stylesheets,
            LogicalInlineContentSize::new(content_box_pt(available_column_width)),
        );

        (measurement.line_count() > 0).then(|| {
            let block_size = match style.column_fill {
                css::ColumnFill::Auto => measurement.sequence.total_height(),
                css::ColumnFill::Balance | css::ColumnFill::BalanceAll => measurement
                    .sequence
                    .balanced_multicolumn_height(column_count, style),
            };
            LogicalBlockContentSize::new(content_box_pt(block_size.max(style.line_height)))
        })
    }

    /// Estimates descendant min/max inline contributions from graph-backed fragments.
    ///
    /// CSS Sizing computes intrinsic inline sizes from the inline formatting
    /// input and CSS Text break opportunities. Flexbox consumes both values
    /// for flex base sizes and automatic minimum sizes:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
    pub(in crate::layout::flex) fn estimate_child_intrinsic_widths(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &Stylesheets<'_>,
        containing_inline_size: LogicalInlineContentSize,
        inline_contribution: inline_layout::InlineIntrinsicContribution,
    ) -> inline_layout::InlineIntrinsicContribution {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return inline_contribution;
        };
        if child.style.display.is_table() {
            return self.with_ancestor_signature(signature.clone(), |layout| {
                let built_child_boxes;
                let child_boxes = if let Some(child_boxes) = child_boxes {
                    child_boxes
                } else {
                    built_child_boxes = layout.build_frozen_child_boxes_with_current_ancestors(
                        element,
                        stylesheets,
                        &child.style,
                    );
                    &built_child_boxes
                };
                let fragment = box_tree::build_frozen_table_fragment(
                    element,
                    signature,
                    &child.style,
                    child_boxes,
                );
                // A table wrapper exposes the grid's min-content floor and
                // independent max-content contribution to intrinsic flex
                // sizing. Generic block measurement only sees the first
                // available table-cell contribution.
                // <https://drafts.csswg.org/css-tables/#used-min-width-of-table>
                // <https://drafts.csswg.org/css-flexbox-1/#intrinsic-main-sizes>
                let sizing = layout.table_wrapper_flex_sizing_from_fragment(
                    element,
                    &child.style,
                    stylesheets,
                    &fragment,
                    containing_inline_size.points(),
                );
                inline_layout::InlineIntrinsicContribution::new(
                    sizing.grid_min_content_inline,
                    sizing
                        .grid_max_content_inline
                        .resolve_against(containing_inline_size),
                )
            });
        }
        self.with_ancestor_signature(signature.clone(), |layout| {
            layout.estimate_element_flex_intrinsic_widths(
                element,
                &child.style,
                stylesheets,
                child_boxes,
                containing_inline_size,
                inline_contribution,
            )
        })
    }

    pub(in crate::layout::flex) fn estimate_element_flex_intrinsic_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_inline_size: LogicalInlineContentSize,
        inline_contribution: inline_layout::InlineIntrinsicContribution,
    ) -> inline_layout::InlineIntrinsicContribution {
        if used_property_containment(element, style).size {
            return inline_layout::InlineIntrinsicContribution::default();
        }
        let mut contribution = inline_contribution;
        if let Some(child_boxes) = child_boxes {
            let (float_min, float_max) = self.inline_float_run_intrinsic_widths_for_boxes(
                child_boxes,
                style,
                stylesheets,
                containing_inline_size.points(),
            );
            contribution.include_max(inline_layout::InlineIntrinsicContribution::new(
                LogicalInlineContentSize::new(content_box_pt(float_min)),
                LogicalInlineContentSize::new(content_box_pt(float_max)),
            ));
            self.merge_box_children_flex_intrinsic_widths(
                child_boxes,
                stylesheets,
                containing_inline_size,
                &mut contribution,
            );
        } else {
            self.merge_dom_children_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                containing_inline_size,
                &mut contribution,
            );
        }
        contribution
    }

    pub(in crate::layout::flex) fn merge_box_children_flex_intrinsic_widths(
        &mut self,
        child_boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        containing_inline_size: LogicalInlineContentSize,
        contribution: &mut inline_layout::InlineIntrinsicContribution,
    ) {
        for child_box in child_boxes {
            let Some((child_element, signature, child_style, child_children)) =
                child_box.element_parts()
            else {
                continue;
            };
            if child_style.display.is_none()
                || matches!(child_style.position, Position::Absolute | Position::Fixed)
                || child_style.display.is_inline_level()
            {
                continue;
            }
            let child_contribution = if table_width_depends_on_percentage_basis(child_style) {
                self.with_ancestor_signature(signature.clone(), |layout| {
                    let fragment = box_tree::build_frozen_table_fragment(
                        child_element,
                        signature,
                        child_style,
                        child_children,
                    );
                    let (min_content, _) = layout
                        .table_parent_intrinsic_content_widths_with_indefinite_percentage_basis(
                            child_element,
                            child_style,
                            stylesheets,
                            &fragment,
                            containing_inline_size.points(),
                        );
                    let (_, max_content) = layout
                        .table_parent_intrinsic_content_widths_from_fragment(
                            child_element,
                            child_style,
                            stylesheets,
                            &fragment,
                            containing_inline_size.points(),
                        );
                    inline_layout::InlineIntrinsicContribution::new(
                        LogicalInlineContentSize::new(content_box_pt(min_content)),
                        LogicalInlineContentSize::new(content_box_pt(max_content)),
                    )
                })
            } else {
                explicit_child_intrinsic_width(child_style, containing_inline_size).unwrap_or_else(
                    || {
                        self.with_ancestor_signature(signature.clone(), |layout| {
                            let (min_content, max_content) = layout.block_intrinsic_content_widths(
                                child_element,
                                child_style,
                                stylesheets,
                                Some(child_children),
                                containing_inline_size.points(),
                            );
                            inline_layout::InlineIntrinsicContribution::new(
                                LogicalInlineContentSize::new(content_box_pt(min_content)),
                                LogicalInlineContentSize::new(content_box_pt(max_content)),
                            )
                        })
                    },
                )
            };
            merge_outer_intrinsic_widths(
                contribution,
                constrain_non_table_child_intrinsic_width(
                    child_style,
                    containing_inline_size,
                    child_contribution,
                ),
                child_style,
                containing_inline_size,
            );
        }
    }

    pub(in crate::layout::flex) fn merge_dom_children_flex_intrinsic_widths(
        &mut self,
        element: &Element,
        parent_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        containing_inline_size: LogicalInlineContentSize,
        contribution: &mut inline_layout::InlineIntrinsicContribution,
    ) {
        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        for node in &element.children {
            let NodeKind::Element(child_element) = &node.kind else {
                continue;
            };
            let signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                signature.clone(),
                stylesheets,
                Some(parent_style),
            );
            if child_style.display.is_none()
                || matches!(child_style.position, Position::Absolute | Position::Fixed)
                || child_style.display.is_inline_level()
            {
                continue;
            }
            let child_contribution = if table_width_depends_on_percentage_basis(&child_style) {
                self.with_ancestor_signature(signature.clone(), |layout| {
                    let child_boxes = layout.build_frozen_child_boxes_with_current_ancestors(
                        child_element,
                        stylesheets,
                        &child_style,
                    );
                    let fragment = box_tree::build_frozen_table_fragment(
                        child_element,
                        &signature,
                        &child_style,
                        &child_boxes,
                    );
                    let (min_content, _) = layout
                        .table_parent_intrinsic_content_widths_with_indefinite_percentage_basis(
                            child_element,
                            &child_style,
                            stylesheets,
                            &fragment,
                            containing_inline_size.points(),
                        );
                    let (_, max_content) = layout
                        .table_parent_intrinsic_content_widths_from_fragment(
                            child_element,
                            &child_style,
                            stylesheets,
                            &fragment,
                            containing_inline_size.points(),
                        );
                    inline_layout::InlineIntrinsicContribution::new(
                        LogicalInlineContentSize::new(content_box_pt(min_content)),
                        LogicalInlineContentSize::new(content_box_pt(max_content)),
                    )
                })
            } else {
                explicit_child_intrinsic_width(&child_style, containing_inline_size).unwrap_or_else(
                    || {
                        self.with_ancestor_signature(signature, |layout| {
                            let (min_content, max_content) = layout.block_intrinsic_content_widths(
                                child_element,
                                &child_style,
                                stylesheets,
                                None,
                                containing_inline_size.points(),
                            );
                            inline_layout::InlineIntrinsicContribution::new(
                                LogicalInlineContentSize::new(content_box_pt(min_content)),
                                LogicalInlineContentSize::new(content_box_pt(max_content)),
                            )
                        })
                    },
                )
            };
            merge_outer_intrinsic_widths(
                contribution,
                constrain_non_table_child_intrinsic_width(
                    &child_style,
                    containing_inline_size,
                    child_contribution,
                ),
                &child_style,
                containing_inline_size,
            );
        }
    }

    /// Estimates the min-content block-size contribution from descendant boxes.
    ///
    /// CSS Sizing defines min-content sizes as intrinsic contributions, and CSS
    /// Flexbox uses those contributions when resolving intrinsic flex item size
    /// constraints such as `max-height: min-content`:
    /// <https://www.w3.org/TR/css-sizing-3/#min-content> and
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
    pub(in crate::layout::flex) fn estimate_child_min_content_block_size(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &Stylesheets<'_>,
        containing_inline_size: LogicalInlineContentSize,
        inline_content_height: LogicalBlockContentSize,
    ) -> LogicalBlockContentSize {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return inline_content_height;
        };
        self.with_ancestor_signature(signature.clone(), |layout| {
            let containing_height_basis = layout
                .block_percentage_context_stack
                .current_percentage_basis();
            layout.estimate_element_children_min_content_block_size(
                element,
                &child.style,
                stylesheets,
                child_boxes,
                FlexMinContentBlockContainingSpace {
                    inline_size: containing_inline_size,
                    height_percentage_basis: containing_height_basis,
                },
                inline_content_height,
            )
        })
    }

    /// Recursively estimates descendant min-content block-size contributions.
    ///
    /// CSS Sizing's block-axis min-content contribution for normal block flow
    /// is the sum of in-flow block child outer sizes, with inline runs measured
    /// by graph-selected line fragments:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>,
    /// <https://www.w3.org/TR/css-inline-3/#line-box>, and
    /// <https://www.w3.org/TR/CSS22/visudet.html#normal-block>.
    pub(in crate::layout::flex) fn estimate_element_children_min_content_block_size(
        &mut self,
        element: &Element,
        parent_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_space: FlexMinContentBlockContainingSpace,
        inline_content_height: LogicalBlockContentSize,
    ) -> LogicalBlockContentSize {
        if used_property_containment(element, parent_style).size {
            return LogicalBlockContentSize::new(content_box_pt(0.0));
        }
        let mut block_size = FlexBlockStackContribution::from_content_size(inline_content_height);

        if let Some(child_boxes) = child_boxes {
            for child_box in child_boxes {
                let Some((child_element, signature, child_style, child_children)) =
                    child_box.element_parts()
                else {
                    continue;
                };
                if !flex_min_content_block_child_participates(
                    child_element,
                    child_style,
                    self.element_uses_document_canvas_flow(child_element),
                ) {
                    continue;
                }
                let child_contribution =
                    self.with_ancestor_signature(signature.clone(), |layout| {
                        layout.flex_child_outer_min_content_block_size(
                            child_element,
                            child_style,
                            stylesheets,
                            Some(child_children),
                            containing_space.inline_size,
                            containing_space.height_percentage_basis,
                        )
                    });
                block_size = block_size.plus(child_contribution);
            }
            return block_size.as_content_size();
        }

        let sibling_tags = element_sibling_signature_list(element);
        let mut element_index = 0usize;
        for node in &element.children {
            let NodeKind::Element(child_element) = &node.kind else {
                continue;
            };
            let signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                signature.clone(),
                stylesheets,
                Some(parent_style),
            );
            if !flex_min_content_block_child_participates(
                child_element,
                &child_style,
                self.element_uses_document_canvas_flow(child_element),
            ) {
                continue;
            }
            let child_contribution = self.with_ancestor_signature(signature, |layout| {
                layout.flex_child_outer_min_content_block_size(
                    child_element,
                    &child_style,
                    stylesheets,
                    None,
                    containing_space.inline_size,
                    containing_space.height_percentage_basis,
                )
            });
            block_size = block_size.plus(child_contribution);
        }
        block_size.as_content_size()
    }

    fn flex_child_outer_min_content_block_size(
        &mut self,
        child_element: &Element,
        child_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_inline_size: LogicalInlineContentSize,
        containing_height_basis: BlockSizePercentageBasis,
    ) -> FlexBlockStackContribution {
        // Flex intrinsic estimation can visit a nested block before its
        // ordinary formatting-context replay. Resolve viewport units at this
        // page-context boundary so an explicit `height: 350vh` contributes
        // its used monolithic extent instead of falling through to a zero
        // intrinsic size.
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        // <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
        let child_style = self.style_with_current_viewport_lengths(child_style);
        let child_inline_size = used_content_box_width_or_auto(
            &child_style,
            layout_pt(containing_inline_size.points()),
            intrinsic_horizontal_non_content(&child_style, containing_inline_size),
        )
        .map(LogicalInlineContentSize::new)
        .unwrap_or(containing_inline_size);
        if let Some(replaced) = resolve_replaced_element(
            child_element,
            &child_style,
            ReplacedBoxSizingContext {
                available_width: containing_inline_size.content_box_length(),
                inline_percentage_basis: PercentageBasis::definite_from(
                    containing_inline_size.content_box_length(),
                    IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                ),
                block_basis: IntrinsicBlockBasis::from_layout_percentage_basis(
                    containing_height_basis,
                ),
            },
            self.base_url,
            self.root_url,
            self.resource_cache,
        ) {
            let geometry = replaced.geometry();
            return FlexBlockStackContribution::from_outer_extent(layout_pt(
                child_style.margin.top
                    + geometry.border_box_size.height
                    + child_style.margin.bottom,
            ));
        }
        let vertical_non_content = non_content_pt(
            child_style.padding.top
                + child_style.padding.bottom
                + vertical_border_width(&child_style),
        );
        let content_size = used_content_box_height_or_auto_with_basis(
            &child_style,
            containing_height_basis,
            vertical_non_content,
        )
        .unwrap_or_else(|| {
            if used_property_containment(child_element, &child_style).size {
                // Size containment makes only an automatic intrinsic content
                // contribution empty. A specified block size still resolves
                // above, and this common constraint path still applies the
                // child's min/max size and box-model edges.
                // <https://www.w3.org/TR/css-contain-2/#size-containment>
                return content_box_pt(0.0);
            }
            let inline_width =
                (child_inline_size.points() - child_style.padding.left - child_style.padding.right)
                    .max(1.0);
            let inline_measurement = self.intrinsic_inline_measurement_for_element(
                child_element,
                &child_style,
                stylesheets,
                child_boxes,
                inline_width,
            );
            self.estimate_element_children_min_content_block_size(
                child_element,
                &child_style,
                stylesheets,
                child_boxes,
                FlexMinContentBlockContainingSpace {
                    inline_size: child_inline_size,
                    height_percentage_basis: PercentageBasis::indefinite(),
                },
                LogicalBlockContentSize::new(content_box_pt(
                    inline_measurement.logical_block_span(&child_style),
                )),
            )
            .content_box_length()
        });
        let constrained_content_size = constrain_flex_item_estimated_height(
            &child_style,
            content_size,
            content_size,
            content_size,
            containing_height_basis,
            vertical_non_content,
        );
        let border_widths = used_border_widths(&child_style);
        FlexBlockStackContribution::from_outer_extent(layout_pt(
            child_style.margin.top
                + child_style.padding.top
                + border_widths.top
                + constrained_content_size.points()
                + child_style.padding.bottom
                + border_widths.bottom
                + child_style.margin.bottom,
        ))
    }

    /// Measure an auto-height flex item as block layout when its final
    /// normal-flow block stack determines a Flexbox cross contribution.
    ///
    /// CSS Flexbox computes `flex-basis:auto` from the item's max-content size
    /// when the main-size property is automatic. For a column flex item, that
    /// means the content block size after laying out floats in the available
    /// inline size, not just the sum of each float's standalone block size:
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item> and
    /// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
    pub(in crate::layout::flex) fn measure_flex_item_auto_block_height_for_flex_basis(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        content_width: PhysicalContentWidth,
        measurement: FlexAutoBlockHeightMeasurement,
    ) -> Option<PhysicalContentHeight> {
        let flow_root_measurement = style.display.is_block_level()
            && matches!(
                style.display.inner,
                DisplayInner::Flow | DisplayInner::FlowRoot
            );
        let table_measurement = style.display.is_table();
        if !style.box_values.height.is_auto()
            || !(flow_root_measurement || table_measurement)
            || (!measurement.is_post_flexing_main_size() && measurement.inline_line_count > 0)
        {
            return None;
        }
        if !measurement.is_post_flexing_main_size()
            && !child_boxes
                .map(flex_item_child_boxes_include_float)
                .unwrap_or(false)
        {
            return None;
        }

        let mut measured_style = style.clone();
        // Fragmentation transitions do not contribute to a flex item's
        // hypothetical cross size.  The Flexbox sizing algorithm measures the
        // item's normal-flow used size before its later pagination replay
        // honors `break-before`/`break-after`; retaining a forced transition
        // here would turn one page area into intrinsic content height and
        // inflate every line containing the item.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
        // <https://www.w3.org/TR/css-break-3/#breaking-controls>
        measured_style.break_before = PageBreak::Auto;
        measured_style.break_after = PageBreak::Auto;
        if flow_root_measurement {
            measured_style.display.inner = DisplayInner::FlowRoot;
        }
        // The probe returns the item's content-box block size, while flex
        // line sizing adds the item's cross-axis margins separately. Keep the
        // original margins out of this surrogate measurement; otherwise the
        // block layout cursor consumes them and reports an outer height as a
        // content height, causing every later line remeasurement to add them
        // a second time.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line> and
        // <https://www.w3.org/TR/css-box-3/#margin-terms>.
        measured_style.margin = css::Edges::ZERO;
        measured_style.box_values.margin =
            css::PhysicalEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
        // `content_width` is a used flex-item content-box width. Freeze it
        // on the measurement surrogate before using that same width as its
        // containing block; otherwise an authored percentage width would be
        // resolved again against itself during the probe.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        set_style_used_width(&mut measured_style, content_width.points());
        measured_style.box_sizing = BoxSizing::ContentBox;
        // `measure_auto_positioned_block_height` establishes the supplied
        // width as this box's containing block. `content_width` is also the
        // already-resolved content-box width of this measurement surrogate.
        // <https://www.w3.org/TR/css-box-3/#box-model>
        let measurement_space = PositionedAutoBlockMeasurementSpace {
            content_width,
            available_physical_height: self
                .current_child_available_space()
                .available_physical_height(),
        };
        Some(PhysicalContentHeight::new(
            self.measure_auto_positioned_block_height(
                element,
                &measured_style,
                stylesheets,
                measurement_space,
                child_boxes,
                None,
            ),
        ))
    }
}

/// Why an auto-height flex item needs block-layout measurement.
///
/// Post-flexing main-size remeasurement is allowed to replace an item's
/// ordinary hypothetical cross contribution, whereas intrinsic flex-base
/// measurement keeps the cheaper inline estimate unless float layout requires
/// a block formatting context: <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexAutoBlockHeightMeasurementPurpose {
    IntrinsicFlexBase,
    PostFlexingMainSize,
}

/// The line-measurement state paired with the semantic reason for measuring a
/// flex item's automatic block size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) struct FlexAutoBlockHeightMeasurement {
    pub(in crate::layout::flex) inline_line_count: usize,
    pub(in crate::layout::flex) purpose: FlexAutoBlockHeightMeasurementPurpose,
}

impl FlexAutoBlockHeightMeasurement {
    pub(in crate::layout::flex) fn is_post_flexing_main_size(self) -> bool {
        self.purpose == FlexAutoBlockHeightMeasurementPurpose::PostFlexingMainSize
    }
}

pub(in crate::layout::flex) fn explicit_child_intrinsic_width(
    child_style: &ComputedStyle,
    containing_inline_size: LogicalInlineContentSize,
) -> Option<inline_layout::InlineIntrinsicContribution> {
    let horizontal_extras = intrinsic_horizontal_non_content(child_style, containing_inline_size);
    used_content_box_width_or_auto(
        child_style,
        layout_pt(containing_inline_size.points()),
        horizontal_extras,
    )
    .map(SemanticLengthExt::points)
    .map(|width| {
        let width = LogicalInlineContentSize::new(content_box_pt(width));
        inline_layout::InlineIntrinsicContribution::new(width, width)
    })
}

/// Apply a block child's own intrinsic inline-size constraints before it
/// becomes a flex item's content contribution.
///
/// `block_intrinsic_content_widths` returns the child's raw descendant
/// contribution because its caller normally owns the child's box-model and
/// `min-width`/`max-width` constraints. Flex's direct descendant query is
/// that caller. Without this transition, `min-width: max-content` on a block
/// inside a flex item is lost and the item's automatic minimum collapses to
/// the raw min-content width.
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
fn constrain_non_table_child_intrinsic_width(
    child_style: &ComputedStyle,
    containing_inline_size: LogicalInlineContentSize,
    contribution: inline_layout::InlineIntrinsicContribution,
) -> inline_layout::InlineIntrinsicContribution {
    // The table branch has already constructed the CSS Tables wrapper
    // contribution, including its own constraints. Re-applying generic block
    // constraints would erase that table-specific sizing result.
    if child_style.display.is_table() {
        return contribution;
    }
    let horizontal_non_content =
        intrinsic_horizontal_non_content(child_style, containing_inline_size);
    let (min_content, max_content) = non_replaced_intrinsic_width_contributions(
        child_style,
        contribution.min_content.content_box_length(),
        contribution.max_content.content_box_length(),
        horizontal_non_content,
    );
    inline_layout::InlineIntrinsicContribution::new(
        LogicalInlineContentSize::new(min_content),
        LogicalInlineContentSize::new(max_content),
    )
}

/// Whether a table's preferred physical width depends on a percentage basis.
///
/// Flexbox asks for a table's intrinsic automatic minimum with an indefinite
/// basis, then resolves its preferred flex base against the definite flex
/// container main size. This predicate keeps those two table queries distinct:
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>.
pub(in crate::layout::flex) fn table_width_depends_on_percentage_basis(
    style: &ComputedStyle,
) -> bool {
    style.display.is_table()
        && matches!(
            &style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
                if value.contains_percentage()
        )
}

pub(in crate::layout::flex) fn intrinsic_horizontal_non_content(
    style: &ComputedStyle,
    containing_inline_size: LogicalInlineContentSize,
) -> NonContentLength {
    let padding = used_padding_edges_for_logical_inline_basis(
        style,
        PercentageBasis::definite(containing_inline_size),
    )
    .to_css_edges();
    non_content_pt(padding.left + padding.right + horizontal_border_width(style))
}

pub(in crate::layout::flex) fn intrinsic_horizontal_outer_edges(
    style: &ComputedStyle,
    containing_inline_size: LogicalInlineContentSize,
) -> FlexIntrinsicInlineOuterExtras {
    let metrics = intrinsic_box_metrics(style);
    FlexIntrinsicInlineOuterExtras::new(layout_pt(
        intrinsic_horizontal_non_content(style, containing_inline_size).points()
            + metrics.margin.left.points()
            + metrics.margin.right.points(),
    ))
}

pub(in crate::layout::flex) fn flex_min_content_block_child_participates(
    _element: &Element,
    style: &ComputedStyle,
    is_document_canvas_flow_element: bool,
) -> bool {
    !style.display.is_none()
        && !matches!(style.position, Position::Absolute | Position::Fixed)
        // Inline-level replaced elements (including `<br>`) already
        // participate in the graph-selected inline line stack passed as
        // `inline_content_height`. Adding them again here treats each inline
        // line as a block child and double-counts its logical block advance,
        // particularly in vertical writing modes.
        // <https://www.w3.org/TR/css-display-3/#inlinify>
        && (style.display.is_block_level() || is_document_canvas_flow_element)
}

pub(in crate::layout::flex) fn flex_item_child_boxes_include_float(
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    child_boxes.iter().any(|child_box| {
        if let Some((_, _, child_style, child_children)) = child_box.element_parts() {
            if !matches!(child_style.position, Position::Absolute | Position::Fixed)
                && child_style.float != Float::None
            {
                return true;
            }
            return flex_item_child_boxes_include_float(child_children);
        }
        match child_box {
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                flex_item_child_boxes_include_float(&box_.children)
            }
            _ => false,
        }
    })
}

/// Return a flex available-space record with a definite cross-axis size.
///
/// CSS Flexbox max-content cross sizing for multi-line column containers lays
/// out each item with the largest max-content cross contribution as its
/// available cross size:
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-cross-sizes>.
pub(in crate::layout::flex) fn flex_available_with_definite_cross_size(
    available: FlexAvailableSpace,
    direction: FlexDirection,
    cross_size: FlexCrossSize,
) -> FlexAvailableSpace {
    available.with_definite_cross_size(direction, cross_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_child_max_content_minimum_is_preserved_for_flex_automatic_minimum() {
        let mut child_style = ComputedStyle::initial();
        child_style.box_values.min_width = css::ComputedLengthPercentageOrAuto::MaxContent;
        let contribution = inline_layout::InlineIntrinsicContribution::new(
            LogicalInlineContentSize::new(content_box_pt(20.0)),
            LogicalInlineContentSize::new(content_box_pt(80.0)),
        );

        let constrained = constrain_non_table_child_intrinsic_width(
            &child_style,
            LogicalInlineContentSize::new(content_box_pt(100.0)),
            contribution,
        );

        assert_eq!(constrained.min_content.points(), 80.0);
        assert_eq!(constrained.max_content.points(), 80.0);
    }
}
