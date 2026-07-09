use super::*;
use crate::layout::flex::compute::estimated_outer_cross_size;

/// The physical containing space available to an intrinsic block-size walk
/// below a flex item.
///
/// Width remains a physical constraint for inline measurement, while the
/// block-size percentage basis independently controls whether descendant
/// `height` percentages resolve:
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexMinContentBlockContainingSpace {
    pub(in crate::layout::flex) width: PhysicalContentWidth,
    pub(in crate::layout::flex) height_percentage_basis: BlockSizePercentageBasis,
}

#[derive(Debug, Clone, Copy)]
struct FlexItemPhysicalIntrinsicSizes {
    /// Intrinsic values after CSS Writing Modes has projected them onto the
    /// physical axes consumed by flex layout.
    preferred_width: PhysicalContentWidth,
    preferred_min_width: PhysicalContentWidth,
    intrinsic_content_height: PhysicalContentHeight,
    min_content_height: PhysicalContentHeight,
}

/// Apply a flex item's block-axis constraints without inventing a percentage
/// basis from its physical inline size.
///
/// Percentage `min-height` and `max-height` resolve against the containing
/// block height. During intrinsic flex sizing that basis is often indefinite,
/// in which case cyclic percentages must remain unresolved rather than using
/// the item's available width:
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>.
pub(in crate::layout::flex) fn constrain_flex_item_estimated_height<Source>(
    style: &ComputedStyle,
    value: f32,
    min_content: f32,
    max_content: f32,
    percentage_basis: PercentageBasis<ContentBoxLength, Source>,
    vertical_non_content: f32,
) -> ContentBoxLength {
    constrain_height_with_intrinsic(
        style,
        content_box_pt(value),
        content_box_pt(min_content),
        content_box_pt(max_content),
        percentage_basis,
        non_content_pt(vertical_non_content),
    )
}

/// Project a flex item's logical intrinsic sizes into Quire's physical axes.
///
/// CSS Writing Modes maps inline size to physical height in vertical writing
/// modes, while block size maps to physical width. Flexbox consumes those
/// physical sizes when resolving flex base sizes and hypothetical cross sizes:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>.
fn flex_item_physical_intrinsic_sizes(
    writing_mode: WritingMode,
    logical_inline_size: f32,
    logical_min_inline_size: f32,
    logical_block_size: f32,
) -> FlexItemPhysicalIntrinsicSizes {
    if !WritingModeAxes::new(writing_mode, Direction::Ltr).swaps_physical_axes() {
        FlexItemPhysicalIntrinsicSizes {
            preferred_width: PhysicalContentWidth::new(content_box_pt(logical_inline_size)),
            preferred_min_width: PhysicalContentWidth::new(content_box_pt(logical_min_inline_size)),
            intrinsic_content_height: PhysicalContentHeight::new(content_box_pt(
                logical_block_size,
            )),
            min_content_height: PhysicalContentHeight::new(content_box_pt(
                logical_block_size.max(0.0),
            )),
        }
    } else {
        FlexItemPhysicalIntrinsicSizes {
            preferred_width: PhysicalContentWidth::new(content_box_pt(logical_block_size)),
            preferred_min_width: PhysicalContentWidth::new(content_box_pt(
                logical_block_size.max(0.0),
            )),
            intrinsic_content_height: PhysicalContentHeight::new(content_box_pt(
                logical_inline_size,
            )),
            min_content_height: PhysicalContentHeight::new(content_box_pt(
                logical_min_inline_size.max(0.0),
            )),
        }
    }
}

