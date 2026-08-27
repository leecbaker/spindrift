use super::item_flow::{
    FlexItemLogicalIntrinsicSizes, flex_estimated_border_box_width,
    flex_item_physical_intrinsic_sizes,
};
use super::*;

/// Return the normalized single block-level replaced child, if it is the
/// entire flow content of this flex item.
///
/// Flex-item blockification must preserve the descendant's authored outer
/// display role. This narrow fast path keeps a `display:block` SVG out of the
/// inline-replaced-row measurement while preserving its CSS used size.
/// <https://www.w3.org/TR/css-display-3/#box-generation>
fn direct_block_replaced_child<'a>(
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> Option<(&'a Element, &'a ComputedStyle)> {
    let [child_box] = child_boxes else {
        return None;
    };
    let (element, _, style, _) = child_box.element_parts()?;
    (is_replaced_element(element) && style.display.is_block_level()).then_some((element, style))
}

/// Shared inputs derived once for all flex-item sizing cases.
///
/// The normalized style and its physical/logical percentage bases must travel
/// together: special cases such as containment and nested formatting contexts
/// must use the same bases as ordinary flow sizing.
#[derive(Clone, Copy)]
pub(super) struct FlexItemEstimateContext<'a> {
    pub(super) style: &'a ComputedStyle,
    pub(super) available: FlexItemAvailableSpace,
    pub(super) physical_direction: FlexDirection,
    pub(super) containing_width: ContentBoxLength,
    pub(super) containing_width_basis: FlexAvailablePercentageBasis,
    pub(super) containing_height_basis: FlexAvailablePercentageBasis,
    pub(super) containing_inline_size: LogicalInlineContentSize,
    pub(super) containing_inline_size_points: f32,
    /// Whether `containing_inline_size` is the flex item's already-resolved
    /// content box rather than its containing block.  The two values have the
    /// same unit but require different box-model conversions before inline
    /// line selection.
    pub(super) inline_measurement_space: FlexInlineMeasurementSpace,
    pub(super) preferred_inline_basis: FlexAvailablePercentageBasis,
    pub(super) vertical_non_content: NonContentLength,
}

/// The box-model space represented by an inline measurement constraint.
///
/// A normal flex estimate begins with the container's content box, so the
/// item must remove its inline padding before selecting text lines.  A
/// definite preferred cross size and a post-flexing main size have already
/// crossed that boundary: they are the item's content box.  Retaining this
/// distinction prevents the item's padding from being removed twice, which
/// otherwise creates an unpainted extra line in the flex item's used block
/// size.
/// <https://www.w3.org/TR/css-sizing-3/#box-model> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FlexInlineMeasurementSpace {
    ContainingBlockContentBox,
    ResolvedItemContentBox,
}

