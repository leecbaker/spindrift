use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexCollapseMode {
    IncludeCollapsed,
    OmitCollapsed,
}

impl FlexCollapseMode {
    pub(in crate::layout::flex) fn omits_collapsed(self) -> bool {
        matches!(self, Self::OmitCollapsed)
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::flex) fn compute_flex_layout(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
    ) -> Option<FlexLayout> {
        let collapsed_struts = if children
            .iter()
            .any(|child| flex_item_is_collapsed(&child.style))
        {
            let visible_layout = self.compute_flex_layout_internal(
                children,
                style,
                stylesheets,
                available,
                FlexCollapseMode::IncludeCollapsed,
                &[],
            )?;
            collapsed_struts_from_visible_layout(children, style, &visible_layout)
        } else {
            Vec::new()
        };
        self.compute_flex_layout_internal(
            children,
            style,
            stylesheets,
            available,
            FlexCollapseMode::OmitCollapsed,
            &collapsed_struts,
        )
    }

    pub(in crate::layout::flex) fn compute_flex_layout_internal(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available: FlexAvailableSpace,
        collapse_mode: FlexCollapseMode,
        collapsed_struts: &[FlexCollapsedStrut],
    ) -> Option<FlexLayout> {
        let mut tree: taffy_layout::TaffyTree<FlexItemEstimate> = taffy_layout::TaffyTree::new();
        // CSS Flexbox used sizes are real-valued CSS lengths. Taffy rounds final
        // layouts by default for screen pixels; PDF emission must preserve the
        // unrounded layout and let rasterizers antialias at their output DPI.
        tree.disable_rounding();
        let flex_axes = FlexAxes::for_style(style);
        let physical_direction = flex_axes.physical_direction.taffy_direction();
        let item_measure_available =
            balanced_flex_item_measure_available_space(style, physical_direction, available);
        let PhysicalFlexGaps {
            horizontal: physical_gap_width,
            vertical: physical_gap_height,
        } = physical_flex_gaps(style);
        let mut nodes = Vec::with_capacity(children.len());
        let mut estimates = vec![FlexItemEstimate::fixed(0.0, 0.0); children.len()];
        // CSS Values permits `calc(infinity)` for flex factors. If any grow
        // factor is infinite, Flexbox distributes positive free space only
        // among the infinite factors, equally; normalizing them before the
        // finite Taffy adapter avoids `∞ / ∞` arithmetic while preserving that
        // used-value rule.
        // <https://www.w3.org/TR/css-values-4/#calc-infinities> and
        // <https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths>.
        let has_infinite_grow = children
            .iter()
            .any(|child| child.style.flex_grow.is_infinite());
        let mut active_estimates = Vec::with_capacity(children.len());
        let mut source_indices = Vec::with_capacity(children.len());
        let mut estimated_collapsed_struts = Vec::new();
        for (source_index, child) in children.iter().enumerate() {
            let child_style = &child.style;
            let child_padding = flex_item_used_padding(child_style, style, available);
            let child_margin = flex_item_used_margin(child_style, style, available);
            let item_available = flex_item_estimate_available_space(
                child_style,
                style,
                physical_direction,
                item_measure_available,
            );
            let mut estimated_size = self.estimate_flex_item_size(
                child,
                stylesheets,
                item_available,
                physical_direction,
            );
            // `flex-basis: content` ignores the preferred main-size property.
            // Replaced-item estimates normally expose that authored size as
            // their used width/height (before the flex adapter replaces it),
            // so obtain the content contribution from a main-size-suppressed
            // formatting-context estimate instead. Attributes such as canvas
            // dimensions remain part of that intrinsic contribution.
            // <https://drafts.csswg.org/css-flexbox-1/#flex-basis-property>
            if matches!(child_style.flex_basis, css::ComputedFlexBasis::Content) {
                let mut content_child = child.clone();
                if physical_direction.is_row_axis() {
                    content_child.style.box_values.width =
                        css::ComputedLengthPercentageOrAuto::Auto;
                } else {
                    content_child.style.box_values.height =
                        css::ComputedLengthPercentageOrAuto::Auto;
                }
                let content_estimate = self.estimate_flex_item_size(
                    &content_child,
                    stylesheets,
                    item_available,
                    physical_direction,
                );
                if physical_direction.is_row_axis() {
                    estimated_size.content_width = content_estimate.content_width;
                    estimated_size.min_width = content_estimate.min_width;
                } else {
                    estimated_size.content_height = content_estimate.content_height;
                    estimated_size.min_height = content_estimate.min_height;
                }
            }
            // Inline-size containment suppresses only the item's logical
            // inline contribution.  In a horizontal flex item this is its
            // physical width; keep the independently measured physical
            // height so cross-axis stretching and auto block sizing still see
            // descendant layout.  `contain:size` has already replaced both
            // axes in the estimate above.
            // <https://drafts.csswg.org/css-contain-3/#inline-size-containment>
            if child_style.contain.inline_size
                && !child_style.contain.size
                && child_style.writing_mode == WritingMode::HorizontalTb
            {
                let fallback_width = child_style
                    .contain_intrinsic_size
                    .width
                    .clone()
                    .map(|width| {
                        used_length_percentage(
                            width,
                            PercentageBasis::definite(layout_pt(available.width.points().max(0.0))),
                        )
                        .points()
                    })
                    .unwrap_or(0.0);
                estimated_size.width = content_box_pt(fallback_width);
                estimated_size.min_width = content_box_pt(fallback_width);
                estimated_size.content_width = content_box_pt(fallback_width);
            }
            estimates[source_index] = estimated_size;
            if collapse_mode.omits_collapsed() && flex_item_is_collapsed(child_style) {
                estimated_collapsed_struts.push(FlexCollapsedStrut {
                    item_index: source_index,
                    cross_size: FlexCrossSize::new(
                        estimated_outer_cross_size(child_style, estimated_size, physical_direction)
                            .points(),
                    ),
                    source_start: source_index,
                    source_end: source_index + 1,
                });
                continue;
            }
            let flex_basis_overrides_main_size =
                !matches!(child_style.flex_basis, css::ComputedFlexBasis::Auto);
            let preferred_aspect_ratio = child_style.aspect_ratio.preferred_ratio(
                child.is_replaced_element(),
                estimated_size.preferred_aspect_ratio,
            );
            let stretched_cross_size = stretched_flex_item_cross_size(
                child_style,
                style,
                physical_direction,
                item_measure_available,
            );
            let uses_balanced_line_cross_stretch = style.flex_wrap.balances_lines()
                && style.flex_line_count.is_some()
                && stretched_cross_size.is_some();
            let cross_size_is_auto = if physical_direction.is_row_axis() {
                child_style.box_values.height.is_auto()
            } else {
                child_style.box_values.width.is_auto()
            };
            let cross_axis_has_constraint = if physical_direction.is_row_axis() {
                !child_style.box_values.min_height.is_auto()
                    || !child_style.box_values.max_height.is_auto()
            } else {
                !child_style.box_values.min_width.is_auto()
                    || !child_style.box_values.max_width.is_auto()
            };
            // Quire resolves an authored non-replaced ratio in its flex
            // adapters so its box-sizing semantics and transferred size
            // suggestions remain intact. Leave natural replaced ratios with
            // Taffy, which still needs them to derive the ordinary intrinsic
            // replaced size.
            let has_authored_non_replaced_ratio = !child.is_replaced_element()
                && child_style
                    .aspect_ratio
                    .preferred_ratio_for_non_replaced(false)
                    .is_some();
            let taffy_aspect_ratio = if has_authored_non_replaced_ratio
                || stretched_cross_size.is_some()
                || !cross_size_is_auto
                || cross_axis_has_constraint
            {
                None
            } else {
                preferred_aspect_ratio
            };
            let child_borders = used_border_widths(child_style);
            let horizontal_non_content =
                child_padding.left + child_padding.right + child_borders.left + child_borders.right;
            let vertical_non_content =
                child_padding.top + child_padding.bottom + child_borders.top + child_borders.bottom;
            let horizontal_stretch = FlexStretchFitContext {
                available_margin_box_size: Some(
                    crate::units::IntoLayoutLength::into_layout_length(
                        item_measure_available.width.content_box_length(),
                    ),
                ),
                margin_size: layout_pt(child_margin.left + child_margin.right),
                non_content_size: non_content_pt(horizontal_non_content),
                box_sizing: child_style.box_sizing,
            };
            let wrapping_column_cross_fit_content =
                (matches!(physical_direction, FlexDirection::Column)
                    && style.flex_wrap.wraps()
                    && item_measure_available.width_basis.is_definite())
                .then(|| {
                    let available_content = stretch_fit_content_box_size(
                        crate::units::IntoLayoutLength::into_layout_length(
                            item_measure_available.width.content_box_length(),
                        ),
                        horizontal_stretch.margin_size,
                        horizontal_stretch.non_content_size,
                    );
                    content_box_pt(
                        estimated_size
                            .content_width
                            .points()
                            .max(estimated_size.min_width.points())
                            .min(
                                available_content
                                    .points()
                                    .max(estimated_size.min_width.points()),
                            )
                            .max(0.0),
                    )
                });
            let vertical_stretch = FlexStretchFitContext {
                available_margin_box_size: item_measure_available
                    .height
                    .map(PhysicalContentHeight::points)
                    .map(layout_pt),
                margin_size: layout_pt(child_margin.top + child_margin.bottom),
                non_content_size: non_content_pt(vertical_non_content),
                box_sizing: child_style.box_sizing,
            };
            // CSS Tables defines a table's used `min-width` as at least its
            // min-content width. Taffy's generic definite-minimum path would
            // otherwise replace that table-specific floor with the authored
            // value (for example, `min-width: 50px`), allowing a 100px
            // min-content table to shrink to 50px as a flex item.
            // <https://drafts.csswg.org/css-tables/#used-min-width-of-table>
            let flex_min_width =
                if child_style.display.is_table() && !child_style.box_values.min_width.is_auto() {
                    css::ComputedLengthPercentageOrAuto::MinContent
                } else {
                    child_style.box_values.min_width.clone()
                };
            // Taffy's leaf measurement callback must use the same resolved
            // flex basis supplied to its flex algorithm. In particular,
            // `flex-basis:auto` can acquire a definite main size by
            // transferring a definite cross size through aspect-ratio.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
            let flex_basis_context = FlexBasisContext {
                direction: physical_direction,
                available_main_size: if physical_direction.is_row_axis() {
                    available.width.points()
                } else {
                    available
                        .height
                        .map(PhysicalContentHeight::points)
                        .unwrap_or_else(|| estimated_size.content_height.points())
                },
                available_cross_size: if physical_direction.is_row_axis() {
                    available.height_basis_points()
                } else {
                    available.width_basis_points()
                },
                stretched_cross_size,
                main_size_basis: if physical_direction.is_row_axis() {
                    available.width_basis
                } else {
                    available.height_basis
                },
                preferred_aspect_ratio,
            };
            let taffy_flex_basis =
                taffy_flex_basis(child_style, &estimated_size, flex_basis_context);
            // Taffy asks a leaf measure function for an automatic main size
            // even when the flex basis is a definite length. Its generic
            // fallback would then return this item's authored main-size
            // estimate, which is incorrect: Flexbox uses the resolved flex
            // basis. Keep the full estimate for intrinsic contributions and
            // automatic minimums, but give Taffy's measure callback the
            // resolved content-box flex basis on the main axis.
            // <https://drafts.csswg.org/css-flexbox-1/#algo-main-item>.
            let mut taffy_measure_estimate = estimated_size;
            if has_authored_non_replaced_ratio {
                // The leaf measure callback otherwise reintroduces the
                // authored ratio when Taffy supplies a known cross size.
                // Quire owns this ratio's flex sizing so it can preserve CSS
                // Sizing's box-model and transferred-size semantics.
                taffy_measure_estimate.preferred_aspect_ratio = None;
            }
            if let Some(flex_basis) = taffy_flex_basis.into_option() {
                let main_non_content = if physical_direction.is_row_axis() {
                    horizontal_non_content
                } else {
                    vertical_non_content
                };
                let content_basis = match child_style.box_sizing {
                    BoxSizing::ContentBox => flex_basis,
                    BoxSizing::BorderBox => (flex_basis - main_non_content).max(0.0),
                };
                if physical_direction.is_row_axis() {
                    taffy_measure_estimate.width = content_box_pt(content_basis);
                } else {
                    taffy_measure_estimate.height = content_box_pt(content_basis);
                }
            }
            let node = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        display: taffy_layout::Display::Flex,
                        box_sizing: match child_style.box_sizing {
                            BoxSizing::BorderBox => taffy_layout::BoxSizing::BorderBox,
                            BoxSizing::ContentBox => taffy_layout::BoxSizing::ContentBox,
                        },
                        direction: taffy_direction(child_style.direction),
                        size: taffy_layout::Size {
                            width: flex_item_size_dimension(
                                child_style.box_values.width.clone(),
                                estimated_size.width,
                                estimated_size.min_width,
                                estimated_size.content_width,
                                FlexItemSizeDimensionContext {
                                    flex_direction: physical_direction,
                                    dimension_axis: FlexDirection::Row,
                                    percentage_basis: available.width_basis,
                                    stretch: horizontal_stretch,
                                    flex_basis_overrides_main_size,
                                    auto_cross_uses_stretch_fit: uses_balanced_line_cross_stretch,
                                    auto_cross_fit_content: wrapping_column_cross_fit_content,
                                },
                            ),
                            height: flex_item_size_dimension(
                                child_style.box_values.height.clone(),
                                estimated_size.height,
                                estimated_size.min_height,
                                estimated_size.content_height,
                                FlexItemSizeDimensionContext {
                                    flex_direction: physical_direction,
                                    dimension_axis: FlexDirection::Column,
                                    percentage_basis: available.height_basis,
                                    stretch: vertical_stretch,
                                    flex_basis_overrides_main_size,
                                    auto_cross_uses_stretch_fit: uses_balanced_line_cross_stretch,
                                    auto_cross_fit_content: None,
                                },
                            ),
                        },
                        aspect_ratio: taffy_aspect_ratio,
                        min_size: taffy_layout::Size {
                            width: flex_min_size_dimension(
                                flex_min_width,
                                estimated_size.min_width,
                                estimated_size.content_width,
                                FlexMinSizeDimensionContext {
                                    definite_preferred_content_size:
                                        used_content_box_width_or_auto_with_basis(
                                            child_style,
                                            available.width_basis,
                                            non_content_pt(horizontal_non_content),
                                        ),
                                    transferred_size_suggestion:
                                        automatic_minimum_transferred_size_suggestion(
                                            child_style,
                                            FlexDirection::Row,
                                            available.height_basis_points(),
                                            physical_direction
                                                .is_row_axis()
                                                .then_some(stretched_cross_size)
                                                .flatten(),
                                            preferred_aspect_ratio,
                                            estimated_size.min_height,
                                            estimated_size.content_height,
                                        ),
                                    is_replaced: child.is_replaced_element(),
                                    is_main_axis: physical_direction.is_row_axis(),
                                    is_item_block_axis: matches!(
                                        child_style.writing_mode,
                                        WritingMode::VerticalRl
                                            | WritingMode::VerticalLr
                                            | WritingMode::SidewaysRl
                                            | WritingMode::SidewaysLr
                                    ),
                                    overflow: flex_item_main_axis_overflow(
                                        child_style,
                                        physical_direction,
                                    ),
                                    percentage_basis: available.width_basis,
                                    stretch: horizontal_stretch,
                                },
                            ),
                            height: flex_min_size_dimension(
                                child_style.box_values.min_height.clone(),
                                estimated_size.min_height,
                                estimated_size.content_height,
                                FlexMinSizeDimensionContext {
                                    definite_preferred_content_size:
                                        used_content_box_height_or_auto_with_basis(
                                            child_style,
                                            available.height_basis,
                                            non_content_pt(vertical_non_content),
                                        ),
                                    transferred_size_suggestion:
                                        automatic_minimum_transferred_size_suggestion(
                                            child_style,
                                            FlexDirection::Column,
                                            available.width_basis_points(),
                                            physical_direction
                                                .is_column_axis()
                                                .then_some(stretched_cross_size)
                                                .flatten(),
                                            preferred_aspect_ratio,
                                            estimated_size.min_width,
                                            estimated_size.content_width,
                                        ),
                                    is_replaced: child.is_replaced_element(),
                                    is_main_axis: physical_direction.is_column_axis(),
                                    is_item_block_axis: matches!(
                                        child_style.writing_mode,
                                        WritingMode::HorizontalTb
                                    ),
                                    overflow: flex_item_main_axis_overflow(
                                        child_style,
                                        physical_direction,
                                    ),
                                    percentage_basis: available.height_basis,
                                    stretch: vertical_stretch,
                                },
                            ),
                        },
                        max_size: taffy_layout::Size {
                            width: taffy_intrinsic_dimension_with_basis_and_stretch(
                                child_style.box_values.max_width.clone(),
                                available.width_basis,
                                estimated_size.min_width,
                                estimated_size.content_width,
                                horizontal_stretch,
                            ),
                            height: taffy_intrinsic_dimension_with_basis_and_stretch(
                                child_style.box_values.max_height.clone(),
                                available.height_basis,
                                estimated_size.min_height,
                                estimated_size.content_height,
                                vertical_stretch,
                            ),
                        },
                        margin: taffy_margin(child_style, style, available),
                        padding: taffy_padding(child_style, style, available),
                        border: taffy_edges(used_border_widths(child_style)),
                        flex_grow: if has_infinite_grow {
                            if child_style.flex_grow.is_infinite() {
                                1.0
                            } else {
                                0.0
                            }
                        } else {
                            child_style.flex_grow
                        },
                        flex_shrink: child_style.flex_shrink,
                        flex_basis: taffy_flex_basis,
                        align_self: taffy_effective_align_self(
                            child_style,
                            style,
                            physical_direction,
                            available,
                        ),
                        ..Default::default()
                    },
                    taffy_measure_estimate,
                )
                .ok()?;
            nodes.push(node);
            active_estimates.push(estimated_size);
            source_indices.push(source_index);
        }

        let root = tree
            .new_with_children(
                taffy_layout::Style {
                    display: taffy_layout::Display::Flex,
                    box_sizing: taffy_layout::BoxSizing::BorderBox,
                    direction: taffy_flex_layout_direction(style, physical_direction),
                    size: taffy_layout::Size {
                        width: taffy_layout::Dimension::length(available.width.points()),
                        height: if available.height_basis.is_definite() {
                            available
                                .height
                                .map(PhysicalContentHeight::points)
                                .map(taffy_layout::Dimension::length)
                                .unwrap_or_else(taffy_layout::Dimension::auto)
                        } else {
                            taffy_layout::Dimension::auto()
                        },
                    },
                    min_size: taffy_layout::Size {
                        width: taffy_min_dimension(
                            style.box_values.min_width.clone(),
                            available.width_basis,
                        ),
                        height: taffy_min_dimension(
                            style.box_values.min_height.clone(),
                            available.width_basis,
                        ),
                    },
                    max_size: taffy_layout::Size {
                        width: taffy_optional_dimension(style.box_values.max_width.clone()),
                        height: taffy_optional_dimension(style.box_values.max_height.clone()),
                    },
                    flex_direction: match physical_direction {
                        FlexDirection::Row => taffy_layout::FlexDirection::Row,
                        FlexDirection::RowReverse => taffy_layout::FlexDirection::RowReverse,
                        FlexDirection::Column => taffy_layout::FlexDirection::Column,
                        FlexDirection::ColumnReverse => taffy_layout::FlexDirection::ColumnReverse,
                    },
                    flex_wrap: match style.flex_wrap {
                        FlexWrap::NoWrap => taffy_layout::FlexWrap::NoWrap,
                        FlexWrap::Wrap => taffy_layout::FlexWrap::Wrap,
                        FlexWrap::WrapReverse => taffy_layout::FlexWrap::WrapReverse,
                        FlexWrap::Balance => taffy_layout::FlexWrap::Wrap,
                        FlexWrap::BalanceReverse => taffy_layout::FlexWrap::WrapReverse,
                    },
                    justify_content: taffy_justify_content(
                        style.justify_content,
                        physical_direction,
                    ),
                    align_content: Some(taffy_align_content(style.align_content)),
                    align_items: Some(taffy_align_items(style.align_items)),
                    gap: taffy_layout::Size {
                        width: taffy_gap(physical_gap_width.clone(), available.width_basis),
                        height: taffy_gap(physical_gap_height.clone(), available.height_basis),
                    },
                    ..Default::default()
                },
                &nodes,
            )
            .ok()?;

        tree.compute_layout_with_measure(
            root,
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::Definite(available.width.points()),
                height: available
                    .height
                    .map(PhysicalContentHeight::points)
                    .map(taffy_layout::AvailableSpace::Definite)
                    .unwrap_or(taffy_layout::AvailableSpace::MaxContent),
            },
            |known_dimensions, available_space, _node_id, node_context, _style| {
                measure_flex_item(known_dimensions, available_space, node_context)
            },
        )
        .ok()?;

        let root_rect = taffy_rect_from_layout(tree.layout(root).ok()?);
        let mut items = vec![FlexItemLayout::new(ContainerRect::zero()); children.len()];
        let mut active_items = Vec::with_capacity(nodes.len());
        for &node in &nodes {
            let layout = tree.layout(node).ok()?;
            let rect = taffy_rect_from_layout(layout);
            active_items.push(FlexItemLayout::from_taffy_rect(rect, flex_axes));
        }
        let active_children = source_indices
            .iter()
            .map(|&index| children[index].clone())
            .collect::<Vec<_>>();
        let container_cross_size = if physical_direction.is_row_axis() {
            root_rect.size.height
        } else {
            root_rect.size.width
        };
        let mut active_lines = flex_lines_from_items(
            &active_items,
            &active_children,
            &active_estimates,
            style,
            physical_direction,
            container_cross_size,
        );
        let balanced_taffy_context = BalancedTaffyLayoutContext {
            template_tree: &tree,
            template_root: root,
            template_nodes: &nodes,
            estimates: &active_estimates,
            flex_axes,
            available,
        };
        let balanced_hypothetical_main_sizes = if style.flex_wrap.balances_lines() {
            let active_indices = (0..nodes.len()).collect::<Vec<_>>();
            balanced_taffy_line_layouts(&balanced_taffy_context, &active_indices, true).map(
                |layouts| {
                    layouts
                        .iter()
                        .map(|layout| layout.main_size(flex_axes))
                        .collect::<Vec<_>>()
                },
            )
        } else {
            None
        };
        if style.flex_wrap.balances_lines()
            && rebalance_flex_line_membership(
                &mut active_lines,
                &mut active_items,
                &active_children,
                FlexBalanceContext {
                    physical_direction,
                    requested_line_count: style.flex_line_count,
                    hypothetical_main_sizes: balanced_hypothetical_main_sizes.as_deref(),
                    main_gap: used_flex_gap(
                        if physical_direction.is_row_axis() {
                            physical_gap_width.clone()
                        } else {
                            physical_gap_height.clone()
                        },
                        PercentageBasis::definite(content_box_pt(
                            if physical_direction.is_row_axis() {
                                root_rect.size.width
                            } else {
                                root_rect.size.height
                            },
                        )),
                    )
                    .points(),
                    cross_gap: used_flex_gap(
                        if physical_direction.is_row_axis() {
                            physical_gap_height.clone()
                        } else {
                            physical_gap_width.clone()
                        },
                        PercentageBasis::definite(content_box_pt(
                            if physical_direction.is_row_axis() {
                                root_rect.size.height
                            } else {
                                root_rect.size.width
                            },
                        )),
                    )
                    .points(),
                    available_main_size: if physical_direction.is_row_axis() {
                        root_rect.size.width
                    } else {
                        root_rect.size.height
                    },
                },
            )
        {
            resolve_balanced_line_flexible_lengths(
                &balanced_taffy_context,
                &active_lines,
                &mut active_items,
            );
            repack_lines_after_main_size_adjustment(
                &mut active_lines,
                &mut active_items,
                &active_children,
                style,
                physical_direction,
                if physical_direction.is_row_axis() {
                    root_rect.size.width
                } else {
                    root_rect.size.height
                },
            );
            refresh_flex_line_metadata(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
        }
        let collapsed_struts = if collapsed_struts.is_empty() {
            estimated_collapsed_struts.as_slice()
        } else {
            collapsed_struts
        };
        attach_collapsed_struts_to_active_lines(
            &mut active_lines,
            &source_indices,
            collapsed_struts,
        );
        repack_lines_after_collapsed_struts(
            &mut active_lines,
            &mut active_items,
            physical_direction,
        );
        if apply_line_cross_size_dependent_item_remeasurements(
            self,
            &mut active_items,
            &mut active_estimates,
            &active_children,
            &active_lines,
            FlexLineCrossRemeasureContext {
                container_style: style,
                stylesheets,
                physical_direction,
                available,
                container_cross_size_basis: if physical_direction.is_row_axis() {
                    available.height_basis
                } else {
                    available.width_basis
                },
                line_cross_gap: flex_line_cross_gap(
                    style,
                    physical_direction,
                    available,
                    physical_gap_width,
                    physical_gap_height,
                ),
            },
        ) {
            refresh_flex_line_metadata(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
        }
        if apply_main_size_aspect_ratio_cross_size_corrections(
            &mut active_items,
            &mut active_estimates,
            &active_children,
            style,
            physical_direction,
            available,
        ) {
            refresh_flex_line_metadata(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
        }
        replace_synthesized_baseline_offsets(
            &mut active_items,
            &active_estimates,
            &active_children,
            &active_lines,
            style,
            physical_direction,
        );
        apply_column_baseline_self_alignment_offsets(
            &mut active_items,
            &active_estimates,
            &active_children,
            &active_lines,
            style,
            physical_direction,
        );
        refresh_flex_line_cross_bounds(
            &mut active_lines,
            &active_items,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        refresh_flex_line_metadata(
            &mut active_lines,
            &active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        apply_baseline_align_content_offsets(
            &mut active_items,
            &mut active_lines,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        refresh_flex_line_metadata(
            &mut active_lines,
            &active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        apply_baseline_self_alignment_fallback_offsets(
            &mut active_items,
            &active_children,
            &active_lines,
            style,
            physical_direction,
        );
        apply_subject_axis_self_alignment_offsets(
            &mut active_items,
            &active_children,
            &active_lines,
            style,
            physical_direction,
        );
        refresh_flex_line_cross_bounds(
            &mut active_lines,
            &active_items,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        refresh_flex_line_metadata(
            &mut active_lines,
            &active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        if apply_main_axis_automatic_minimums(
            &mut active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
            available,
        ) {
            repack_lines_after_main_size_adjustment(
                &mut active_lines,
                &mut active_items,
                &active_children,
                style,
                physical_direction,
                if physical_direction.is_row_axis() {
                    root_rect.size.width
                } else {
                    root_rect.size.height
                },
            );
            refresh_flex_line_cross_bounds(
                &mut active_lines,
                &active_items,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
            refresh_flex_line_metadata(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
        }
        apply_stretch_align_content_overflow_fallback_offsets(
            &mut active_items,
            &mut active_lines,
            &active_children,
            style,
            physical_direction,
            container_cross_size,
        );
        if apply_non_negative_flex_item_content_box_minimums(&mut active_items, &active_children) {
            refresh_flex_line_metadata(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                container_cross_size,
            );
        }
        expand_flex_line_cross_bounds_for_item_overflow(
            &mut active_lines,
            &active_items,
            &active_children,
            physical_direction,
        );
        mirror_vertical_cross_axis_for_rtl_inline_flow(
            &mut active_items,
            &mut active_lines,
            style,
            physical_direction,
            container_cross_size,
        );
        assign_flex_item_percentage_height_bases(
            &mut active_items,
            &active_children,
            style,
            physical_direction,
            available,
        );
        assign_flex_item_fragmentation_heights(
            &mut active_items,
            &active_estimates,
            &active_children,
        );
        let source_lines = active_lines
            .iter()
            .map(|line| {
                let item_indices = line
                    .item_indices
                    .iter()
                    .map(|&active_index| source_indices[active_index])
                    .collect::<Vec<_>>();
                FlexLineLayout {
                    source_start: item_indices
                        .iter()
                        .cloned()
                        .min()
                        .unwrap_or(line.source_start),
                    source_end: item_indices
                        .iter()
                        .cloned()
                        .max()
                        .map(|index| index + 1)
                        .unwrap_or(line.source_end),
                    item_indices,
                    main_start: line.main_start,
                    main_end: line.main_end,
                    cross_start: line.cross_start,
                    cross_end: line.cross_end,
                    first_baseline: line.first_baseline,
                    last_baseline: line.last_baseline,
                    collapsed_struts: line.collapsed_struts.clone(),
                }
            })
            .collect::<Vec<_>>();
        for (active_index, source_index) in source_indices.iter().cloned().enumerate() {
            items[source_index] = active_items[active_index].clone();
        }
        let item_extent_height = items
            .iter()
            .map(|item| item.y() + item.height())
            .fold(0.0f32, f32::max);
        let collapsed_cross_height = if physical_direction.is_row_axis() {
            source_lines
                .iter()
                .map(|line| line.largest_collapsed_strut().points())
                .fold(0.0f32, f32::max)
        } else {
            0.0
        };
        let height = if available.height_basis.is_definite() {
            root_rect.size.height
        } else if available.height.is_some() {
            item_extent_height.max(collapsed_cross_height)
        } else if style.flex_wrap.balances_lines() && physical_direction.is_column_axis() {
            // The initial Taffy tree is laid out before Quire repartitions a
            // balanced column container. Its auto height can therefore still
            // describe the unbalanced single column. After balance has
            // committed the line membership, the physical block extent is the
            // resulting item extent instead.
            // <https://drafts.csswg.org/css-flexbox-2/#algo-balance>
            item_extent_height.max(collapsed_cross_height)
        } else {
            root_rect
                .size
                .height
                .max(item_extent_height)
                .max(collapsed_cross_height)
        };

        let first_baseline = flex_container_first_baseline(
            &active_lines,
            &active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
        );
        // A fragment plan is built only while real fragmentainers are
        // consumed. A synthetic unfragmented entry loses residual capacity
        // and forced-break information required by continuation replay.
        let fragment_plan = FlexFragmentPlan::default();

        Some(FlexLayout {
            height,
            first_baseline,
            items,
            lines: source_lines,
            fragment_plan,
        })
    }
}

/// Resolve one candidate balanced line through the same flex-length algorithm
/// used by the initial Taffy layout.
///
/// Balancing chooses contiguous item sequences from their hypothetical outer
/// main sizes, but each selected sequence then needs its own flexible-length
/// resolution. Keeping the Taffy item styles intact preserves flex factors,
/// automatic minimum sizes, min/max clamping, auto margins, and
/// `justify-content`; only wrapping is disabled because the caller supplies
/// exactly one line:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance> and
/// <https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths>.
struct BalancedTaffyLayoutContext<'a> {
    template_tree: &'a taffy_layout::TaffyTree<FlexItemEstimate>,
    template_root: taffy_layout::NodeId,
    template_nodes: &'a [taffy_layout::NodeId],
    estimates: &'a [FlexItemEstimate],
    flex_axes: FlexAxes,
    available: FlexAvailableSpace,
}

fn balanced_taffy_line_layouts(
    context: &BalancedTaffyLayoutContext<'_>,
    item_indices: &[usize],
    hypothetical_sizes: bool,
) -> Option<Vec<FlexItemLayout>> {
    let mut tree = taffy_layout::TaffyTree::new();
    tree.disable_rounding();
    let mut nodes = Vec::with_capacity(item_indices.len());
    for &item_index in item_indices {
        let template_node = *context.template_nodes.get(item_index)?;
        let estimate = *context.estimates.get(item_index)?;
        let mut style = context.template_tree.style(template_node).ok()?.clone();
        if hypothetical_sizes {
            style.flex_grow = 0.0;
            style.flex_shrink = 0.0;
        }
        nodes.push(tree.new_leaf_with_context(style, estimate).ok()?);
    }

    let mut root_style = context
        .template_tree
        .style(context.template_root)
        .ok()?
        .clone();
    root_style.flex_wrap = taffy_layout::FlexWrap::NoWrap;
    let root = tree.new_with_children(root_style, &nodes).ok()?;
    tree.compute_layout_with_measure(
        root,
        taffy_layout::Size {
            width: taffy_layout::AvailableSpace::Definite(context.available.width.points()),
            height: context
                .available
                .height
                .map(PhysicalContentHeight::points)
                .map(taffy_layout::AvailableSpace::Definite)
                .unwrap_or(taffy_layout::AvailableSpace::MaxContent),
        },
        |known_dimensions, available_space, _node_id, node_context, _style| {
            measure_flex_item(known_dimensions, available_space, node_context)
        },
    )
    .ok()?;
    nodes
        .iter()
        .map(|&node| {
            tree.layout(node)
                .ok()
                .map(taffy_rect_from_layout)
                .map(|rect| FlexItemLayout::from_taffy_rect(rect, context.flex_axes))
        })
        .collect()
}

/// Replace the selected balanced lines' main-axis geometry with a fresh flex
/// resolution for each line.
///
/// This deliberately keeps the cross-axis geometry selected by the outer flex
/// layout. The balancing phase changes only line membership; cross sizing and
/// line packing continue through the regular flex pipeline afterward:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>.
fn resolve_balanced_line_flexible_lengths(
    context: &BalancedTaffyLayoutContext<'_>,
    lines: &[FlexLineLayout],
    items: &mut [FlexItemLayout],
) {
    for line in lines {
        let Some(layouts) = balanced_taffy_line_layouts(context, &line.item_indices, false) else {
            continue;
        };
        for (&item_index, layout) in line.item_indices.iter().zip(layouts) {
            let Some(item) = items.get_mut(item_index) else {
                continue;
            };
            item.set_main_size(context.flex_axes, layout.main_size(context.flex_axes));
            item.set_main_start(context.flex_axes, layout.main_start(context.flex_axes));
        }
    }
}

/// Record the source block extent that fragmented replay must cover.
///
/// Flex layout keeps an item's used border-box height even when descendants
/// visibly overflow it. Fragmentation must nevertheless keep replaying the
/// item until that descendant content has been consumed by fragmentainers:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination> and
/// <https://www.w3.org/TR/css-break-3/#box-splitting>.
pub(in crate::layout::flex) fn assign_flex_item_fragmentation_heights(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
) {
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        // Size containment prevents descendant intrinsic sizes from
        // contributing to the item's layout/fragmentation size.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if child.style.contain.size {
            continue;
        }
        let content_overflow = estimate.content_height.points().max(0.0);
        let border_box_overflow = content_overflow
            + child.style.padding.top
            + child.style.padding.bottom
            + vertical_border_width(&child.style);
        item.set_fragmentation_height(border_box_overflow);
    }
}

/// Applies Flexbox's automatic minimum main size to final item layouts.
///
/// CSS Flexbox section 4.5 defines `min-width:auto`/`min-height:auto` on flex
/// items as a content-based automatic minimum in the main axis when overflow is
/// non-scrollable. Taffy remains the primary flex algorithm here, but this guard
/// preserves content and transferred size suggestions when a definite zero-sized
/// flex container would otherwise shrink the final item layout below its
/// automatic minimum:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion>.
pub(in crate::layout::flex) fn apply_main_axis_automatic_minimums(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> bool {
    let mut changed = false;
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        let Some(minimum) = automatic_minimum_main_size(
            child,
            estimate,
            container_style,
            physical_direction,
            available,
        ) else {
            continue;
        };
        if physical_direction.is_row_axis() {
            if item.width() >= minimum {
                continue;
            }
            let delta = minimum - item.width();
            if matches!(physical_direction, FlexDirection::RowReverse) {
                item.set_main_start(axes, item.main_start(axes) - delta);
            }
            item.set_main_size(axes, minimum);
            changed = true;
        } else {
            if item.height() >= minimum {
                continue;
            }
            let delta = minimum - item.height();
            if matches!(physical_direction, FlexDirection::ColumnReverse) {
                item.set_main_start(axes, item.main_start(axes) - delta);
            }
            item.set_main_size(axes, minimum);
            changed = true;
        }
    }
    changed
}

/// Ensures final flex item border boxes can contain their non-content edges.
///
/// CSS Sizing floors the content box at zero, including stretch-fit sizing
/// where a small target margin box can be smaller than the item's padding and
/// border. Taffy may report a zero final border-box cross size for these cases,
/// so Quire restores the minimum border-box size before painting/replay:
/// <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-stretch>.
pub(in crate::layout::flex) fn apply_non_negative_flex_item_content_box_minimums(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
) -> bool {
    let mut changed = false;
    for (item, child) in items.iter_mut().zip(children) {
        let borders = used_border_widths(&child.style);
        let min_width =
            child.style.padding.left + child.style.padding.right + borders.left + borders.right;
        if item.width() < min_width {
            item.set_width(min_width);
            changed = true;
        }

        let min_height =
            child.style.padding.top + child.style.padding.bottom + borders.top + borders.bottom;
        if item.height() < min_height {
            item.set_height(min_height);
            changed = true;
        }
    }
    changed
}

pub(in crate::layout::flex) fn expand_flex_line_cross_bounds_for_item_overflow(
    lines: &mut [FlexLineLayout],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) {
    for line in lines {
        for &index in &line.item_indices {
            let Some(item) = items.get(index) else {
                continue;
            };
            let Some(child) = children.get(index) else {
                continue;
            };
            let (cross_start, cross_end) =
                item_outer_cross_bounds(item, &child.style, physical_direction);
            line.cross_start = line.cross_start.min(FlexCrossOffset::new(cross_start));
            line.cross_end = line.cross_end.max(FlexCrossOffset::new(cross_end));
        }
    }
}

/// Returns the physical available size to use while estimating a flex item's
/// descendants for flex base sizing.
///
/// CSS Flexbox treats a stretched flex item's cross size as definite for
/// laying out descendants when computing the flex base size, provided the flex
/// container has a definite cross size:
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
pub(in crate::layout::flex) fn flex_item_estimate_available_space(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> FlexItemAvailableSpace {
    let mut item_available = FlexItemAvailableSpace::from_container(available);
    // A column flex item's authored physical width is its cross size. When it
    // is a non-percentage length, its contents lay out against that definite
    // width while the automatic main-size flex basis is measured. Keep this
    // item-local descendant constraint separate from the container percentage
    // basis used to resolve percentage-valued widths:
    // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item> and
    // <https://www.w3.org/TR/css-sizing-3/#definite>.
    let horizontal_non_content =
        child_style.padding.left + child_style.padding.right + horizontal_border_width(child_style);
    if physical_direction.is_column_axis()
        && child_style
            .box_values
            .width
            .length_if_no_percent()
            .is_some()
        && let Some(width) = used_content_box_width_or_auto_with_basis(
            child_style,
            available.width_basis,
            non_content_pt(horizontal_non_content),
        )
    {
        item_available.set_definite_width(
            PhysicalContentWidth::new(width),
            FlexAvailableSizeSource::DefinitePreferredCrossSize,
        );
    }
    // A specified physical height is a definite percentage basis for the
    // item's descendants regardless of whether it happens to be flex's main
    // or cross axis. In particular, a row flex item's `height` must resolve a
    // child's percentage height while its automatic minimum is measured.
    // Column items additionally fall back to a definite flex base size when
    // their preferred main height is automatic. Do not replace a row item's
    // physical width: its own percentage-valued `width` still resolves
    // against the flex container rather than its flex basis.
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>.
    let vertical_non_content =
        child_style.padding.top + child_style.padding.bottom + vertical_border_width(child_style);
    let preferred_height = used_content_box_height_or_auto_with_basis(
        child_style,
        available.height_basis,
        non_content_pt(vertical_non_content),
    );
    let definite_height = preferred_height
        .map(|height| {
            (
                height,
                if physical_direction.is_column_axis() {
                    FlexAvailableSizeSource::DefinitePreferredMainSize
                } else {
                    FlexAvailableSizeSource::DefinitePreferredCrossSize
                },
            )
        })
        .or_else(|| {
            physical_direction
                .is_column_axis()
                .then(|| {
                    definite_post_flexing_main_size(child_style, physical_direction, available).map(
                        |height| {
                            (
                                content_box_pt(height),
                                FlexAvailableSizeSource::DefiniteFlexBase,
                            )
                        },
                    )
                })
                .flatten()
        });
    if let Some((height, source)) = definite_height {
        item_available.set_definite_height(PhysicalContentHeight::new(height), source);
    }
    let Some(stretched_cross_size) =
        stretched_flex_item_cross_size(child_style, container_style, physical_direction, available)
    else {
        return item_available;
    };

    if physical_direction.is_row_axis() {
        item_available.set_definite_height(
            PhysicalContentHeight::new(content_box_pt(stretched_cross_size)),
            FlexAvailableSizeSource::DefiniteCrossSize,
        );
        item_available.stretched_height = Some(PhysicalContentHeight::new(content_box_pt(
            stretched_cross_size,
        )));
    } else {
        item_available.set_definite_width(
            PhysicalContentWidth::new(content_box_pt(stretched_cross_size)),
            FlexAvailableSizeSource::DefiniteCrossSize,
        );
        item_available.stretched_width = Some(PhysicalContentWidth::new(content_box_pt(
            stretched_cross_size,
        )));
    }
    item_available
}

pub(in crate::layout::flex) fn stretched_flex_item_cross_size(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    if !matches!(
        effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    ) || flex_item_has_auto_cross_margin(child_style, physical_direction)
    {
        return None;
    }

    if physical_direction.is_row_axis() {
        if !child_style.box_values.height.is_auto() {
            return None;
        }
        let container_cross_size = available.height_basis_points()?;
        Some((container_cross_size - child_style.margin.top - child_style.margin.bottom).max(0.0))
    } else {
        if !child_style.box_values.width.is_auto() {
            return None;
        }
        let container_cross_size = available.width_basis_points()?;
        Some((container_cross_size - child_style.margin.left - child_style.margin.right).max(0.0))
    }
}

fn assign_flex_item_percentage_height_bases(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) {
    for (item, child) in items.iter_mut().zip(children) {
        item.percentage_height_basis = flex_item_final_percentage_height_basis(
            item,
            child,
            container_style,
            physical_direction,
            available,
        );
    }
}

fn flex_item_final_percentage_height_basis(
    item: &FlexItemLayout,
    child: &StyledChild<'_>,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> FlexPercentageBasis {
    // A row flex item's specified physical height is already definite before
    // cross-axis alignment. Preserve it as the descendant percentage basis
    // even when the container's own percentage basis is indefinite.
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
    if physical_direction.is_row_axis()
        && used_content_box_height_or_auto_with_basis(
            &child.style,
            available.height_basis,
            non_content_pt(
                child.style.padding.top
                    + child.style.padding.bottom
                    + vertical_border_width(&child.style),
            ),
        )
        .is_some()
    {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            item.height(),
            FlexDefiniteSizeSource::ResolvedLineCrossSize,
        );
    }

    if physical_direction.is_column_axis() && available.height_basis.is_definite() {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            item.height(),
            FlexDefiniteSizeSource::PostFlexingMainSizeFromDefiniteContainer,
        );
    }

    if physical_direction.is_column_axis()
        && definite_post_flexing_main_size(&child.style, physical_direction, available).is_some()
    {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            item.height(),
            FlexDefiniteSizeSource::PostFlexingMainSizeFromDefiniteFlexBase,
        );
    }

    if physical_direction.is_row_axis()
        && let Some(stretched_height) = stretched_flex_item_cross_size(
            &child.style,
            container_style,
            physical_direction,
            available,
        )
    {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            stretched_height,
            FlexDefiniteSizeSource::StretchedCrossSizeFromDefiniteSingleLineContainer,
        );
    }

    // An auto-sized row container determines its line cross size during flex
    // layout. Once that happens, each item's final cross size is definite for
    // laying out descendants, including descendants with percentage heights:
    // <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
    if physical_direction.is_row_axis() && !available.height_basis.is_definite() {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            item.height(),
            FlexDefiniteSizeSource::ResolvedLineCrossSize,
        );
    }

    PercentageBasis::indefinite()
}

fn definite_flex_basis_main_size(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    let css::ComputedFlexBasis::LengthPercentage(ref length) = style.flex_basis else {
        return None;
    };
    used_length_percentage_or_auto_with_basis(
        css::ComputedLengthPercentageOrAuto::LengthPercentage(length.value.clone()),
        available.main_basis(physical_direction),
    )
    .map(|length| length.points())
}

/// Returns a flex item's main size when Flexbox makes its post-flexing size
/// definite independently of the container's main size.
///
/// A definite `flex-basis` qualifies directly. `flex-basis:auto` instead
/// retrieves the preferred main size, so an explicit definite main-size also
/// qualifies; `flex-basis:content` deliberately does not retrieve that size:
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes> and
/// <https://drafts.csswg.org/css-flexbox/#flex-basis-property>.
fn definite_post_flexing_main_size(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    definite_flex_basis_main_size(style, physical_direction, available).or_else(|| {
        if !matches!(style.flex_basis, css::ComputedFlexBasis::Auto) {
            return None;
        }
        if physical_direction.is_row_axis() {
            used_content_box_width_or_auto_with_basis(
                style,
                available.width_basis,
                non_content_pt(
                    style.padding.left + style.padding.right + horizontal_border_width(style),
                ),
            )
            .map(SemanticLengthExt::points)
        } else {
            used_content_box_height_or_auto_with_basis(
                style,
                available.height_basis,
                non_content_pt(
                    style.padding.top + style.padding.bottom + vertical_border_width(style),
                ),
            )
            .map(SemanticLengthExt::points)
        }
    })
}

/// Resolves the automatic minimum main size of a flex item.
///
/// CSS Flexbox computes automatic minimum sizes from the content-based minimum
/// size for non-scrollable overflow. A preferred aspect ratio can transfer a
/// definite cross size into that minimum; non-replaced items use the larger of
/// the content and transferred suggestions, while replaced items use the smaller:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion>.
pub(in crate::layout::flex) fn automatic_minimum_main_size(
    child: &StyledChild<'_>,
    estimate: &FlexItemEstimate,
    container_style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<f32> {
    let child_style = &child.style;
    // The post-layout guard must use the same definite stretched cross size as
    // Taffy's primary flex calculation. Otherwise an automatic minimum of a
    // replaced item can be recomputed from its specified main size alone and
    // overwrite the smaller content-size suggestion transferred from a
    // definite cross size.
    // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
    let item_available =
        flex_item_estimate_available_space(child_style, container_style, direction, available);
    let stretched_cross_size = if direction.is_row_axis() {
        item_available
            .stretched_height
            .map(PhysicalContentHeight::points)
    } else {
        item_available
            .stretched_width
            .map(PhysicalContentWidth::points)
    };
    let preferred_aspect_ratio = child_style
        .aspect_ratio
        .preferred_ratio(child.is_replaced_element(), estimate.preferred_aspect_ratio);
    let (specified_min, estimated_min, preferred_size, overflow) = if direction.is_row_axis() {
        (
            child_style.box_values.min_width.clone(),
            estimate.min_width,
            used_content_box_width_or_auto_with_basis(
                child_style,
                available.width_basis,
                non_content_pt(
                    child_style.padding.left
                        + child_style.padding.right
                        + horizontal_border_width(child_style),
                ),
            )
            .map(SemanticLengthExt::points),
            flex_item_main_axis_overflow(child_style, direction),
        )
    } else {
        (
            child_style.box_values.min_height.clone(),
            estimate.min_height,
            used_content_box_height_or_auto_with_basis(
                child_style,
                available.height_basis,
                non_content_pt(
                    child_style.padding.top
                        + child_style.padding.bottom
                        + vertical_border_width(child_style),
                ),
            )
            .map(SemanticLengthExt::points),
            flex_item_main_axis_overflow(child_style, direction),
        )
    };
    if !flex_min_size_uses_automatic_minimum(
        specified_min.clone(),
        child_style.writing_mode,
        direction,
    ) || overflow.is_scrollable()
    {
        return None;
    }
    // The intrinsic estimate has incorporated the authored `min-*`
    // constraint.  For `calc-size(auto, …)`, recover the content-size
    // suggestion before substituting `auto`, or the calculation would be
    // applied again by this final-layout safeguard.
    // <https://drafts.csswg.org/css-values-5/#calc-size>.
    let max_content = if direction.is_row_axis() {
        estimate.content_width
    } else {
        estimate.content_height
    };
    let content_size_suggestion = if specified_min.calc_size_with_auto_basis().is_some() {
        estimated_min.min(max_content)
    } else {
        estimated_min
    };
    let mut minimum = content_size_suggestion.points().max(0.0);
    let transferred = if direction.is_row_axis() {
        automatic_minimum_transferred_size_suggestion(
            child_style,
            FlexDirection::Row,
            available.height_basis_points(),
            stretched_cross_size,
            preferred_aspect_ratio,
            estimate.min_height,
            estimate.content_height,
        )
    } else {
        automatic_minimum_transferred_size_suggestion(
            child_style,
            FlexDirection::Column,
            available.width_basis_points(),
            stretched_cross_size,
            preferred_aspect_ratio,
            estimate.min_width,
            estimate.content_width,
        )
    };
    if let Some(transferred) = transferred {
        let transferred = transferred.points().max(0.0);
        minimum = if child.is_replaced_element() {
            minimum.min(transferred)
        } else {
            minimum.max(transferred)
        };
    }
    if let Some(preferred_size) = preferred_size {
        minimum = minimum.min(preferred_size.max(0.0));
    }
    // `calc-size(auto, …)` substitutes the computed content-based automatic
    // minimum after Flexbox has combined its content, transferred, and
    // specified-size suggestions.  Keep this post-layout safeguard in sync
    // with the Taffy adapter above; otherwise Taffy's final size can be
    // corrected only to the untransformed `auto` value.
    // <https://drafts.csswg.org/css-values-5/#calc-size> and
    // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
    let (min_content, max_content, percentage_basis, stretch) = if direction.is_row_axis() {
        (
            content_size_suggestion.points(),
            estimate.content_width.points(),
            available.width_basis,
            available.width.points(),
        )
    } else {
        (
            content_size_suggestion.points(),
            estimate.content_height.points(),
            available.height_basis,
            available
                .height
                .map(PhysicalContentHeight::points)
                .unwrap_or_else(|| estimate.content_height.points()),
        )
    };
    minimum = specified_min
        .calc_size_with_auto_basis()
        .map(|value| {
            value
                .used_value(
                    minimum,
                    min_content,
                    max_content,
                    minimum,
                    stretch,
                    PercentageBasis::definite(layout_pt(percentage_basis.points().unwrap_or(0.0))),
                )
                .points()
        })
        .unwrap_or(minimum)
        .max(0.0);
    // Taffy's item layout is a border-box size, while all content, specified,
    // and transferred size suggestions above are content-box sizes. Convert at
    // this boundary so the post-layout safeguard does not compare unlike box
    // model spaces and accidentally permit a flex item to shrink through its
    // automatic minimum by its padding or borders:
    // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
    // <https://www.w3.org/TR/css-sizing-3/#box-model>.
    let non_content = if direction.is_row_axis() {
        child_style.padding.left + child_style.padding.right + horizontal_border_width(child_style)
    } else {
        child_style.padding.top + child_style.padding.bottom + vertical_border_width(child_style)
    };
    Some(
        content_box_to_border_box_length(content_box_pt(minimum), non_content_pt(non_content))
            .points(),
    )
}

/// Maps CSS `justify-content` to Taffy's flex alignment keywords.
///
/// CSS Box Alignment distinguishes logical `start`/`end`, flex-relative
/// `flex-start`/`flex-end`, and physical `left`/`right` keywords. Taffy's
/// flexbox algorithm supports logical and flex-relative keywords directly
/// when `Style.direction` is set. Physical `left`/`right` keywords affect a
/// horizontal main axis; on a vertical main axis they fall back to the
/// physical block-start side, and otherwise they must be converted through the
/// current flex direction before layout:
/// <https://www.w3.org/TR/css-align-3/#typedef-content-position> and
/// <https://www.w3.org/TR/css-flexbox-1/#flex-direction-property>.
pub(in crate::layout::flex) fn taffy_safety(
    safety: AlignmentSafety,
) -> taffy_layout::AlignmentSafety {
    match safety {
        AlignmentSafety::Default => taffy_layout::AlignmentSafety::Unsafe,
        AlignmentSafety::Unsafe => taffy_layout::AlignmentSafety::Unsafe,
        AlignmentSafety::Safe => taffy_layout::AlignmentSafety::Safe,
    }
}

pub(in crate::layout::flex) fn taffy_content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> taffy_layout::AlignContent {
    let safety = taffy_safety(safety);
    match keyword {
        ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::Stretch,
                safety,
            }
        }
        ContentAlignmentKeyword::Start => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::Start,
            safety,
        },
        ContentAlignmentKeyword::End => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::End,
            safety,
        },
        ContentAlignmentKeyword::FlexStart | ContentAlignmentKeyword::Left => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::FlexStart,
                safety,
            }
        }
        ContentAlignmentKeyword::FlexEnd | ContentAlignmentKeyword::Right => {
            taffy_layout::AlignContent {
                keyword: taffy_layout::AlignContentKeyword::FlexEnd,
                safety,
            }
        }
        ContentAlignmentKeyword::Center => taffy_layout::AlignContent {
            keyword: taffy_layout::AlignContentKeyword::Center,
            safety,
        },
        ContentAlignmentKeyword::SpaceBetween => taffy_layout::AlignContent::SPACE_BETWEEN,
        ContentAlignmentKeyword::SpaceAround => taffy_layout::AlignContent::SPACE_AROUND,
        ContentAlignmentKeyword::SpaceEvenly => taffy_layout::AlignContent::SPACE_EVENLY,
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            taffy_layout::AlignContent::FLEX_START
        }
    }
}