/// Whether a content-based flex basis is resolved along the item's logical
/// inline axis.
///
/// In that case CSS Flexbox lays the item out at its max-content flex base
/// before deriving its hypothetical cross size. Measuring its line boxes at a
/// narrower container width would introduce soft wraps that do not exist in
/// the used flex item.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
/// <https://www.w3.org/TR/css-sizing-3/#max-content>
fn flex_basis_uses_content_inline_size(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> bool {
    let logical_inline_is_main = match WritingModeAxes::new(style.writing_mode, style.direction)
        .physical_axis(LogicalAxis::Inline)
    {
        PhysicalAxis::Horizontal => physical_direction.is_row_axis(),
        PhysicalAxis::Vertical => physical_direction.is_column_axis(),
    };
    if !logical_inline_is_main {
        return false;
    }
    match style.flex_basis {
        css::ComputedFlexBasis::Content | css::ComputedFlexBasis::MaxContent => true,
        css::ComputedFlexBasis::Auto => {
            if physical_direction.is_row_axis() {
                style.box_values.width.is_auto()
            } else {
                style.box_values.height.is_auto()
            }
        }
        css::ComputedFlexBasis::LengthPercentage(_)
        | css::ComputedFlexBasis::MinContent
        | css::ComputedFlexBasis::FitContent(_) => false,
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Estimates the hypothetical content size of a flex item.
    ///
    /// CSS Flexbox defines flex base sizes and intrinsic contributions before
    /// the flex formatting algorithm distributes free space. Inline
    /// contributions are measured through `InlineOpportunityGraph` so flex
    /// estimates use the same CSS Text break opportunities as inline layout:
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>,
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>, and
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>.
    pub(in crate::layout::flex) fn estimate_flex_item_size(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        available: FlexItemAvailableSpace,
        physical_direction: FlexDirection,
    ) -> FlexItemEstimate {
        let percentage_height_basis =
            flex_item_estimate_percentage_height_basis(&child.style, available);
        let mut estimate =
            self.with_flex_item_percentage_height_basis(percentage_height_basis, |layout| {
                layout.estimate_flex_item_size_with_percentage_basis(
                    child,
                    stylesheets,
                    available,
                    physical_direction,
                )
            });
        if child.style.contain.layout {
            // A layout-contained principal box exports no first/last baseline;
            // its flex/grid parent must use the synthesized fallback from the
            // border box instead.
            // <https://www.w3.org/TR/css-contain-1/#containment-layout>
            estimate.metrics.clear_block_baselines();
            estimate.first_horizontal_baseline = None;
            estimate.last_horizontal_baseline = None;
        }
        estimate
    }

    pub(in crate::layout::flex) fn estimate_flex_item_size_with_percentage_basis(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        available: FlexItemAvailableSpace,
        physical_direction: FlexDirection,
    ) -> FlexItemEstimate {
        // Flex keeps the child's source style for descendant cascade. This
        // intrinsic multicol probe is a layout consumer, so use a separate
        // normalized multicol style for its tracks and balancing geometry.
        let multicol_style = self.multicol_used_style(&child.style);
        let style = &multicol_style;
        let containing_width = available.width.points();
        let containing_width_basis = available.width_basis;
        let containing_height_basis = available.height_basis;
        let containing_inline_size = available.inline_size(style).points();
        let containing_inline_basis = available.inline_basis(style);
        let preferred_inline_basis = if containing_inline_basis.is_definite() {
            containing_inline_basis
        } else {
            containing_width_basis
        };
        let vertical_non_content =
            style.padding.top + style.padding.bottom + vertical_border_width(style);
        if let Some(children) = child.anonymous_content() {
            let measurement = self.intrinsic_inline_measurement_for_boxes(
                children,
                style,
                stylesheets,
                containing_inline_size.max(1.0),
            );
            let contribution = measurement.contribution;
            let logical_inline_size = contribution.max_content.max(style.font_size * 0.25);
            let logical_min_inline_size = contribution.min_content.max(style.font_size * 0.25);
            let used_line_height = self.font_system.used_line_height(style).points();
            let logical_block_size = measurement.height().max(used_line_height);
            let physical_intrinsic = flex_item_physical_intrinsic_sizes(
                style.writing_mode,
                logical_inline_size,
                logical_min_inline_size,
                logical_block_size,
            );
            let content_width = used_length_percentage_or_auto_with_basis(
                style.box_values.width.clone(),
                containing_width_basis,
            )
            .map(|width| width.points())
            .unwrap_or(physical_intrinsic.preferred_width.points());
            let content_height = used_content_box_height_or_auto_with_basis(
                style,
                containing_height_basis,
                non_content_pt(vertical_non_content),
            )
            .map(SemanticLengthExt::points)
            .unwrap_or(physical_intrinsic.intrinsic_content_height.points());
            let width = constrain_content_width(
                style,
                content_box_pt(content_width),
                PercentageBasis::definite(layout_pt(containing_width)),
            )
            .points();
            let height = constrain_flex_item_estimated_height(
                style,
                content_height,
                physical_intrinsic.min_content_height.points(),
                physical_intrinsic.intrinsic_content_height.points(),
                containing_height_basis,
                vertical_non_content,
            );
            let min_width = constrain_content_width(
                style,
                physical_intrinsic.preferred_min_width.content_box_length(),
                PercentageBasis::definite(layout_pt(containing_width)),
            )
            .points();
            let min_height = constrain_flex_item_estimated_height(
                style,
                physical_intrinsic.min_content_height.points(),
                physical_intrinsic.min_content_height.points(),
                physical_intrinsic.intrinsic_content_height.points(),
                containing_height_basis,
                vertical_non_content,
            );
            let fallback_line_baseline_offset =
                self.inline_box_text_line_layout_baseline_offset(style);
            let first_line_baseline_offset = first_sequence_line_baseline_offset(
                &measurement.sequence,
                fallback_line_baseline_offset,
            );
            let last_line_baseline_offset = last_sequence_line_baseline_offset(
                &measurement.sequence,
                fallback_line_baseline_offset,
            );
            let preceding_line_height = preceding_line_height_before_last(&measurement.sequence);
            let baseline_edge = used_border_widths(style).top + style.padding.top;
            let first_baseline = baseline_edge + first_line_baseline_offset;
            let last_baseline = baseline_edge
                + measurement
                    .sequence
                    .last_line_baseline_offset(fallback_line_baseline_offset);
            return FlexItemEstimate {
                metrics: IntrinsicItemMetrics {
                    width: content_box_pt(width),
                    height,
                    min_width: content_box_pt(min_width),
                    min_height,
                    content_width: physical_intrinsic.preferred_width.content_box_length(),
                    content_height: physical_intrinsic
                        .intrinsic_content_height
                        .content_box_length(),
                    preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                    first_baseline: Some(first_baseline),
                    last_baseline: Some(last_baseline),
                },
                first_horizontal_baseline: first_horizontal_text_baseline_offset(
                    style,
                    width,
                    first_line_baseline_offset,
                ),
                last_horizontal_baseline: last_horizontal_text_baseline_offset(
                    style,
                    width,
                    preceding_line_height,
                    last_line_baseline_offset,
                ),
            };
        }

        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return FlexItemEstimate::fixed(0.0, 0.0);
        };
        if style.contain.size && replaced_element_kind(element).is_none() {
            // Size-contained flex items participate in flex layout with their
            // principal box sized as if empty. Their descendants are still
            // laid out in-place and may still supply baselines.
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            let fallback_width = style
                .contain_intrinsic_size
                .width
                .clone()
                .map(|width| {
                    used_length_percentage(
                        width,
                        PercentageBasis::definite(layout_pt(containing_width.max(0.0))),
                    )
                    .points()
                })
                .unwrap_or(0.0);
            let fallback_height = style
                .contain_intrinsic_size
                .height
                .clone()
                .map(|height| {
                    used_length_percentage(
                        height,
                        PercentageBasis::definite(layout_pt(containing_width.max(0.0))),
                    )
                    .points()
                })
                .unwrap_or(0.0);
            let mut content_width = used_length_percentage_or_auto_with_basis(
                style.box_values.width.clone(),
                containing_width_basis,
            )
            .map(|width| width.points())
            .unwrap_or(fallback_width);
            let mut content_height = used_content_box_height_or_auto_with_basis(
                style,
                containing_height_basis,
                non_content_pt(vertical_non_content),
            )
            .map(SemanticLengthExt::points)
            .unwrap_or(fallback_height);
            if let Some(ratio) = style.aspect_ratio.preferred_ratio_for_non_replaced(false) {
                match (
                    style.box_values.width.is_auto(),
                    style.box_values.height.is_auto(),
                ) {
                    (false, true) => content_height = content_width / ratio,
                    (true, false) => content_width = content_height * ratio,
                    _ => {}
                }
            }
            let width = constrain_content_width(
                style,
                content_box_pt(content_width),
                PercentageBasis::definite(layout_pt(containing_width)),
            )
            .points();
            let height = constrain_flex_item_estimated_height(
                style,
                content_height,
                0.0,
                0.0,
                containing_height_basis,
                vertical_non_content,
            );
            let descendant_baselines = self.estimate_flex_item_descendant_baselines(
                element,
                signature,
                style,
                child_boxes,
                stylesheets,
                containing_width,
            );
            return FlexItemEstimate {
                metrics: IntrinsicItemMetrics {
                    width: content_box_pt(width),
                    height,
                    min_width: constrain_content_width(
                        style,
                        content_box_pt(fallback_width),
                        PercentageBasis::definite(layout_pt(containing_width)),
                    ),
                    min_height: constrain_flex_item_estimated_height(
                        style,
                        fallback_height,
                        fallback_height,
                        fallback_height,
                        containing_height_basis,
                        vertical_non_content,
                    ),
                    content_width: content_box_pt(fallback_width),
                    content_height: content_box_pt(fallback_height),
                    preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                    first_baseline: descendant_baselines.first_baseline,
                    last_baseline: descendant_baselines.last_baseline,
                },
                first_horizontal_baseline: descendant_baselines.first_horizontal_baseline,
                last_horizontal_baseline: descendant_baselines.last_horizontal_baseline,
            };
        }
        if style.display.is_flex() {
            let intrinsic_size = self.with_ancestor_signature(signature.clone(), |layout| {
                let built_child_boxes;
                let child_boxes = if let Some(child_boxes) = child_boxes {
                    child_boxes
                } else {
                    built_child_boxes = layout.build_frozen_child_boxes_with_current_ancestors(
                        element,
                        stylesheets,
                        style,
                    );
                    &built_child_boxes
                };
                let children = flex_children_from_boxes(element, signature, style, child_boxes);
                (!children.is_empty()).then(|| {
                    let intrinsic_height = used_content_box_height_or_auto_with_basis(
                        style,
                        containing_height_basis,
                        non_content_pt(vertical_non_content),
                    );
                    layout.estimate_intrinsic_flex_container_size(
                        &children,
                        style,
                        stylesheets,
                        FlexAvailableSpace {
                            width: PhysicalContentWidth::new(content_box_pt(containing_width)),
                            width_basis: flex_available_percentage_basis_from_points(
                                used_length_percentage_or_auto_with_basis(
                                    style.box_values.width.clone(),
                                    containing_width_basis,
                                )
                                .map(|width| width.points())
                                .map(|_| containing_width),
                                FlexAvailableSizeSource::IntrinsicContainerSize,
                            ),
                            height: intrinsic_height.map(PhysicalContentHeight::new),
                            height_basis: flex_available_percentage_basis_from_points(
                                intrinsic_height.map(SemanticLengthExt::points),
                                FlexAvailableSizeSource::IntrinsicContainerSize,
                            ),
                        },
                    )
                })
            });
            if let Some(intrinsic_size) = intrinsic_size {
                let content_width = used_length_percentage_or_auto_with_basis(
                    style.box_values.width.clone(),
                    containing_width_basis,
                )
                .map(|width| width.points())
                .unwrap_or_else(|| intrinsic_size.width.points())
                .max(style.font_size);
                let used_line_height = self.font_system.used_line_height(style).points();
                let content_height = used_content_box_height_or_auto_with_basis(
                    style,
                    containing_height_basis,
                    non_content_pt(vertical_non_content),
                )
                .map(SemanticLengthExt::points)
                .unwrap_or_else(|| intrinsic_size.height.points())
                .max(used_line_height);
                return FlexItemEstimate {
                    metrics: IntrinsicItemMetrics {
                        width: constrain_content_width(
                            style,
                            content_box_pt(content_width),
                            PercentageBasis::definite(layout_pt(containing_width)),
                        ),
                        height: constrain_content_height(
                            style,
                            content_box_pt(content_height),
                            PercentageBasis::definite(layout_pt(containing_width)),
                        ),
                        min_width: constrain_content_width(
                            style,
                            intrinsic_size.min_width,
                            PercentageBasis::definite(layout_pt(containing_width)),
                        ),
                        min_height: constrain_content_height(
                            style,
                            intrinsic_size.min_height,
                            PercentageBasis::definite(layout_pt(containing_width)),
                        ),
                        content_width: intrinsic_size.content_width,
                        content_height: intrinsic_size.content_height,
                        preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                        first_baseline: intrinsic_size.first_baseline,
                        last_baseline: intrinsic_size.last_baseline,
                    },
                    first_horizontal_baseline: intrinsic_size.first_horizontal_baseline,
                    last_horizontal_baseline: intrinsic_size.last_horizontal_baseline,
                };
            }
        }

        if style.display.inner == DisplayInner::Grid {
            let grid_intrinsic = self.with_ancestor_signature(signature.clone(), |layout| {
                layout.estimate_grid_container_for_flex_item(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    containing_width,
                    crate::layout::grid::grid_percentage_basis_from_points(
                        containing_width_basis.points(),
                        crate::layout::grid::GridAvailableSizeSource::ContainerInlineSize,
                    ),
                    crate::layout::grid::grid_percentage_basis_from_points(
                        containing_height_basis.points(),
                        crate::layout::grid::GridAvailableSizeSource::ContainerBlockSize,
                    ),
                    vertical_non_content,
                )
            });
            if let Some(grid_intrinsic) = grid_intrinsic {
                let intrinsic_height = grid_intrinsic.intrinsic_height;
                let content_height = grid_intrinsic
                    .definite_content_height
                    .unwrap_or(intrinsic_height);
                let width = constrain_content_width(
                    style,
                    grid_intrinsic.content_width,
                    PercentageBasis::definite(layout_pt(containing_width)),
                )
                .points();
                let height = constrain_flex_item_estimated_height(
                    style,
                    content_height.points(),
                    intrinsic_height.points(),
                    intrinsic_height.points(),
                    containing_height_basis,
                    vertical_non_content,
                );
                let min_width = constrain_content_width(
                    style,
                    grid_intrinsic.min_width,
                    PercentageBasis::definite(layout_pt(containing_width)),
                )
                .points();
                let min_height = constrain_flex_item_estimated_height(
                    style,
                    intrinsic_height.points(),
                    intrinsic_height.points(),
                    intrinsic_height.points(),
                    containing_height_basis,
                    vertical_non_content,
                );
                return FlexItemEstimate {
                    metrics: IntrinsicItemMetrics {
                        width: content_box_pt(width),
                        height,
                        min_width: content_box_pt(min_width),
                        min_height,
                        content_width: grid_intrinsic.max_width,
                        content_height: intrinsic_height,
                        preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                        first_baseline: grid_intrinsic.first_baseline,
                        last_baseline: grid_intrinsic.last_baseline,
                    },
                    first_horizontal_baseline: None,
                    last_horizontal_baseline: None,
                };
            }
        }

        let replaced_intrinsic = match replaced_element_kind(element) {
            Some(ReplacedElementKind::Canvas) => Some(if element.tag == "iframe" {
                intrinsic_iframe_size(element)
            } else {
                intrinsic_canvas_size(element)
            }),
            Some(ReplacedElementKind::Image) => intrinsic_image_size(
                element,
                style,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|image| image.replaced_size())
            // A `<video>` without a poster still has a CSS replaced box. Its
            // media frame is unavailable to this static PDF renderer, but
            // Flexbox must use the HTML default object size for flex base and
            // automatic-minimum sizing rather than treating it as an empty
            // ordinary block.
            // <https://html.spec.whatwg.org/multipage/media.html#the-video-element>
            // <https://www.w3.org/TR/CSS22/visudet.html#inline-replaced-width>
            .or_else(|| (element.tag == "video").then(|| intrinsic_default_replaced_size(element))),
            Some(ReplacedElementKind::Svg) => intrinsic_svg_size(element),
            None => None,
        };
        if let Some(intrinsic) = replaced_intrinsic
            && let Some(size) =
                estimate_replaced_flex_item(intrinsic, style, containing_width, available)
        {
            return size;
        }

        if has_direct_inline_replaced_child(element)
            && !has_direct_flow_child_with_font_metrics(
                element,
                style,
                stylesheets,
                &mut self.font_system,
            )
        {
            let (row_width, row_height) =
                self.measure_direct_inline_row(element, style, stylesheets);
            let content_width = used_length_percentage_or_auto_with_basis(
                style.box_values.width.clone(),
                containing_width_basis,
            )
            .map(|width| width.points())
            .unwrap_or(row_width);
            let content_height = used_content_box_height_or_auto_with_basis(
                style,
                containing_height_basis,
                non_content_pt(vertical_non_content),
            )
            .map(SemanticLengthExt::points)
            .unwrap_or(row_height);
            let width = constrain_content_width(
                style,
                content_box_pt(content_width),
                PercentageBasis::definite(layout_pt(containing_width)),
            )
            .points();
            let height = constrain_flex_item_estimated_height(
                style,
                content_height,
                row_height,
                row_height,
                containing_height_basis,
                vertical_non_content,
            );
            let min_width = constrain_content_width(
                style,
                content_box_pt(row_width),
                PercentageBasis::definite(layout_pt(containing_width)),
            )
            .points();
            let min_height = constrain_flex_item_estimated_height(
                style,
                row_height,
                row_height,
                row_height,
                containing_height_basis,
                vertical_non_content,
            );
            let line_baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
            let first_baseline = used_border_widths(style).top
                + style.padding.top
                + self.inline_box_text_line_layout_baseline_offset(style);
            return FlexItemEstimate {
                metrics: IntrinsicItemMetrics {
                    width: content_box_pt(width),
                    height,
                    min_width: content_box_pt(min_width),
                    min_height,
                    content_width: content_box_pt(row_width),
                    content_height: content_box_pt(row_height),
                    preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                    first_baseline: Some(first_baseline),
                    last_baseline: Some(first_baseline),
                },
                first_horizontal_baseline: first_horizontal_text_baseline_offset(
                    style,
                    width,
                    line_baseline_offset,
                ),
                last_horizontal_baseline: last_horizontal_text_baseline_offset(
                    style,
                    width,
                    0.0,
                    line_baseline_offset,
                ),
            };
        }

        let definition_list_column_height =
            self.estimate_definition_list_column_height(child, stylesheets, containing_inline_size);
        let inline_content_width =
            (containing_inline_size - style.padding.left - style.padding.right).max(1.0);
        let inline_percentage_basis = if style.box_values.width.is_auto() {
            PercentageBasis::indefinite()
        } else {
            PercentageBasis::definite_from(
                content_box_pt(inline_content_width),
                IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
            )
        };
        let mut inline_measurement =
            self.with_intrinsic_inline_percentage_basis(inline_percentage_basis, |layout| {
                layout.estimate_child_inline_measurement(child, stylesheets, inline_content_width)
            });
        let child_intrinsic = self.estimate_child_intrinsic_widths(
            child,
            stylesheets,
            containing_inline_size,
            inline_measurement.contribution,
        );
        let content_basis_inline_width =
            flex_basis_uses_content_inline_size(style, physical_direction)
                && child_intrinsic.max_content > inline_content_width + 0.01;
        if content_basis_inline_width {
            inline_measurement =
                self.with_intrinsic_inline_percentage_basis(inline_percentage_basis, |layout| {
                    layout.estimate_child_inline_measurement(
                        child,
                        stylesheets,
                        child_intrinsic.max_content,
                    )
                });
        }
        let hypothetical_cross_measure_width = if content_basis_inline_width {
            child_intrinsic.max_content
        } else {
            containing_inline_size
        };
        let child_preferred_block_height = self.estimate_child_min_content_block_size(
            child,
            stylesheets,
            hypothetical_cross_measure_width,
            inline_measurement.height(),
        );

        let logical_inline_size = child_intrinsic.max_content;
        let logical_min_inline_size = child_intrinsic.min_content;
        let used_line_height = style.line_height;
        let fallback_logical_block_size = if inline_measurement.line_count() > 0 {
            inline_measurement
                .height()
                .max(inline_measurement.line_count() as f32 * used_line_height)
        } else if element.children.is_empty() && child_intrinsic.max_content == 0.0 {
            // CSS Flexbox determines the hypothetical cross size by laying the
            // item out as a block. A genuinely empty block has zero content
            // height, allowing align-content/stretch to distribute the flex
            // container's cross size across wrapped lines.
            // https://www.w3.org/TR/css-flexbox-1/#algo-cross-item
            0.0
        } else {
            used_line_height
        };
        let logical_block_size = definition_list_column_height
            .unwrap_or_else(|| fallback_logical_block_size.max(child_preferred_block_height));
        let mut physical_intrinsic = flex_item_physical_intrinsic_sizes(
            style.writing_mode,
            logical_inline_size,
            logical_min_inline_size,
            logical_block_size,
        );
        if (inline_measurement.line_count() == 0 || inline_measurement.line_count() > 1)
            && matches!(
                style.writing_mode,
                WritingMode::VerticalRl
                    | WritingMode::VerticalLr
                    | WritingMode::SidewaysRl
                    | WritingMode::SidewaysLr
            )
        {
            physical_intrinsic.preferred_width = PhysicalContentWidth::new(content_box_pt(
                physical_intrinsic
                    .preferred_width
                    .points()
                    .max(child_intrinsic.max_content),
            ));
            physical_intrinsic.preferred_min_width = PhysicalContentWidth::new(content_box_pt(
                physical_intrinsic
                    .preferred_min_width
                    .points()
                    .max(child_intrinsic.min_content),
            ));
        }
        let mut content_width = used_length_percentage_or_auto_with_basis(
            style.box_values.width.clone(),
            preferred_inline_basis,
        )
        .map(|width| width.points())
        .unwrap_or(physical_intrinsic.preferred_width.points());
        let block_height_probe_width = if style.box_values.width.is_auto()
            && matches!(style.writing_mode, WritingMode::HorizontalTb)
            && physical_direction.is_row_axis()
            && flex_basis_uses_content_inline_size(style, physical_direction)
        {
            // The hypothetical cross size follows a content-based flex base
            // size, not the flex container's constrained available width.
            // Measuring float clearance at that narrower width would wrap
            // independent floats into artificial rows and inflate the
            // item's block contribution.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
            content_width
        } else if style.box_values.width.is_auto() {
            containing_inline_size
        } else {
            content_width
        };
        if let Some(block_height) = self.measure_flex_item_auto_block_height_for_flex_basis(
            element,
            style,
            stylesheets,
            child_boxes,
            block_height_probe_width,
            inline_measurement.line_count(),
        ) && matches!(style.writing_mode, WritingMode::HorizontalTb)
        {
            physical_intrinsic.intrinsic_content_height =
                PhysicalContentHeight::new(content_box_pt(block_height));
            physical_intrinsic.min_content_height =
                PhysicalContentHeight::new(content_box_pt(block_height.max(0.0)));
        }
        if style.box_values.height.is_auto()
            && matches!(style.writing_mode, WritingMode::HorizontalTb)
            && let Some(multicol_height) = self.estimate_child_multicol_inline_height(
                child,
                stylesheets,
                constrain_content_width(
                    style,
                    content_box_pt(content_width),
                    PercentageBasis::definite(layout_pt(containing_width)),
                )
                .points(),
            )
        {
            physical_intrinsic.intrinsic_content_height =
                PhysicalContentHeight::new(content_box_pt(multicol_height));
            physical_intrinsic.min_content_height =
                PhysicalContentHeight::new(content_box_pt(multicol_height.max(0.0)));
        }
        let mut content_height = used_content_box_height_or_auto_with_basis(
            style,
            containing_height_basis,
            non_content_pt(vertical_non_content),
        )
        .map(SemanticLengthExt::points)
        .unwrap_or(physical_intrinsic.intrinsic_content_height.points());
        if let Some(ratio) = style.aspect_ratio.preferred_ratio_for_non_replaced(false) {
            match (
                style.box_values.width.is_auto(),
                style.box_values.height.is_auto(),
            ) {
                (false, true) => {
                    let transferred_height = content_width / ratio;
                    if inline_measurement.line_count() == 0 && element.children.is_empty() {
                        physical_intrinsic.intrinsic_content_height =
                            PhysicalContentHeight::new(content_box_pt(transferred_height));
                        physical_intrinsic.min_content_height =
                            PhysicalContentHeight::new(content_box_pt(transferred_height));
                        content_height = transferred_height;
                    } else {
                        content_height = content_height.max(transferred_height);
                        physical_intrinsic.min_content_height =
                            PhysicalContentHeight::new(content_box_pt(
                                physical_intrinsic
                                    .min_content_height
                                    .points()
                                    .max(transferred_height),
                            ));
                    }
                }
                (true, false) => {
                    let transferred_width = content_height * ratio;
                    if inline_measurement.line_count() == 0 && element.children.is_empty() {
                        content_width = transferred_width;
                    } else {
                        content_width = content_width.max(transferred_width);
                    }
                    // The preferred width exported to Flexbox's intrinsic
                    // sizing phases must include the ratio transfer as well
                    // as this temporary used width. In particular,
                    // `flex-basis: content` and an inline flex container's
                    // shrink-to-fit cross size consume these contributions
                    // rather than `width` above.
                    // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
                    // and <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
                    physical_intrinsic.preferred_width = PhysicalContentWidth::new(content_box_pt(
                        physical_intrinsic
                            .preferred_width
                            .points()
                            .max(transferred_width),
                    ));
                    physical_intrinsic.preferred_min_width =
                        PhysicalContentWidth::new(content_box_pt(
                            physical_intrinsic
                                .preferred_min_width
                                .points()
                                .max(transferred_width),
                        ));
                    if matches!(
                        style.writing_mode,
                        WritingMode::VerticalRl
                            | WritingMode::VerticalLr
                            | WritingMode::SidewaysRl
                            | WritingMode::SidewaysLr
                    ) {
                        if inline_measurement.line_count() == 0 && element.children.is_empty() {
                            physical_intrinsic.min_content_height =
                                PhysicalContentHeight::new(content_box_pt(content_height));
                        } else {
                            physical_intrinsic.min_content_height =
                                PhysicalContentHeight::new(content_box_pt(
                                    physical_intrinsic
                                        .min_content_height
                                        .points()
                                        .max(content_height),
                                ));
                        }
                    } else {
                        physical_intrinsic.min_content_height =
                            PhysicalContentHeight::new(content_box_pt(
                                physical_intrinsic
                                    .min_content_height
                                    .points()
                                    .max(content_height),
                            ));
                    }
                }
                (true, true) => {
                    let stretched_height = available
                        .stretched_height
                        .map(|height| (height.points() - vertical_non_content).max(0.0));
                    let stretched_width = available.stretched_width.map(|width| {
                        let horizontal_non_content = style.padding.left
                            + style.padding.right
                            + horizontal_border_width(style);
                        (width.points() - horizontal_non_content).max(0.0)
                    });
                    if let Some(height) = stretched_height {
                        content_height = height;
                        content_width = height * ratio;
                    } else if let Some(width) = stretched_width {
                        content_width = width;
                        content_height = width / ratio;
                    }
                    if stretched_height.is_some() || stretched_width.is_some() {
                        physical_intrinsic.preferred_width =
                            PhysicalContentWidth::new(content_box_pt(content_width));
                        physical_intrinsic.preferred_min_width =
                            PhysicalContentWidth::new(content_box_pt(content_width));
                        if inline_measurement.line_count() == 0 && element.children.is_empty() {
                            physical_intrinsic.intrinsic_content_height =
                                PhysicalContentHeight::new(content_box_pt(content_height));
                            physical_intrinsic.min_content_height =
                                PhysicalContentHeight::new(content_box_pt(content_height));
                        } else {
                            // A definite stretched cross size transfers through
                            // `aspect-ratio`, but it does not replace a
                            // non-replaced item's content-based automatic
                            // minimum in the main axis.
                            // https://www.w3.org/TR/css-flexbox-1/#min-size-auto
                            physical_intrinsic.intrinsic_content_height =
                                PhysicalContentHeight::new(content_box_pt(
                                    physical_intrinsic
                                        .intrinsic_content_height
                                        .points()
                                        .max(content_height),
                                ));
                            physical_intrinsic.min_content_height =
                                PhysicalContentHeight::new(content_box_pt(
                                    physical_intrinsic
                                        .min_content_height
                                        .points()
                                        .max(content_height),
                                ));
                        }
                    }
                }
                _ => {}
            }
        }

        let mut width = constrain_content_width(
            style,
            content_box_pt(content_width),
            PercentageBasis::definite(layout_pt(containing_width)),
        )
        .points();
        let height = constrain_flex_item_estimated_height(
            style,
            content_height,
            physical_intrinsic.min_content_height.points(),
            physical_intrinsic.intrinsic_content_height.points(),
            containing_height_basis,
            vertical_non_content,
        );
        // An automatic non-replaced box with a preferred aspect ratio can
        // acquire a definite block size from its min/max block constraints.
        // That used main size then transfers into the automatic cross size.
        // This matters while calculating an intrinsic column-flex container:
        // its shrink-to-fit width is the item's transferred width, not the
        // pre-constraint empty-content contribution.  Use the shared transfer
        // helper so `box-sizing:border-box` applies the ratio to the border
        // box while `auto <ratio>` remains content-box based.
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio> and
        // <https://drafts.csswg.org/css-flexbox-1/#intrinsic-sizes>.
        if style.box_values.width.is_auto()
            && style.box_values.height.is_auto()
            && !style.box_values.min_height.is_auto()
            && let Some(ratio) = style.aspect_ratio.preferred_ratio_for_non_replaced(false)
        {
            let transferred_width = flex_aspect_ratio_transferred_content_main_size(
                style,
                height,
                // We are deriving the physical width (the row-axis main
                // size) from the resolved physical height.
                FlexDirection::Row,
                ratio,
            )
            .points();
            width = constrain_content_width(
                style,
                content_box_pt(transferred_width),
                PercentageBasis::definite(layout_pt(containing_width)),
            )
            .points();
            physical_intrinsic.preferred_width = PhysicalContentWidth::new(content_box_pt(
                physical_intrinsic
                    .preferred_width
                    .points()
                    .max(transferred_width),
            ));
            physical_intrinsic.preferred_min_width = PhysicalContentWidth::new(content_box_pt(
                physical_intrinsic
                    .preferred_min_width
                    .points()
                    .max(transferred_width),
            ));
        }
        let min_width = constrain_content_width(
            style,
            physical_intrinsic.preferred_min_width.content_box_length(),
            PercentageBasis::definite(layout_pt(containing_width)),
        )
        .points();
        let min_height = constrain_flex_item_estimated_height(
            style,
            physical_intrinsic.min_content_height.points(),
            physical_intrinsic.min_content_height.points(),
            physical_intrinsic.intrinsic_content_height.points(),
            containing_height_basis,
            vertical_non_content,
        );
        let fallback_line_baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
        let first_line_baseline_offset = first_sequence_line_baseline_offset(
            &inline_measurement.sequence,
            fallback_line_baseline_offset,
        );
        let last_line_baseline_offset = last_sequence_line_baseline_offset(
            &inline_measurement.sequence,
            fallback_line_baseline_offset,
        );
        let preceding_line_height = preceding_line_height_before_last(&inline_measurement.sequence);
        let descendant_baselines = if inline_measurement.line_count() == 0 {
            self.estimate_flex_item_descendant_baselines(
                element,
                signature,
                style,
                child_boxes,
                stylesheets,
                containing_width,
            )
        } else {
            FlexItemBaselineEstimate::default()
        };

        let baseline_edge = used_border_widths(style).top + style.padding.top;
        let first_text_baseline = baseline_edge + first_line_baseline_offset;
        let last_text_baseline = baseline_edge
            + inline_measurement
                .sequence
                .last_line_baseline_offset(fallback_line_baseline_offset);
        FlexItemEstimate {
            metrics: IntrinsicItemMetrics {
                width: content_box_pt(width),
                height,
                min_width: content_box_pt(min_width),
                min_height,
                content_width: physical_intrinsic.preferred_width.content_box_length(),
                content_height: physical_intrinsic
                    .intrinsic_content_height
                    .content_box_length(),
                preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                first_baseline: (inline_measurement.line_count() > 0)
                    .then_some(first_text_baseline)
                    .or(descendant_baselines.first_baseline),
                last_baseline: (inline_measurement.line_count() > 0)
                    .then_some(last_text_baseline)
                    .or(descendant_baselines.last_baseline),
            },
            first_horizontal_baseline: (inline_measurement.line_count() > 0)
                .then(|| {
                    first_horizontal_text_baseline_offset(style, width, first_line_baseline_offset)
                })
                .flatten()
                .or(descendant_baselines.first_horizontal_baseline),
            last_horizontal_baseline: (inline_measurement.line_count() > 0)
                .then(|| {
                    last_horizontal_text_baseline_offset(
                        style,
                        width,
                        preceding_line_height,
                        last_line_baseline_offset,
                    )
                })
                .flatten()
                .or(descendant_baselines.last_horizontal_baseline),
        }
    }

    pub(in crate::layout::flex) fn estimate_child_inline_measurement(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> inline_layout::InlineIntrinsicMeasurement {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return child
                .anonymous_content()
                .map(|children| {
                    self.intrinsic_inline_measurement_for_boxes(
                        children,
                        &child.style,
                        stylesheets,
                        available_width,
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
                available_width,
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
        stylesheets: &[Stylesheet],
        content_width: f32,
    ) -> Option<f32> {
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
            return self.estimate_multicol_auto_block_size(
                style,
                stylesheets,
                child_boxes,
                content_width,
            );
        }
        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(content_width)),
            style.font_size,
        )
        .points();
        let column_count =
            used_multicol_column_count(style, content_width, gap).filter(|count| *count > 1)?;
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((content_width - total_gap) / column_count as f32).max(1.0);
        let available_column_width =
            (column_width - style.padding.left - style.padding.right).max(1.0);
        let measurement =
            self.estimate_child_inline_measurement(child, stylesheets, available_column_width);

        (measurement.line_count() > 0).then(|| {
            measurement
                .sequence
                .balanced_multicolumn_height(column_count, style)
                .max(style.line_height)
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
        stylesheets: &[Stylesheet],
        containing_width: f32,
        inline_contribution: inline_layout::InlineIntrinsicContribution,
    ) -> inline_layout::InlineIntrinsicContribution {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return inline_contribution;
        };
        if child.style.display.is_table() && !child.style.box_values.min_width.is_auto() {
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
                let fragment =
                    box_tree::build_frozen_table_fragment(element, signature, child_boxes);
                // A table with an authored minimum still has the table grid's
                // min-content floor. Its preferred `width` participates
                // separately in flex-basis/main-size resolution; treating it
                // as the content-based automatic minimum would prevent a
                // `flex-basis` from shrinking a table with (for example)
                // `width: 500px`.
                // <https://drafts.csswg.org/css-tables/#used-min-width-of-table>
                // <https://drafts.csswg.org/css-flexbox-1/#min-size-auto>
                let (min_content, max_content) = layout.table_intrinsic_widths_from_fragment(
                    element,
                    &child.style,
                    stylesheets,
                    &fragment,
                    containing_width,
                );
                inline_layout::InlineIntrinsicContribution {
                    min_content,
                    max_content,
                }
            });
        }
        self.with_ancestor_signature(signature.clone(), |layout| {
            layout.estimate_element_flex_intrinsic_widths(
                element,
                &child.style,
                stylesheets,
                child_boxes,
                containing_width,
                inline_contribution,
            )
        })
    }

    pub(in crate::layout::flex) fn estimate_element_flex_intrinsic_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_width: f32,
        inline_contribution: inline_layout::InlineIntrinsicContribution,
    ) -> inline_layout::InlineIntrinsicContribution {
        if style.contain.size {
            return inline_layout::InlineIntrinsicContribution::default();
        }
        let mut contribution = inline_contribution;
        if let Some(child_boxes) = child_boxes {
            let (float_min, float_max) = self.inline_float_run_intrinsic_widths_for_boxes(
                child_boxes,
                style,
                stylesheets,
                containing_width,
            );
            contribution.min_content = contribution.min_content.max(float_min);
            contribution.max_content = contribution.max_content.max(float_max);
            self.merge_box_children_flex_intrinsic_widths(
                child_boxes,
                stylesheets,
                containing_width,
                &mut contribution,
            );
        } else {
            self.merge_dom_children_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                containing_width,
                &mut contribution,
            );
        }
        contribution
    }

    pub(in crate::layout::flex) fn merge_box_children_flex_intrinsic_widths(
        &mut self,
        child_boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        containing_width: f32,
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
                        child_children,
                    );
                    let (min_content, _) = layout
                        .table_parent_intrinsic_content_widths_with_indefinite_percentage_basis(
                            child_element,
                            child_style,
                            stylesheets,
                            &fragment,
                            containing_width,
                        );
                    let (_, max_content) = layout
                        .table_parent_intrinsic_content_widths_from_fragment(
                            child_element,
                            child_style,
                            stylesheets,
                            &fragment,
                            containing_width,
                        );
                    (min_content, max_content)
                })
            } else {
                explicit_child_intrinsic_width(child_style, containing_width).unwrap_or_else(|| {
                    self.with_ancestor_signature(signature.clone(), |layout| {
                        layout.block_intrinsic_content_widths(
                            child_element,
                            child_style,
                            stylesheets,
                            Some(child_children),
                            containing_width,
                        )
                    })
                })
            };
            merge_outer_intrinsic_widths(
                contribution,
                child_contribution,
                child_style,
                containing_width,
            );
        }
    }

    pub(in crate::layout::flex) fn merge_dom_children_flex_intrinsic_widths(
        &mut self,
        element: &Element,
        parent_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        containing_width: f32,
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
                        &child_boxes,
                    );
                    let (min_content, _) = layout
                        .table_parent_intrinsic_content_widths_with_indefinite_percentage_basis(
                            child_element,
                            &child_style,
                            stylesheets,
                            &fragment,
                            containing_width,
                        );
                    let (_, max_content) = layout
                        .table_parent_intrinsic_content_widths_from_fragment(
                            child_element,
                            &child_style,
                            stylesheets,
                            &fragment,
                            containing_width,
                        );
                    (min_content, max_content)
                })
            } else {
                explicit_child_intrinsic_width(&child_style, containing_width).unwrap_or_else(
                    || {
                        self.with_ancestor_signature(signature, |layout| {
                            layout.block_intrinsic_content_widths(
                                child_element,
                                &child_style,
                                stylesheets,
                                None,
                                containing_width,
                            )
                        })
                    },
                )
            };
            merge_outer_intrinsic_widths(
                contribution,
                child_contribution,
                &child_style,
                containing_width,
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
        stylesheets: &[Stylesheet],
        containing_width: f32,
        inline_content_height: f32,
    ) -> f32 {
        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return inline_content_height;
        };
        self.with_ancestor_signature(signature.clone(), |layout| {
            let containing_height_basis = layout
                .definite_block_size_stack
                .last()
                .cloned()
                .unwrap_or_else(PercentageBasis::indefinite);
            layout.estimate_element_children_min_content_block_size(
                element,
                &child.style,
                stylesheets,
                child_boxes,
                FlexMinContentBlockContainingSpace {
                    width: PhysicalContentWidth::new(content_box_pt(containing_width)),
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
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_space: FlexMinContentBlockContainingSpace,
        inline_content_height: f32,
    ) -> f32 {
        if parent_style.contain.size {
            return 0.0;
        }
        let mut block_size = inline_content_height;

        if let Some(child_boxes) = child_boxes {
            for child_box in child_boxes {
                let Some((child_element, signature, child_style, child_children)) =
                    child_box.element_parts()
                else {
                    continue;
                };
                if !flex_min_content_block_child_participates(child_element, child_style) {
                    continue;
                }
                block_size += self.with_ancestor_signature(signature.clone(), |layout| {
                    layout.flex_child_outer_min_content_block_size(
                        child_element,
                        child_style,
                        stylesheets,
                        Some(child_children),
                        containing_space.width.points(),
                        containing_space.height_percentage_basis,
                    )
                });
            }
            return block_size;
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
            if !flex_min_content_block_child_participates(child_element, &child_style) {
                continue;
            }
            block_size += self.with_ancestor_signature(signature, |layout| {
                layout.flex_child_outer_min_content_block_size(
                    child_element,
                    &child_style,
                    stylesheets,
                    None,
                    containing_space.width.points(),
                    containing_space.height_percentage_basis,
                )
            });
        }
        block_size
    }

    pub(in crate::layout::flex) fn flex_child_outer_min_content_block_size(
        &mut self,
        child_element: &Element,
        child_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        containing_width: f32,
        containing_height_basis: BlockSizePercentageBasis,
    ) -> f32 {
        // Flex intrinsic estimation can visit a nested block before its
        // ordinary formatting-context replay. Resolve viewport units at this
        // page-context boundary so an explicit `height: 350vh` contributes
        // its used monolithic extent instead of falling through to a zero
        // intrinsic size.
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        // <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
        let child_style = self.style_with_current_viewport_lengths(child_style);
        let vertical_non_content = child_style.padding.top
            + child_style.padding.bottom
            + vertical_border_width(&child_style);
        let content_size = used_content_box_height_or_auto_with_basis(
            &child_style,
            containing_height_basis,
            non_content_pt(vertical_non_content),
        )
        .map(SemanticLengthExt::points)
        .unwrap_or_else(|| {
            let inline_width =
                (containing_width - child_style.padding.left - child_style.padding.right).max(1.0);
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
                    width: PhysicalContentWidth::new(content_box_pt(containing_width)),
                    height_percentage_basis: PercentageBasis::indefinite(),
                },
                inline_measurement.height(),
            )
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
        child_style.margin.top
            + child_style.padding.top
            + border_widths.top
            + constrained_content_size.points()
            + child_style.padding.bottom
            + border_widths.bottom
            + child_style.margin.bottom
    }

    /// Measure an auto-height flex item as block layout when float placement affects flex basis.
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
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        content_width: f32,
        inline_line_count: usize,
    ) -> Option<f32> {
        if !style.box_values.height.is_auto()
            || !style.display.is_block_level()
            || !matches!(
                style.display.inner,
                DisplayInner::Flow | DisplayInner::FlowRoot
            )
            || inline_line_count > 0
        {
            return None;
        }
        if !child_boxes
            .map(flex_item_child_boxes_include_float)
            .unwrap_or(false)
        {
            return None;
        }

        let mut measured_style = style.clone();
        measured_style.display.inner = DisplayInner::FlowRoot;
        Some(self.measure_auto_positioned_block_height(
            element,
            &measured_style,
            stylesheets,
            content_width.max(0.0),
            child_boxes,
            None,
        ))
    }

    /// Estimate a flex container's intrinsic size and exported row baselines.
    ///
    /// CSS Flexbox defines intrinsic flex container sizes from flex-item
    /// contributions and exports first/last main-axis baselines from row flex
    /// lines for parent baseline alignment:
    /// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
    pub(in crate::layout::flex) fn estimate_intrinsic_flex_container_size(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
    ) -> FlexItemEstimate {
        // A flex container's intrinsic dimensions come only from flex-item
        // contributions. Unlike an inline formatting context, it has no line
        // strut to contribute a font-size or line-height floor, including when
        // it is empty. Such a floor would feed a synthetic size into
        // shrink-to-fit ancestors and incorrectly expose a container's
        // background around its flex items.
        // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
        let physical_direction = physical_flex_direction(style);
        let border_widths = used_border_widths(style);
        let PhysicalFlexGaps {
            horizontal: physical_gap_width,
            vertical: physical_gap_height,
        } = physical_flex_gaps(style);
        // A definite cross size on the flex container is available while
        // measuring its items for intrinsic main-size contributions. In
        // particular, stretch alignment can transfer that size through a
        // preferred aspect ratio before the inline flex container's own main
        // size is known:
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
        // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
        let intrinsic_item_available = definite_flex_item_cross_content_size(
            style,
            physical_direction,
            available.cross_basis(physical_direction),
        )
        .map(|cross_size| {
            flex_available_with_definite_cross_size(available, physical_direction, cross_size)
        })
        .unwrap_or(available);
        let (mut intrinsic_items, estimated_baseline_items) = self.estimate_flex_intrinsic_items(
            children,
            style,
            stylesheets,
            intrinsic_item_available,
            physical_direction,
        );
        // A collapsed item does not contribute to the flex main-size
        // calculation, but Flexbox preserves its hypothetical cross size as
        // a strut.  Keep that distinction while computing an inline flex
        // container's intrinsic cross size; otherwise a column flex box can
        // shrink to its visible 10pt child despite a collapsed 50pt child.
        // <https://www.w3.org/TR/css-flexbox-1/#visibility-collapse>
        let collapsed_cross_strut = children
            .iter()
            .filter(|child| flex_item_is_collapsed(&child.style))
            .map(|child| {
                let item_available = estimated_flex_item_available_space(
                    &child.style,
                    style,
                    physical_direction,
                    intrinsic_item_available,
                );
                let estimate = self.estimate_flex_item_size(
                    child,
                    stylesheets,
                    item_available,
                    physical_direction,
                );
                estimated_outer_cross_size(&child.style, estimate, physical_direction).points()
            })
            .fold(0.0f32, f32::max);
        if style.flex_wrap == FlexWrap::NoWrap || intrinsic_items.len() == 1 {
            let available_main_size = if physical_direction.is_row_axis() {
                Some(intrinsic_item_available.width.points())
            } else {
                intrinsic_item_available
                    .height
                    .map(PhysicalContentHeight::points)
            };
            apply_single_line_flexed_main_cross_contributions(
                &mut intrinsic_items,
                physical_direction,
                available_main_size,
            );
        }

        let intrinsic_main_gap =
            estimated_intrinsic_flex_gap(if physical_direction.is_column_axis() {
                physical_gap_height.clone()
            } else {
                physical_gap_width.clone()
            })
            .points();
        let intrinsic_cross_gap =
            estimated_intrinsic_flex_gap(if physical_direction.is_column_axis() {
                physical_gap_width
            } else {
                physical_gap_height
            })
            .points();
        let min_main = intrinsic_flex_container_min_main_size(
            style,
            physical_direction,
            &intrinsic_items,
            intrinsic_main_gap,
            intrinsic_item_available,
        );
        let max_main = intrinsic_flex_container_max_main_size(
            style,
            physical_direction,
            &intrinsic_items,
            intrinsic_main_gap,
            intrinsic_item_available,
        );
        let (mut min_cross, mut max_cross) = intrinsic_flex_container_cross_sizes(
            style,
            physical_direction,
            &intrinsic_items,
            intrinsic_cross_gap,
            intrinsic_item_available,
            min_main,
            max_main,
        );
        if style.flex_direction.is_column_axis()
            && style.flex_wrap != FlexWrap::NoWrap
            && !style.flex_wrap.balances_lines()
        {
            let available_cross_size = intrinsic_items
                .iter()
                .map(|item| item.max_cross_contribution)
                .fold(0.0f32, f32::max);
            if available_cross_size > 0.0 {
                let constrained_available = flex_available_with_definite_cross_size(
                    intrinsic_item_available,
                    physical_direction,
                    available_cross_size,
                );
                let (max_content_items, _) = self.estimate_flex_intrinsic_items(
                    children,
                    style,
                    stylesheets,
                    constrained_available,
                    physical_direction,
                );
                let max_content_min_main = intrinsic_flex_container_min_main_size(
                    style,
                    physical_direction,
                    &max_content_items,
                    intrinsic_main_gap,
                    constrained_available,
                );
                let max_content_max_main = intrinsic_flex_container_max_main_size(
                    style,
                    physical_direction,
                    &max_content_items,
                    intrinsic_main_gap,
                    constrained_available,
                );
                let (_, max_content_cross) = intrinsic_flex_container_cross_sizes(
                    style,
                    physical_direction,
                    &max_content_items,
                    intrinsic_cross_gap,
                    constrained_available,
                    max_content_min_main,
                    max_content_max_main,
                );
                max_cross = max_content_cross;
            }
        }
        min_cross = min_cross.max(collapsed_cross_strut);
        max_cross = max_cross.max(collapsed_cross_strut);
        let (mut min_width, mut width, mut min_height, mut height) =
            if physical_direction.is_column_axis() {
                (min_cross, max_cross, min_main, max_main)
            } else {
                (min_main, max_main, min_cross, max_cross)
            };

        let line_metrics = estimate_row_flex_container_line_metrics(
            style,
            intrinsic_item_available,
            &estimated_baseline_items,
        );
        if let Some(metrics) = line_metrics
            && style.flex_wrap != FlexWrap::NoWrap
            && metrics.line_count > 1
        {
            if physical_direction.is_row_axis() {
                height = height.max(metrics.cross_size);
                min_height = min_height.max(metrics.cross_size);
            } else {
                width = width.max(metrics.cross_size);
                min_width = min_width.max(metrics.cross_size);
            }
        }

        let content_width = width;
        let content_height = height;
        let (first_baseline, last_baseline) = line_metrics
            .map(|metrics| (metrics.first_baseline, metrics.last_baseline))
            .unwrap_or((None, None));
        let (first_baseline, last_baseline, first_horizontal_baseline, last_horizontal_baseline) =
            if physical_direction.is_row_axis() {
                (
                    first_baseline.map(|baseline| border_widths.top + style.padding.top + baseline),
                    last_baseline.map(|baseline| border_widths.top + style.padding.top + baseline),
                    None,
                    None,
                )
            } else if style.flex_direction.is_row_axis() {
                (
                    None,
                    None,
                    first_baseline
                        .map(|baseline| border_widths.left + style.padding.left + baseline),
                    last_baseline
                        .map(|baseline| border_widths.left + style.padding.left + baseline),
                )
            } else {
                (None, None, None, None)
            };
        let width = used_length_percentage_or_auto_with_basis(
            style.box_values.width.clone(),
            available.width_basis,
        )
        .map(|width| width.points())
        .unwrap_or(width);
        let height = used_length_percentage_or_auto(
            style.box_values.height.clone(),
            PercentageBasis::definite(layout_pt(
                available
                    .height
                    .map(PhysicalContentHeight::points)
                    .unwrap_or_else(|| available.width.points()),
            )),
        )
        .map(|height| height.points())
        .or_else(|| available.height.map(PhysicalContentHeight::points))
        .unwrap_or(height);
        FlexItemEstimate {
            metrics: IntrinsicItemMetrics {
                width: content_box_pt(width),
                height: content_box_pt(height),
                min_width: content_box_pt(min_width),
                min_height: content_box_pt(min_height),
                content_width: content_box_pt(content_width),
                content_height: content_box_pt(content_height),
                preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                first_baseline,
                last_baseline,
            },
            first_horizontal_baseline,
            last_horizontal_baseline,
        }
    }

    fn estimate_flex_intrinsic_items(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
        physical_direction: FlexDirection,
    ) -> (Vec<FlexIntrinsicItem>, Vec<EstimatedFlexBaselineItem>) {
        let mut intrinsic_items = Vec::with_capacity(children.len());
        let mut estimated_baseline_items = Vec::with_capacity(children.len());
        let container_inline_size = FlexItemAvailableSpace::from_container(available)
            .inline_size(style)
            .points()
            .max(0.0);

        for child in children {
            // Collapsed flex items are removed from flex layout before the
            // container's intrinsic main-size contributions are calculated.
            // Their original cross size survives only as a strut in the
            // final flex layout pass; including their main contribution here
            // incorrectly widens an auto-sized container and leaves a gap
            // where the collapsed item used to be.
            // <https://drafts.csswg.org/css-flexbox-1/#visibility-collapse>.
            if flex_item_is_collapsed(&child.style) {
                continue;
            }
            let item_available = estimated_flex_item_available_space(
                &child.style,
                style,
                physical_direction,
                available,
            );
            let size = self.estimate_flex_item_size(
                child,
                stylesheets,
                item_available,
                physical_direction,
            );
            let item = FlexIntrinsicItem::new(
                child,
                size,
                physical_direction,
                available,
                container_inline_size,
            );
            let (first_baseline, last_baseline) =
                estimated_flex_item_cross_axis_baselines(size, physical_direction);
            estimated_baseline_items.push(EstimatedFlexBaselineItem {
                outer_main_size: item.flex_base_size,
                outer_cross_size: item.max_cross_contribution,
                margin_cross_start: if physical_direction.is_row_axis() {
                    child.style.margin.top
                } else {
                    child.style.margin.left
                },
                cross_alignment: estimated_flex_item_cross_alignment(&child.style, style),
                first_baseline,
                last_baseline,
            });
            intrinsic_items.push(item);
        }

        (intrinsic_items, estimated_baseline_items)
    }

    pub(in crate::layout::flex) fn estimate_definition_list_column_height(
        &mut self,
        child: &StyledChild<'_>,
        stylesheets: &[Stylesheet],
        containing_width: f32,
    ) -> Option<f32> {
        let multicol_style = self.multicol_used_style(&child.style);
        let style = &multicol_style;
        let (element, _, child_boxes) = child.element_parts()?;
        if !is_definition_list_element(element) {
            return None;
        }

        let groups = child_boxes
            .map(definition_list_column_groups_from_boxes)
            .unwrap_or_else(|| {
                definition_list_column_groups_with_font_metrics(
                    element,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                )
            });
        if groups.is_empty() {
            return None;
        }

        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(containing_width)),
            style.font_size,
        )
        .points();
        let column_count =
            used_multicol_column_count(style, containing_width, gap).filter(|count| *count > 1)?;
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((containing_width - total_gap) / column_count as f32).max(1.0);
        let mut column_heights = vec![0.0f32; column_count];

        for (group_index, group) in groups.iter().enumerate() {
            let column_index = group_index % column_count;
            for item in group {
                column_heights[column_index] +=
                    self.estimate_flex_column_item_height(item, stylesheets, column_width);
            }
        }

        column_heights.into_iter().reduce(f32::max)
    }

    pub(in crate::layout::flex) fn estimate_flex_column_item_height(
        &mut self,
        item: &DefinitionListColumnItem<'_>,
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        let content_width =
            (available_width - item.style.padding.left - item.style.padding.right).max(1.0);
        let content_height = item
            .children
            .map(|children| {
                self.intrinsic_inline_block_metrics_for_boxes(
                    children,
                    &item.style,
                    stylesheets,
                    content_width,
                )
                .0
            })
            .unwrap_or_else(|| {
                self.intrinsic_inline_block_metrics_for_element(
                    item.element,
                    &item.style,
                    stylesheets,
                    None,
                    content_width,
                )
                .0
            })
            .max(self.font_system.used_line_height(&item.style).points());

        item.style.margin.top
            + vertical_border_width(&item.style)
            + item.style.padding.top
            + content_height
            + item.style.padding.bottom
            + item.style.margin.bottom
    }
}