impl FlexInlineMeasurementSpace {
    pub(super) fn content_box_inline_width(
        self,
        available_inline_size: f32,
        style: &ComputedStyle,
    ) -> f32 {
        match self {
            Self::ContainingBlockContentBox => {
                (available_inline_size - style.padding.left - style.padding.right).max(1.0)
            }
            Self::ResolvedItemContentBox => available_inline_size.max(1.0),
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn estimate_nested_flex_container_item(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &Stylesheets<'_>,
        context: FlexItemEstimateContext<'_>,
    ) -> Option<FlexItemEstimate> {
        let FlexItemEstimateContext {
            style,
            available,
            physical_direction,
            containing_width,
            containing_width_basis,
            containing_height_basis,
            vertical_non_content,
            ..
        } = context;
        let main_size_resolved_by_flex = matches!(
            available.width_basis,
            PercentageBasis::Definite {
                source: FlexAvailableSizeSource::PostFlexingMainSize,
                ..
            }
        ) && physical_direction.is_row_axis();
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
                // `available.height_basis` can describe this item's already
                // resolved used height. In that case the nested flex
                // container is being measured *inside* the flex item, so its
                // own authored percentage height must not be resolved a
                // second time against that used height. For example, a
                // column parent's `height: 200px` gives its `height: 50%`
                // child a 100px used main size; a nested flex estimate must
                // use 100px, not resolve 50% of 100px again.
                // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
                // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
                let resolved_item_height = matches!(
                    available.height_basis,
                    PercentageBasis::Definite {
                        source: FlexAvailableSizeSource::DefinitePreferredMainSize
                            | FlexAvailableSizeSource::DefinitePreferredCrossSize
                            | FlexAvailableSizeSource::DefiniteFlexBase
                            | FlexAvailableSizeSource::PostFlexingMainSize
                            | FlexAvailableSizeSource::DefiniteCrossSize
                            | FlexAvailableSizeSource::BalancedLineSlot
                            | FlexAvailableSizeSource::DefiniteSingleLineStretch,
                        ..
                    }
                )
                .then_some(available.height)
                .flatten()
                .map(PhysicalContentHeight::content_box_length);
                let explicit_intrinsic_height = resolved_item_height.or_else(|| {
                    used_content_box_height_or_auto_with_basis(
                        style,
                        containing_height_basis,
                        vertical_non_content,
                    )
                });
                // An auto-height flex item stretched by a parent line
                // has a definite used cross size for its descendants.
                // A nested flex container must retain that marked basis
                // while estimating its own items; otherwise percentage
                // block constraints inside it are treated as cyclic even
                // after the parent has finalized the line cross size.
                // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
                let intrinsic_height = explicit_intrinsic_height.or_else(|| {
                    available
                        .stretched_height
                        .map(PhysicalContentHeight::content_box_length)
                });
                let intrinsic_height_basis = if resolved_item_height.is_some() {
                    available.height_basis
                } else if explicit_intrinsic_height.is_some() {
                    // This is the nested flex container's own resolved
                    // preferred block size, not merely an intrinsic
                    // measurement constraint. Its items may therefore use
                    // it as a percentage basis while contributing their
                    // intrinsic inline sizes.
                    flex_available_percentage_basis(
                        intrinsic_height,
                        FlexAvailableSizeSource::DefinitePreferredCrossSize,
                    )
                } else if available.stretched_height.is_some() {
                    available.height_basis
                } else {
                    flex_available_percentage_basis(
                        intrinsic_height,
                        FlexAvailableSizeSource::IntrinsicContainerSize,
                    )
                };
                // The nested Flex solver consumes its container style as well
                // as the available-space record. Freeze only this temporary
                // layout style when its parent item already has a used height;
                // descendants continue to come from the source box tree
                // built above, so this does not leak a used value into CSS
                // cascade.
                let mut intrinsic_style = style.clone();
                if let Some(height) = resolved_item_height {
                    set_style_used_height(&mut intrinsic_style, height.points());
                }
                let descendant_block_basis =
                    intrinsic_block_basis_from_flex_available_height(intrinsic_height_basis);
                layout.with_flex_item_percentage_height_basis(descendant_block_basis, |layout| {
                    layout.estimate_intrinsic_flex_container_size(
                        &children,
                        &intrinsic_style,
                        stylesheets,
                        FlexAvailableSpace {
                            width: PhysicalContentWidth::new(containing_width),
                            width_basis: if main_size_resolved_by_flex {
                                PercentageBasis::definite_from(
                                    containing_width,
                                    FlexAvailableSizeSource::PostFlexingMainSize,
                                )
                            } else {
                                flex_available_percentage_basis(
                                    used_length_percentage_or_auto_with_basis(
                                        style.box_values.width.clone(),
                                        containing_width_basis,
                                    )
                                    .map(|_| containing_width),
                                    FlexAvailableSizeSource::IntrinsicContainerSize,
                                )
                            },
                            height: intrinsic_height.map(PhysicalContentHeight::new),
                            height_basis: intrinsic_height_basis,
                        },
                    )
                })
            })
        });
        if let Some(intrinsic_size) = intrinsic_size {
            let content_width = used_length_percentage_or_auto_with_basis(
                style.box_values.width.clone(),
                containing_width_basis,
            )
            .map(|width| width.points())
            .unwrap_or_else(|| {
                if main_size_resolved_by_flex {
                    containing_width.points()
                } else {
                    intrinsic_size.width.points()
                }
            })
            .max(style.font_size);
            let used_line_height = self.font_system.used_line_height(style).points();
            let content_height = used_content_box_height_or_auto_with_basis(
                style,
                containing_height_basis,
                vertical_non_content,
            )
            .map(SemanticLengthExt::points)
            .unwrap_or_else(|| intrinsic_size.height.points())
            .max(used_line_height);
            let used_baselines = self.with_ancestor_signature(signature.clone(), |layout| {
                let children = flex_children_from_boxes(element, signature, style, child_boxes?);
                layout
                    .compute_flex_layout(
                        &children,
                        style,
                        stylesheets,
                        FlexAvailableSpace {
                            width: PhysicalContentWidth::new(content_box_pt(content_width)),
                            width_basis: PercentageBasis::definite_from(
                                content_box_pt(content_width),
                                FlexAvailableSizeSource::PostFlexingMainSize,
                            ),
                            height: Some(PhysicalContentHeight::new(content_box_pt(
                                content_height,
                            ))),
                            height_basis: PercentageBasis::definite_from(
                                content_box_pt(content_height),
                                FlexAvailableSizeSource::DefiniteCrossSize,
                            ),
                        },
                    )
                    .map(|layout| layout.baselines)
            });
            return Some(FlexItemEstimate::new(
                IntrinsicItemMetrics {
                    width: constrain_content_width(
                        style,
                        content_box_pt(content_width),
                        PercentageBasis::definite(containing_width),
                    ),
                    height: constrain_content_height(
                        style,
                        content_box_pt(content_height),
                        PercentageBasis::definite(containing_width),
                    ),
                    min_width: constrain_content_width(
                        style,
                        intrinsic_size.min_width,
                        PercentageBasis::definite(containing_width),
                    ),
                    min_height: constrain_content_height(
                        style,
                        intrinsic_size.min_height,
                        PercentageBasis::definite(containing_width),
                    ),
                    content_width: intrinsic_size.content_width,
                    content_height: intrinsic_size.content_height,
                    preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                    first_baseline: intrinsic_size.first_baseline,
                    last_baseline: intrinsic_size.last_baseline,
                },
                used_baselines
                    .map(|baselines| FlexItemBaselineEstimate {
                        vertical: baselines.vertical,
                        horizontal: baselines.horizontal,
                    })
                    .unwrap_or(intrinsic_size.baselines),
            ));
        }
        None
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn estimate_inline_replaced_row_flex_item(
        &mut self,
        element: &Element,
        stylesheets: &Stylesheets<'_>,
        context: FlexItemEstimateContext<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> FlexItemEstimate {
        let FlexItemEstimateContext {
            style,
            available,
            containing_width,
            containing_width_basis,
            containing_height_basis,
            vertical_non_content,
            ..
        } = context;
        // A direct inline replaced row is measured through its own flex item
        // as well as through the enclosing item's first-pass estimate. This
        // second boundary must preserve a resolved item height; otherwise the
        // row's automatic minimum re-measures an image at its natural height
        // and widens an auto-sized nested flex container.
        let block_basis =
            flex_item_estimate_percentage_height_basis(style, available, vertical_non_content);
        let direct_block_replaced_row = child_boxes.and_then(direct_block_replaced_child).and_then(
            |(replaced_child, replaced_style)| {
                let parent_content_width = used_content_box_width_or_auto_with_basis(
                    style,
                    containing_width_basis,
                    non_content_pt(
                        style.padding.left + style.padding.right + horizontal_border_width(style),
                    ),
                )?;
                let replaced = resolve_replaced_element(
                    replaced_child,
                    replaced_style,
                    ReplacedBoxSizingContext {
                        available_width: parent_content_width,
                        inline_percentage_basis: PercentageBasis::definite_from(
                            parent_content_width,
                            IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                        ),
                        block_basis: intrinsic_block_basis_from_flex_available_height(
                            containing_height_basis,
                        ),
                    },
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                )?;
                Some((
                    parent_content_width.points(),
                    replaced_style.margin.top
                        + replaced.geometry().border_box_size.height
                        + replaced_style.margin.bottom,
                ))
            },
        );
        let (row_width, row_height) = direct_block_replaced_row.unwrap_or_else(|| {
            self.measure_direct_inline_row(element, style, stylesheets, block_basis)
        });
        let content_width = used_length_percentage_or_auto_with_basis(
            style.box_values.width.clone(),
            containing_width_basis,
        )
        .map(|width| width.points())
        .unwrap_or(row_width);
        let content_height = used_content_box_height_or_auto_with_basis(
            style,
            containing_height_basis,
            vertical_non_content,
        )
        .unwrap_or_else(|| content_box_pt(row_height));
        let width = constrain_content_width(
            style,
            content_box_pt(content_width),
            PercentageBasis::definite(containing_width),
        )
        .points();
        let height = constrain_flex_item_estimated_height(
            style,
            content_height,
            content_box_pt(row_height),
            content_box_pt(row_height),
            containing_height_basis,
            vertical_non_content,
        );
        let min_width = constrain_content_width(
            style,
            content_box_pt(row_width),
            PercentageBasis::definite(containing_width),
        )
        .points();
        let min_height = constrain_flex_item_estimated_height(
            style,
            content_box_pt(row_height),
            content_box_pt(row_height),
            content_box_pt(row_height),
            containing_height_basis,
            vertical_non_content,
        );
        let line_baseline_offset =
            layout_pt(self.inline_box_text_line_layout_baseline_offset(style));
        let first_baseline = used_border_widths(style).top
            + style.padding.top
            + self.inline_box_text_line_layout_baseline_offset(style);
        FlexItemEstimate::new(
            IntrinsicItemMetrics {
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
            FlexItemBaselineEstimate {
                vertical: FlexItemBaselinePair {
                    first: Some(flex_vertical_baseline_from_points(first_baseline)),
                    last: Some(flex_vertical_baseline_from_points(first_baseline)),
                },
                horizontal: FlexItemBaselinePair {
                    first: first_horizontal_text_baseline_offset(
                        style,
                        flex_estimated_border_box_width(style, content_box_pt(width)),
                        line_baseline_offset,
                    ),
                    last: last_horizontal_text_baseline_offset(
                        style,
                        flex_estimated_border_box_width(style, content_box_pt(width)),
                        layout_pt(0.0),
                        line_baseline_offset,
                    ),
                },
            },
        )
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn estimate_grid_flex_item(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &Stylesheets<'_>,
        context: FlexItemEstimateContext<'_>,
    ) -> Option<FlexItemEstimate> {
        let FlexItemEstimateContext {
            style,
            containing_width,
            containing_width_basis,
            containing_height_basis,
            vertical_non_content,
            ..
        } = context;
        let grid_intrinsic = self.with_ancestor_signature(signature.clone(), |layout| {
            layout.estimate_grid_container_for_flex_item(
                element,
                style,
                stylesheets,
                child_boxes,
                containing_width.points(),
                crate::layout::grid::grid_percentage_basis(
                    containing_width_basis.value(),
                    crate::layout::grid::GridAvailableSizeSource::ContainerInlineSize,
                ),
                crate::layout::grid::grid_percentage_basis(
                    containing_height_basis.value(),
                    crate::layout::grid::GridAvailableSizeSource::ContainerBlockSize,
                ),
                vertical_non_content.points(),
            )
        });
        grid_intrinsic.map(|grid_intrinsic| {
            let intrinsic_height = grid_intrinsic.intrinsic_height;
            let content_height = grid_intrinsic
                .definite_content_height
                .unwrap_or(intrinsic_height);
            let width = constrain_content_width(
                style,
                grid_intrinsic.content_width,
                PercentageBasis::definite(containing_width),
            )
            .points();
            let height = constrain_flex_item_estimated_height(
                style,
                content_height,
                intrinsic_height,
                intrinsic_height,
                containing_height_basis,
                vertical_non_content,
            );
            let min_width = constrain_content_width(
                style,
                grid_intrinsic.min_width,
                PercentageBasis::definite(containing_width),
            )
            .points();
            let min_height = constrain_flex_item_estimated_height(
                style,
                intrinsic_height,
                intrinsic_height,
                intrinsic_height,
                containing_height_basis,
                vertical_non_content,
            );
            FlexItemEstimate::new(
                IntrinsicItemMetrics {
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
                FlexItemBaselineEstimate::default(),
            )
        })
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn estimate_size_contained_flex_item(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        stylesheets: &Stylesheets<'_>,
        context: FlexItemEstimateContext<'_>,
    ) -> FlexItemEstimate {
        let FlexItemEstimateContext {
            style,
            available,
            containing_width,
            containing_width_basis,
            containing_height_basis,
            vertical_non_content,
            ..
        } = context;
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
                    PercentageBasis::definite(containing_width.into_layout_length()),
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
                    PercentageBasis::definite(containing_width.into_layout_length()),
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
            vertical_non_content,
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
            PercentageBasis::definite(containing_width),
        )
        .points();
        let height = constrain_flex_item_estimated_height(
            style,
            content_box_pt(content_height),
            content_box_pt(0.0),
            content_box_pt(0.0),
            containing_height_basis,
            vertical_non_content,
        );
        let descendant_baselines = self.estimate_flex_item_descendant_baselines(
            element,
            signature,
            style,
            child_boxes,
            stylesheets,
            available.width,
        );
        FlexItemEstimate::new(
            IntrinsicItemMetrics {
                width: content_box_pt(width),
                height,
                min_width: constrain_content_width(
                    style,
                    content_box_pt(fallback_width),
                    PercentageBasis::definite(containing_width),
                ),
                min_height: constrain_flex_item_estimated_height(
                    style,
                    content_box_pt(fallback_height),
                    content_box_pt(fallback_height),
                    content_box_pt(fallback_height),
                    containing_height_basis,
                    vertical_non_content,
                ),
                content_width: content_box_pt(fallback_width),
                content_height: content_box_pt(fallback_height),
                preferred_aspect_ratio: style.aspect_ratio.preferred_ratio(false, None),
                first_baseline: descendant_baselines
                    .vertical
                    .first
                    .map(FlexVerticalBaselineOffset::points),
                last_baseline: descendant_baselines
                    .vertical
                    .last
                    .map(FlexVerticalBaselineOffset::points),
            },
            descendant_baselines,
        )
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn estimate_anonymous_flex_item(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        context: FlexItemEstimateContext<'_>,
    ) -> FlexItemEstimate {
        let FlexItemEstimateContext {
            style,
            containing_width,
            containing_width_basis,
            containing_height_basis,
            containing_inline_size_points,
            vertical_non_content,
            ..
        } = context;
        let measurement = self.intrinsic_inline_measurement_for_boxes(
            children,
            style,
            stylesheets,
            containing_inline_size_points.max(1.0),
        );
        let contribution = measurement.contribution;
        let logical_inline_size = contribution
            .max_content
            .points()
            .max(style.font_size * 0.25);
        let logical_min_inline_size = contribution
            .min_content
            .points()
            .max(style.font_size * 0.25);
        let used_line_height = self.font_system.used_line_height(style).points();
        let logical_block_size = measurement.height().max(used_line_height);
        let physical_intrinsic = flex_item_physical_intrinsic_sizes(
            style.writing_mode,
            FlexItemLogicalIntrinsicSizes {
                preferred_inline: LogicalInlineContentSize::new(content_box_pt(
                    logical_inline_size,
                )),
                min_inline: LogicalInlineContentSize::new(content_box_pt(logical_min_inline_size)),
                block: LogicalBlockContentSize::new(content_box_pt(logical_block_size)),
            },
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
            vertical_non_content,
        )
        .unwrap_or(
            physical_intrinsic
                .intrinsic_content_height
                .content_box_length(),
        );
        let width = constrain_content_width(
            style,
            content_box_pt(content_width),
            PercentageBasis::definite(containing_width),
        )
        .points();
        let height = constrain_flex_item_estimated_height(
            style,
            content_height,
            physical_intrinsic.min_content_height.content_box_length(),
            physical_intrinsic
                .intrinsic_content_height
                .content_box_length(),
            containing_height_basis,
            vertical_non_content,
        );
        let min_width = constrain_content_width(
            style,
            physical_intrinsic.preferred_min_width.content_box_length(),
            PercentageBasis::definite(containing_width),
        )
        .points();
        let min_height = constrain_flex_item_estimated_height(
            style,
            physical_intrinsic.min_content_height.content_box_length(),
            physical_intrinsic.min_content_height.content_box_length(),
            physical_intrinsic
                .intrinsic_content_height
                .content_box_length(),
            containing_height_basis,
            vertical_non_content,
        );
        let fallback_line_baseline_offset =
            layout_pt(self.inline_box_text_line_layout_baseline_offset(style));
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
        let first_baseline = baseline_edge + first_line_baseline_offset.points();
        let last_baseline = baseline_edge
            + measurement
                .sequence
                .last_line_baseline_offset(fallback_line_baseline_offset.points());
        FlexItemEstimate::new(
            IntrinsicItemMetrics {
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
            FlexItemBaselineEstimate {
                vertical: FlexItemBaselinePair {
                    first: Some(flex_vertical_baseline_from_points(first_baseline)),
                    last: Some(flex_vertical_baseline_from_points(last_baseline)),
                },
                horizontal: FlexItemBaselinePair {
                    first: first_horizontal_text_baseline_offset(
                        style,
                        flex_estimated_border_box_width(style, content_box_pt(width)),
                        first_line_baseline_offset,
                    ),
                    last: last_horizontal_text_baseline_offset(
                        style,
                        flex_estimated_border_box_width(style, content_box_pt(width)),
                        preceding_line_height,
                        last_line_baseline_offset,
                    ),
                },
            },
        )
    }
}

impl<'a> FlexItemEstimateContext<'a> {
    pub(super) fn new(
        style: &'a ComputedStyle,
        available: FlexItemAvailableSpace,
        physical_direction: FlexDirection,
    ) -> Self {
        let containing_width = available.width.content_box_length();
        let containing_width_basis = available.width_basis;
        let containing_height_basis = available.height_basis;
        let containing_inline_size = available.inline_size(style);
        let containing_inline_size_points = containing_inline_size.points();
        let containing_inline_basis = available.inline_basis(style);
        let inline_measurement_space = match containing_inline_basis {
            PercentageBasis::Definite {
                source:
                    FlexAvailableSizeSource::DefinitePreferredCrossSize
                    | FlexAvailableSizeSource::PostFlexingMainSize,
                ..
            } => FlexInlineMeasurementSpace::ResolvedItemContentBox,
            PercentageBasis::Definite { .. } | PercentageBasis::Indefinite => {
                FlexInlineMeasurementSpace::ContainingBlockContentBox
            }
        };
        let preferred_inline_basis = if containing_inline_basis.is_definite() {
            containing_inline_basis
        } else {
            containing_width_basis
        };
        let vertical_non_content =
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style));
        Self {
            style,
            available,
            physical_direction,
            containing_width,
            containing_width_basis,
            containing_height_basis,
            containing_inline_size,
            containing_inline_size_points,
            inline_measurement_space,
            preferred_inline_basis,
            vertical_non_content,
        }
    }
}