/// Maps CSS `align-content` to Taffy's flex line-packing value.
///
/// CSS Align allows `normal`, baseline positions, overflow-safe positions, and
/// distribution keywords. In flex layout, `normal` behaves as `stretch`; Taffy
/// does not model content baseline packing, so baseline values currently use
/// the spec fallback start-side packing at this boundary:
/// <https://www.w3.org/TR/css-align-3/#align-content-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
pub(in crate::layout::flex) fn taffy_align_content(
    align_content: AlignContent,
) -> taffy_layout::AlignContent {
    taffy_content_alignment(align_content.keyword, align_content.safety)
}

/// Maps CSS `align-items` to Taffy's flex cross-axis item alignment.
///
/// CSS Align defines `normal` as layout-mode dependent; for flex items it
/// behaves as `stretch`. `align-items:self-start`/`self-end` is represented
/// for each affected item through an explicit `align-self` override, because
/// those values depend on the alignment subject's own writing mode:
/// <https://www.w3.org/TR/css-align-3/#align-items-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-items-property>.
pub(in crate::layout::flex) fn taffy_align_items(
    align_items: AlignItems,
) -> taffy_layout::AlignItems {
    taffy_self_alignment(align_items, false)
}

/// Maps CSS `align-self` to Taffy's flex item alignment override.
///
/// `auto` computes to itself and defers to the parent `align-items`; all other
/// values share the `align-items` mapping:
/// <https://www.w3.org/TR/css-align-3/#align-self-property>.
pub(in crate::layout::flex) fn taffy_effective_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<taffy_layout::AlignSelf> {
    let alignment = effective_align_self(child_style, container_style);
    // A percentage cross size in an indefinite axis behaves as `auto` for
    // sizing, but it is not a *computed* `auto` size. CSS Flexbox therefore
    // does not stretch it; the normal/stretch alignment falls back to the
    // cross-start position instead.
    // <https://drafts.csswg.org/css-flexbox-1/#valdef-align-items-stretch>.
    let cyclic_percentage_cross_size = if physical_direction.is_row_axis() {
        matches!(
            &child_style.box_values.height,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.contains_percentage()
        ) && !available.height_basis.is_definite()
    } else {
        matches!(
            &child_style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(value) if value.contains_percentage()
        ) && !available.width_basis.is_definite()
    };
    if cyclic_percentage_cross_size
        && matches!(
            alignment.keyword,
            SelfAlignmentKeyword::Auto
                | SelfAlignmentKeyword::Normal
                | SelfAlignmentKeyword::Stretch
        )
    {
        return Some(taffy_layout::AlignSelf {
            keyword: taffy_layout::AlignItemsKeyword::FlexStart,
            safety: taffy_safety(alignment.safety),
        });
    }
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto
        && !matches!(
            container_style.align_items.keyword,
            SelfAlignmentKeyword::SelfStart | SelfAlignmentKeyword::SelfEnd
        )
    {
        return None;
    }
    Some(taffy_cross_self_alignment(alignment))
}

pub(in crate::layout::flex) fn effective_align_self(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> AlignSelf {
    if child_style.align_self.keyword == SelfAlignmentKeyword::Auto {
        container_style.align_items
    } else {
        child_style.align_self
    }
}

pub(in crate::layout::flex) fn taffy_self_alignment(
    alignment: AlignItems,
    for_align_self: bool,
) -> taffy_layout::AlignItems {
    let safety = taffy_safety(alignment.safety);
    match alignment.keyword {
        SelfAlignmentKeyword::Auto if for_align_self => taffy_layout::AlignItems::STRETCH,
        SelfAlignmentKeyword::Auto
        | SelfAlignmentKeyword::Normal
        | SelfAlignmentKeyword::Stretch => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Stretch,
            safety,
        },
        SelfAlignmentKeyword::Start | SelfAlignmentKeyword::SelfStart => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Start,
            safety,
        },
        SelfAlignmentKeyword::End | SelfAlignmentKeyword::SelfEnd => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::End,
            safety,
        },
        SelfAlignmentKeyword::FlexStart | SelfAlignmentKeyword::Left => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::FlexStart,
            safety,
        },
        SelfAlignmentKeyword::FlexEnd | SelfAlignmentKeyword::Right => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::FlexEnd,
            safety,
        },
        SelfAlignmentKeyword::Center => taffy_layout::AlignItems {
            keyword: taffy_layout::AlignItemsKeyword::Center,
            safety,
        },
        SelfAlignmentKeyword::Baseline | SelfAlignmentKeyword::LastBaseline => {
            taffy_layout::AlignItems::BASELINE
        }
    }
}

/// Maps CSS self-alignment to Taffy's flex item alignment override.
///
/// CSS Box Alignment defines `self-start` and `self-end` from the alignment
/// subject's writing mode, which Taffy's flex alignment model does not carry.
/// Those values are given a start-side placeholder for sizing and line
/// construction; Quire corrects their final cross-axis offsets after
/// Taffy returns item geometry:
/// <https://www.w3.org/TR/css-align-3/#self-position> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-items-property>.
pub(in crate::layout::flex) fn taffy_cross_self_alignment(
    alignment: AlignSelf,
) -> taffy_layout::AlignSelf {
    match alignment.keyword {
        SelfAlignmentKeyword::SelfStart | SelfAlignmentKeyword::SelfEnd => {
            taffy_layout::AlignSelf {
                keyword: taffy_layout::AlignItemsKeyword::FlexStart,
                safety: taffy_safety(alignment.safety),
            }
        }
        _ => taffy_self_alignment(alignment, true),
    }
}

pub(in crate::layout::flex) fn flex_cross_start_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_start_side(style.writing_mode)
    } else {
        inline_start_side(style.writing_mode, style.direction)
    }
}

pub(in crate::layout::flex) fn flex_cross_end_side(style: &ComputedStyle) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_end_side(style.writing_mode)
    } else {
        inline_end_side(style.writing_mode, style.direction)
    }
}

pub(in crate::layout::flex) fn child_self_start_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_start = flex_cross_start_side(container_style);
    let cross_axis = cross_start.axis();
    let block_start = block_start_side(child_style.writing_mode);
    if block_start.axis() == cross_axis {
        block_start
    } else {
        inline_start_side(child_style.writing_mode, child_style.direction)
    }
}

pub(in crate::layout::flex) fn child_self_end_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_start = flex_cross_start_side(container_style);
    let cross_axis = cross_start.axis();
    let block_end = block_end_side(child_style.writing_mode);
    if block_end.axis() == cross_axis {
        block_end
    } else {
        inline_end_side(child_style.writing_mode, child_style.direction)
    }
}
