use super::super::estimate::FlexIntrinsicItem;
use super::*;
use crate::layout::flex::layout::placed_flex_item_style;
use crate::layout::taffy_bridge;
use crate::units::IntoLayoutLength;

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
        stylesheets: &Stylesheets<'_>,
        available: FlexAvailableSpace,
    ) -> Option<FlexLayout> {
        if style.flex_wrap.wraps()
            && (style.margin_trim.block_start
                || style.margin_trim.block_end
                || style.margin_trim.inline_start
                || style.margin_trim.inline_end)
        {
            let placement_probe = self.compute_flex_layout_without_margin_trim(
                children,
                style,
                stylesheets,
                available,
            )?;
            let plan = flex_margin_trim_plan(style, &placement_probe.lines, children.len());
            if !plan.is_empty() {
                let mut trimmed_children = children.to_vec();
                for (index, child) in trimmed_children.iter_mut().enumerate() {
                    plan.apply_to_style(index, &mut child.style);
                }
                return self.compute_flex_layout_without_margin_trim(
                    &trimmed_children,
                    style,
                    stylesheets,
                    available,
                );
            }
        }
        self.compute_flex_layout_without_margin_trim(children, style, stylesheets, available)
    }

    fn compute_flex_layout_without_margin_trim(
        &mut self,
        children: &[StyledChild<'_>],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
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
        stylesheets: &Stylesheets<'_>,
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
        let mut estimates = vec![
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(0.0)),
                PhysicalContentHeight::new(content_box_pt(0.0)),
            );
            children.len()
        ];
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
        let mut active_hypothetical_outer_main_sizes = Vec::with_capacity(children.len());
        let mut source_indices = Vec::with_capacity(children.len());
        let mut estimated_collapsed_struts = Vec::new();
        for (source_index, child) in children.iter().enumerate() {
            let child_style = &child.style;
            let child_containment = child
                .element_parts()
                .map(|(element, _, _)| used_property_containment(element, child_style))
                .unwrap_or_default();
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
                    content_child
                        .style
                        .box_values
                        .height
                        .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
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
                    estimated_size.set_fragmentable_overflow_height(
                        content_estimate.fragmentable_overflow_height,
                    );
                }
            }
            // Inline-size containment suppresses only the item's logical
            // inline contribution.  In a horizontal flex item this is its
            // physical width; keep the independently measured physical
            // height so cross-axis stretching and auto block sizing still see
            // descendant layout.  `contain:size` has already replaced both
            // axes in the estimate above.
            // <https://drafts.csswg.org/css-contain-3/#inline-size-containment>
            if child_containment.inline_size
                && !child_containment.size
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
                    cross_size: flex_cross_size_from_layout_extent(estimated_outer_cross_size(
                        child_style,
                        estimated_size,
                        physical_direction,
                    )),
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
                && matches!(style.flex_line_count, css::FlexLineCount::Count(_))
                && stretched_cross_size.is_some();
            let cross_size_is_auto = if physical_direction.is_row_axis() {
                child_style.box_values.height.is_auto()
            } else {
                child_style.box_values.width.is_auto()
            };
            // A table is an independent formatting context whose caption and
            // grid are measured after Flexbox assigns the final cross slot.
            // Give Taffy that definite wrapper constraint while constructing
            // the line, rather than letting its generic leaf measurement
            // retain the pre-stretch caption-only height. Table replay then
            // consumes the same wrapper border-box span through
            // `PlacedFormattingContext`.
            // <https://drafts.csswg.org/css-flexbox-1/#algo-stretch>
            // <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>
            let table_wrapper_cross_stretch = child_style.display.is_table()
                && cross_size_is_auto
                && stretched_cross_size.is_some();
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
            let flex_min_width = if child_style.display.is_table() {
                css::ComputedLengthPercentageOrAuto::MinContent
            } else {
                child_style.box_values.min_width.clone()
            };
            // The corresponding table-wrapper block minimum is the grid and
            // caption min-content contribution. A specified `height` or
            // `min-height: 0` is a preferred-size suggestion for Flexbox; it
            // must not erase the table's used minimum and let later flex
            // items overlap the replayed grid.
            // <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>
            // <https://drafts.csswg.org/css-flexbox-1/#min-size-auto>
            let flex_min_height = child_style.box_values.min_height.clone();
            // Taffy's leaf measurement callback must use the same resolved
            // flex basis supplied to its flex algorithm. In particular,
            // `flex-basis:auto` can acquire a definite main size by
            // transferring a definite cross size through aspect-ratio.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
            let flex_basis_context = FlexBasisContext {
                direction: physical_direction,
                available_main_size: if physical_direction.is_row_axis() {
                    FlexMainSize::new(available.width.points())
                } else {
                    FlexMainSize::new(
                        available
                            .height
                            .map(PhysicalContentHeight::points)
                            .unwrap_or_else(|| estimated_size.content_height.points()),
                    )
                },
                available_cross_size: if physical_direction.is_row_axis() {
                    available
                        .height_basis_content_box_length()
                        .map(flex_cross_size_from_content_box)
                } else {
                    available
                        .width_basis_content_box_length()
                        .map(flex_cross_size_from_content_box)
                },
                stretched_cross_size,
                main_size_basis: if physical_direction.is_row_axis() {
                    available.width_basis
                } else {
                    available.height_basis
                },
                preferred_aspect_ratio,
            };
            let resolved_flex_basis =
                resolve_taffy_flex_basis(child_style, &estimated_size, flex_basis_context);
            estimated_size.set_main_size_provenance(resolved_flex_basis.provenance);
            let taffy_flex_basis = resolved_flex_basis.dimension;
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
                        direction: taffy_direction(child_style.used_direction()),
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
                                    auto_cross_uses_stretch_fit: uses_balanced_line_cross_stretch
                                        || table_wrapper_cross_stretch,
                                    auto_cross_fit_content: wrapping_column_cross_fit_content,
                                },
                            ),
                            height: flex_item_size_dimension(
                                child_style.box_values.height.value().clone(),
                                estimated_size.height,
                                estimated_size.min_height,
                                estimated_size.content_height,
                                FlexItemSizeDimensionContext {
                                    flex_direction: physical_direction,
                                    dimension_axis: FlexDirection::Column,
                                    percentage_basis: available.height_basis,
                                    stretch: vertical_stretch,
                                    flex_basis_overrides_main_size,
                                    auto_cross_uses_stretch_fit: uses_balanced_line_cross_stretch
                                        || table_wrapper_cross_stretch,
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
                                    definite_preferred_content_size: (!child_style
                                        .display
                                        .is_table()
                                        || !physical_direction.is_row_axis())
                                    .then(|| {
                                        used_content_box_width_or_auto_with_basis(
                                            child_style,
                                            available.width_basis,
                                            non_content_pt(horizontal_non_content),
                                        )
                                    })
                                    .flatten(),
                                    transferred_size_suggestion:
                                        automatic_minimum_transferred_size_suggestion(
                                            child_style,
                                            FlexDirection::Row,
                                            available
                                                .height_basis_content_box_length()
                                                .map(flex_cross_size_from_content_box),
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
                                flex_min_height,
                                estimated_size.min_height,
                                estimated_size.content_height,
                                FlexMinSizeDimensionContext {
                                    definite_preferred_content_size: (!child_style
                                        .display
                                        .is_table()
                                        || !physical_direction.is_column_axis())
                                    .then(|| {
                                        used_content_box_height_or_auto_with_basis(
                                            child_style,
                                            available.height_basis,
                                            non_content_pt(vertical_non_content),
                                        )
                                    })
                                    .flatten(),
                                    transferred_size_suggestion:
                                        automatic_minimum_transferred_size_suggestion(
                                            child_style,
                                            FlexDirection::Column,
                                            available
                                                .width_basis_content_box_length()
                                                .map(flex_cross_size_from_content_box),
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
                            child_style.flex_grow.value()
                        },
                        flex_shrink: child_style.flex_shrink.value(),
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
            let intrinsic_item =
                FlexIntrinsicItem::new(child, estimated_size, physical_direction, available, style);
            // A definite preferred main size caps an automatic minimum while
            // collecting flex lines. Do not substitute an intrinsic minimum
            // when the preferred main size comes solely from `flex-basis`:
            // that value is resolved by the flex algorithm after line
            // collection, whereas an authored width/height is already the
            // specified-size suggestion.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
            // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
            let definite_preferred_main_size = if physical_direction.is_row_axis() {
                used_content_box_width_or_auto_with_basis(
                    &child.style,
                    available.width_basis,
                    non_content_pt(
                        child.style.padding.left
                            + child.style.padding.right
                            + horizontal_border_width(&child.style),
                    ),
                )
                .is_some()
            } else {
                used_content_box_height_or_auto_with_basis(
                    &child.style,
                    available.height_basis,
                    non_content_pt(
                        child.style.padding.top
                            + child.style.padding.bottom
                            + vertical_border_width(&child.style),
                    ),
                )
                .is_some()
            };
            let hypothetical_main_size = definite_preferred_main_size
                .then(|| {
                    automatic_minimum_main_size(
                        child,
                        &estimated_size,
                        style,
                        physical_direction,
                        available,
                    )
                    .map(|minimum| intrinsic_item.flex_base_size.max(minimum))
                })
                .flatten()
                .unwrap_or(intrinsic_item.hypothetical_main_size);
            active_hypothetical_outer_main_sizes.push(hypothetical_main_size);
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
                        // A numeric used height constrains Flexbox's own
                        // cross-axis line packing even when it is not a
                        // definite percentage basis for descendants. Keeping
                        // those concerns separate lets an authored page-height
                        // flex container distribute `align-content` free
                        // space without making cyclic percentages definite.
                        // <https://www.w3.org/TR/css-flexbox-1/#align-content-property>
                        // <https://www.w3.org/TR/css-sizing-3/#definite>
                        height: available
                            .height_constraint()
                            .map(PhysicalContentHeight::points)
                            .map(taffy_layout::Dimension::length)
                            .unwrap_or_else(taffy_layout::Dimension::auto),
                    },
                    min_size: taffy_layout::Size {
                        width: taffy_min_dimension(
                            style.box_values.min_width.clone(),
                            available.width_basis,
                        ),
                        height: taffy_min_dimension(
                            style.box_values.min_height.clone(),
                            available.height_basis,
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

        // Freeze the CSS Align decision before the Taffy bridge begins line
        // construction. Taffy may use compatible placeholders for sizing,
        // but it must not become the source of the final CSS placement mode.
        let active_children = source_indices
            .iter()
            .map(|&index| children[index].clone())
            .collect::<Vec<_>>();
        let cross_alignments = resolve_flex_cross_alignments(
            &active_estimates,
            &active_children,
            style,
            physical_direction,
        );

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
        let mut active_items = nodes
            .iter()
            .map(|&node| {
                let layout = tree.layout(node).ok()?;
                Some(FlexItemLayout::from_taffy_rect(
                    taffy_rect_from_layout(layout),
                    flex_axes,
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        self.measure_final_normal_flow_line_box_spans(
            &active_items,
            &mut active_estimates,
            &active_children,
            style,
            stylesheets,
            available,
        );
        apply_final_normal_flow_item_block_spans(
            &mut active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
        );
        let container_cross_size = FlexCrossSize::new(if physical_direction.is_row_axis() {
            root_rect.size.height
        } else {
            root_rect.size.width
        });
        let line_cross_constraint = FlexLineCrossConstraint::from_container(
            style,
            available,
            physical_direction,
            container_cross_size,
        );
        let line_cross_axis_layout = FlexLineCrossAxisLayout {
            constraint: line_cross_constraint,
            gap: flex_line_cross_gap(
                style,
                physical_direction,
                available,
                physical_gap_width.clone(),
                physical_gap_height.clone(),
            ),
        };
        let container_main_size = if physical_direction.is_row_axis() {
            available
                .width_basis_content_box_length()
                .map(flex_main_size_from_content_box)
        } else {
            available
                .height_basis_content_box_length()
                .map(flex_main_size_from_content_box)
                // A definite maximum main size constrains flex line
                // collection even though it does not make percentage heights
                // definite.  The used root size is already clamped by that
                // maximum at this point, so use it only for wrapping rather
                // than changing the percentage-resolution basis.
                // <https://www.w3.org/TR/css-flexbox-1/#algo-line-break>
                .or_else(|| {
                    (!style.box_values.max_height.is_auto())
                        .then_some(FlexMainSize::new(root_rect.size.height))
                })
        };
        let main_gap = flex_main_gap_size(used_flex_gap_with_basis(
            if physical_direction.is_row_axis() {
                physical_gap_width.clone()
            } else {
                physical_gap_height.clone()
            },
            available.main_basis(physical_direction),
        ));
        self.remeasure_auto_height_row_item_cross_contributions(
            &active_items,
            &mut active_estimates,
            &active_children,
            style,
            stylesheets,
            physical_direction,
            available,
        );
        let mut active_lines = flex_lines_from_items(
            &mut active_items,
            &active_children,
            &active_estimates,
            FlexLineCollectionContext {
                container_style: style,
                physical_direction,
                cross_axis_layout: line_cross_axis_layout,
                hypothetical_outer_main_sizes: &active_hypothetical_outer_main_sizes,
                container_main_size,
                main_gap,
            },
        );
        // Normal-flow measurement may replace the provisional block extent of
        // an automatic column item.  Re-run main-axis packing from those
        // immutable final spans before any cross-axis line geometry is
        // calculated; otherwise later items retain Taffy's stale origins.
        if physical_direction.is_column_axis() {
            repack_lines_after_main_size_adjustment(
                &mut active_lines,
                &mut active_items,
                &active_children,
                style,
                physical_direction,
                FlexMainSize::new(root_rect.size.height),
                available.main_basis(physical_direction),
            );
        }
        let balanced_taffy_context = BalancedTaffyLayoutContext {
            template_tree: &tree,
            template_root: root,
            template_nodes: &nodes,
            estimates: &active_estimates,
            flex_axes,
            // Each isolated no-wrap Taffy tree represents a final balanced
            // line, so it must receive the same reserved cross-axis slot
            // used while estimating its members.  The record preserves the
            // container percentage bases independently.
            available: item_measure_available,
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
        if style.flex_wrap.balances_lines() {
            let _topology_changed = rebalance_flex_line_membership(
                &mut active_lines,
                &mut active_items,
                &active_children,
                FlexBalanceContext {
                    physical_direction,
                    requested_line_count: match style.flex_line_count {
                        css::FlexLineCount::Auto => None,
                        css::FlexLineCount::Count(count) => Some(count.get()),
                    },
                    hypothetical_main_sizes: balanced_hypothetical_main_sizes.as_deref(),
                    main_gap: FlexMainSize::new(
                        used_flex_gap(
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
                    ),
                    cross_gap: FlexCrossSize::new(
                        used_flex_gap(
                            if physical_direction.is_row_axis() {
                                physical_gap_height
                            } else {
                                physical_gap_width
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
                    ),
                    reserved_line_cross_size: line_cross_constraint.reserved_balanced_line_slot(),
                    available_main_size: FlexMainSize::new(if physical_direction.is_row_axis() {
                        root_rect.size.width
                    } else {
                        root_rect.size.height
                    }),
                },
            );
            // A balance plan is final topology, not a conditional correction:
            // every selected line needs its own flexible-length resolution,
            // even when its membership happens to match normal wrapping.
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
                FlexMainSize::new(if physical_direction.is_row_axis() {
                    root_rect.size.width
                } else {
                    root_rect.size.height
                }),
                available.main_basis(physical_direction),
            );
            refresh_flex_line_metadata(
                &mut active_lines,
                &mut active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                line_cross_axis_layout,
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
        // A stretch measurement can refine a baseline participant's intrinsic
        // cross contribution.  Refresh the canonical slots, then apply that
        // final slot to stretch items once more.  The second pass is not a
        // rectangle-derived line reconstruction: membership and the line
        // sizing inputs remain the immutable topology and hypothetical
        // metrics collected above.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
        for _ in 0..2 {
            if !apply_line_cross_size_dependent_item_remeasurements(
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
                    line_cross_gap: line_cross_axis_layout.gap,
                    cross_constraint: line_cross_constraint,
                },
            ) {
                break;
            }
            refresh_flex_line_metadata(
                &mut active_lines,
                &mut active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                line_cross_axis_layout,
            );
        }
        if apply_post_flexing_main_size_cross_remeasurements(
            self,
            &mut active_items,
            &mut active_estimates,
            &active_children,
            PostFlexingMainSizeCrossRemeasureContext {
                container_style: style,
                stylesheets,
                physical_direction,
                available,
            },
        ) {
            refresh_flex_line_metadata(
                &mut active_lines,
                &mut active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                line_cross_axis_layout,
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
                &mut active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                line_cross_axis_layout,
            );
        }
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
                FlexMainSize::new(if physical_direction.is_row_axis() {
                    root_rect.size.width
                } else {
                    root_rect.size.height
                }),
                available.main_basis(physical_direction),
            );
            refresh_flex_line_cross_bounds(
                &mut active_lines,
                &active_items,
                &active_children,
                style,
                physical_direction,
                line_cross_constraint,
            );
            refresh_flex_line_metadata(
                &mut active_lines,
                &mut active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                line_cross_axis_layout,
            );
        }
        // Resolve the stretch overflow fallback before later cross-axis
        // metadata reconciliation. Those passes intentionally rebuild line
        // bounds, but must retain the overflow group origin selected here.
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
                &mut active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
                line_cross_axis_layout,
            );
        }
        // Every cross-axis placement now consumes the same post-sizing line
        // slots. Later content-packing stages only translate whole lines.
        finalize_flex_cross_axis_placement(
            &mut active_items,
            &active_estimates,
            &active_children,
            &mut active_lines,
            style,
            physical_direction,
            &cross_alignments,
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
        refresh_flex_line_baselines(
            &mut active_lines,
            &active_items,
            &active_estimates,
            &active_children,
            style,
            physical_direction,
        );
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
        if repack_final_flex_line_offsets(
            &mut active_items,
            &mut active_lines,
            style,
            physical_direction,
            container_cross_size,
            line_cross_axis_layout.gap,
        ) {
            // Translating complete lines also translates their absolute
            // baselines. Recompute only the exported baseline metadata: a
            // full line-metadata refresh would compact the restored
            // distributed slots again.
            refresh_flex_line_baselines(
                &mut active_lines,
                &active_items,
                &active_estimates,
                &active_children,
                style,
                physical_direction,
            );
        }
        assign_flex_item_percentage_height_bases(
            &mut active_items,
            &active_children,
            style,
            physical_direction,
            available,
        );
        self.remeasure_nested_flex_fragmentable_overflow_extents(
            &active_items,
            &mut active_estimates,
            &active_children,
            style,
            stylesheets,
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
                    logical_cross_start_rank: line.logical_cross_start_rank,
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
        // This is a physical vertical paint/layout extent, irrespective of
        // which axis is the flex main axis.  Taffy's item location includes
        // the resolved block-start margin; add the independently resolved
        // block-end margin to obtain the item's outer vertical edge.
        //
        // Flex-item margins do not collapse with the flex container or one
        // another, so an automatic flex container must retain this edge when
        // it becomes its used physical height.
        // <https://www.w3.org/TR/css-flexbox-1/#flex-items>
        let finalized_outer_vertical_end = |item: &FlexItemLayout, child: &StyledChild<'_>| {
            let margin = flex_item_used_margin(&child.style, style, available);
            item.y().points() + item.height().points() + margin.bottom
        };
        let item_extent_height = items
            .iter()
            .zip(children)
            .filter(|(_, child)| !flex_item_is_collapsed(&child.style))
            .map(|(item, child)| finalized_outer_vertical_end(item, child))
            .fold(0.0f32, f32::max);
        let collapsed_cross_height = if physical_direction.is_row_axis() {
            source_lines
                .iter()
                .map(|line| line.largest_collapsed_strut().points())
                .fold(0.0f32, f32::max)
        } else {
            0.0
        };
        // Post-Taffy flex reconciliation can change a row item's final cross
        // size (for example through automatic minimum-size and aspect-ratio
        // handling). An auto-height flex root must derive its used cross size
        // from those finalized margin boxes, rather than retain Taffy's
        // provisional root height from before the correction.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-container>
        let finalized_row_cross_height = items
            .iter()
            .zip(children)
            .filter(|(_, child)| !flex_item_is_collapsed(&child.style))
            .map(|(item, child)| finalized_outer_vertical_end(item, child))
            .fold(0.0f32, f32::max);
        let height = if !style.box_values.height.is_auto() || available.height_basis.is_definite() {
            root_rect.size.height
        } else if physical_direction.is_row_axis() {
            // A fragmentainer may provide a physical available-height limit
            // without making this automatic row container's cross size
            // definite. Its used height is still its finalized line margin
            // box, including an item's trailing cross-axis margin; using the
            // provisional Taffy root height here loses that margin during
            // nested percentage replay.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-container>
            finalized_row_cross_height.max(collapsed_cross_height)
        } else if available.height.is_some() {
            item_extent_height.max(collapsed_cross_height)
        } else if physical_direction.is_column_axis() {
            // An automatic column main size is the extent of the finalized
            // flex items. Taffy's intrinsic root probe can retain a
            // provisional max-content height (notably when a percentage
            // flex-basis falls back to `content`), but that value is not a
            // used CSS main size and must not create trailing free space.
            // This also covers balanced columns after their line membership
            // has been reconciled.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-main-container>
            // <https://www.w3.org/TR/css-flexbox-2/#algo-balance>
            item_extent_height.max(collapsed_cross_height)
        } else {
            root_rect
                .size
                .height
                .max(item_extent_height)
                .max(collapsed_cross_height)
        };

        let baselines = flex_container_baselines(
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
            height: PhysicalContentHeight::new(content_box_pt(height)),
            main_gap,
            baselines,
            items,
            lines: source_lines,
            fragment_plan,
        })
    }
}

/// Select the margins trimmed by a wrapped flex container after line
/// membership has been established.  Line membership is order-modified and
/// excludes collapsed items, as required by CSS Flexbox and CSS Box.
/// <https://drafts.csswg.org/css-box-4/#margin-trim-flex>.
fn flex_margin_trim_plan(
    style: &ComputedStyle,
    lines: &[FlexLineLayout],
    item_count: usize,
) -> MarginTrimPlan {
    let mut plan = MarginTrimPlan::for_item_count(item_count);
    let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
    let (main_start, main_end) = match style.flex_direction {
        FlexDirection::Row => (LogicalSide::InlineStart, LogicalSide::InlineEnd),
        FlexDirection::RowReverse => (LogicalSide::InlineEnd, LogicalSide::InlineStart),
        FlexDirection::Column => (LogicalSide::BlockStart, LogicalSide::BlockEnd),
        FlexDirection::ColumnReverse => (LogicalSide::BlockEnd, LogicalSide::BlockStart),
    };
    let main_start = axes.physical_side(main_start);
    let main_end = axes.physical_side(main_end);

    for (enabled, side) in [
        (style.margin_trim.block_start, LogicalSide::BlockStart),
        (style.margin_trim.block_end, LogicalSide::BlockEnd),
        (style.margin_trim.inline_start, LogicalSide::InlineStart),
        (style.margin_trim.inline_end, LogicalSide::InlineEnd),
    ] {
        if !enabled {
            continue;
        }
        let physical_side = axes.physical_side(side);
        if physical_side.axis() == main_start.axis() {
            for line in lines {
                let item = if physical_side == main_start {
                    line.item_indices.first()
                } else if physical_side == main_end {
                    line.item_indices.last()
                } else {
                    None
                };
                if let Some(&item) = item {
                    plan.trim(item, physical_side);
                }
            }
            continue;
        }

        let edge_line = if physical_side.is_start_edge() {
            lines.iter().min_by(|left, right| {
                left.cross_start
                    .partial_cmp(&right.cross_start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        } else {
            lines.iter().max_by(|left, right| {
                left.cross_end
                    .partial_cmp(&right.cross_end)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        };
        if let Some(line) = edge_line {
            for &item in &line.item_indices {
                plan.trim(item, physical_side);
            }
        }
    }
    plan
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
    // The template root was built for the full container.  A balanced plan
    // resolves one no-wrap Taffy tree per final line, so its cross axis must
    // instead use the same reserved line slot that was used to estimate the
    // items.  Child percentage dimensions have already been resolved against
    // their original container bases while constructing the template styles;
    // this changes only the line's layout constraint.
    // <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
    root_style.size.width = taffy_layout::Dimension::length(context.available.width.points());
    root_style.size.height = context
        .available
        .height_constraint()
        .map(PhysicalContentHeight::points)
        .map(taffy_layout::Dimension::length)
        .unwrap_or_else(taffy_layout::Dimension::auto);
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
            // This no-wrap tree is the final balanced line, not a probe.
            // Preserve its resolved cross size as well as its flexible main
            // size so downstream line measurement and CSS Align placement do
            // not retain a full-container normal-wrap box.
            item.set_cross_size(context.flex_axes, layout.cross_size(context.flex_axes));
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
        // Size containment suppresses descendant-derived intrinsic source
        // extent, but not the flex item's independently resolved used box.
        // A definite `height` therefore remains a monolithic source span for
        // the outer flex fragment plan; only its children are denied internal
        // break opportunities.
        // <https://www.w3.org/TR/css-contain-2/#size-containment>
        // <https://www.w3.org/TR/css-break-3/#monolithic>
        if child
            .element_parts()
            .is_some_and(|(element, _, _)| used_property_containment(element, &child.style).size)
        {
            item.set_fragmentation_height(PhysicalContentHeight::new(content_box_pt(
                item.height().points(),
            )));
            continue;
        }
        // Scrollable overflow remains inside the flex item's scrollport. It
        // may contribute visual/clipped descendant paint, but it must not
        // manufacture additional page-fragment slices beyond the used flex
        // item border box; doing so turns `overflow: hidden` into an
        // overflowing, page-long item after the flex algorithm correctly
        // resolved its automatic minimum to zero.
        // <https://www.w3.org/TR/css-overflow-3/#scrollable>
        // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
        if child.style.overflow_y.is_scrollable() {
            continue;
        }
        let content_overflow = estimate.fragmentable_overflow_height.points().max(0.0);
        // The intrinsic content extent is already measured from the item's
        // border-box block start. Do not append its padding or border here:
        // the used box owns those decorations, and appending its block-end
        // edge would manufacture a later source continuation after descendant
        // overflow has been consumed.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        item.set_fragmentation_height(PhysicalContentHeight::new(content_box_pt(content_overflow)));
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Re-measure an auto-height horizontal row item after flexible lengths
    /// have fixed its main size.
    ///
    /// The outer line must use this normal in-flow cross contribution when it
    /// establishes its hypothetical cross size. In particular, a stretched
    /// item does not receive its final used cross size until *after* that
    /// line-sizing step, so treating the remeasurement as fragmentation-only
    /// leaves later nested lines at the first line's block position.
    ///
    /// This is deliberately limited to horizontal rows. Other writing-mode
    /// and axis combinations have separate percentage-basis and line
    /// constraints that cannot safely reuse this physical-height
    /// remeasurement.
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
    #[allow(clippy::too_many_arguments)]
    fn remeasure_auto_height_row_item_cross_contributions(
        &mut self,
        items: &[FlexItemLayout],
        estimates: &mut [FlexItemEstimate],
        children: &[StyledChild<'_>],
        container_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        physical_direction: FlexDirection,
        available: FlexAvailableSpace,
    ) {
        if !physical_direction.is_row_axis()
            || container_style.writing_mode != WritingMode::HorizontalTb
        {
            return;
        }

        for ((item, estimate), child) in items.iter().zip(estimates).zip(children) {
            if !child.style.box_values.height.is_auto() {
                continue;
            }

            let borders = used_border_widths(&child.style);
            let horizontal_non_content =
                child.style.padding.left + child.style.padding.right + borders.left + borders.right;
            let used_content_width = (item.width().points() - horizontal_non_content).max(0.0);
            let mut item_available = flex_item_estimate_available_space(
                &child.style,
                container_style,
                physical_direction,
                available,
            );
            item_available.set_definite_width(
                PhysicalContentWidth::new(content_box_pt(used_content_width)),
                FlexAvailableSizeSource::PostFlexingMainSize,
            );
            let remeasured = self.estimate_flex_item_size(
                child,
                stylesheets,
                item_available,
                physical_direction,
            );
            estimate.replace_row_normal_flow_cross_contribution_preserving_fragmentable_overflow(
                remeasured,
            );
        }
    }

    /// Re-measure nested row-flex items at their resolved main size for their
    /// fragmentable descendant extent.
    ///
    /// Flex base sizing may measure an auto-width nested flex container with
    /// an indefinite main-size basis. Once the outer algorithm has assigned a
    /// narrower used main size, its wrapping descendants can occupy more
    /// physical block space. The used outer cross size remains unchanged here;
    /// only the source extent used by CSS Fragmentation is refreshed.
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>
    #[allow(clippy::too_many_arguments)]
    fn remeasure_nested_flex_fragmentable_overflow_extents(
        &mut self,
        items: &[FlexItemLayout],
        estimates: &mut [FlexItemEstimate],
        children: &[StyledChild<'_>],
        container_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        physical_direction: FlexDirection,
        available: FlexAvailableSpace,
    ) {
        if !physical_direction.is_row_axis() {
            return;
        }
        for ((item, estimate), child) in items.iter().zip(estimates).zip(children) {
            if !child.style.display.is_flex() || !child.style.box_values.height.is_auto() {
                continue;
            }
            let borders = used_border_widths(&child.style);
            let horizontal_non_content =
                child.style.padding.left + child.style.padding.right + borders.left + borders.right;
            let used_content_width = (item.width().points() - horizontal_non_content).max(0.0);
            if (used_content_width - estimate.width.points()).abs() <= 0.01 {
                continue;
            }
            let mut item_available = flex_item_estimate_available_space(
                &child.style,
                container_style,
                physical_direction,
                available,
            );
            item_available.set_definite_width(
                PhysicalContentWidth::new(content_box_pt(used_content_width)),
                FlexAvailableSizeSource::PostFlexingMainSize,
            );
            let remeasured = self.estimate_flex_item_size(
                child,
                stylesheets,
                item_available,
                physical_direction,
            );
            estimate.merge_fragmentable_overflow_height(remeasured.fragmentable_overflow_height);
        }
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
        let current = item.main_size(axes);
        if current >= minimum {
            continue;
        }
        if matches!(
            physical_direction,
            FlexDirection::RowReverse | FlexDirection::ColumnReverse
        ) {
            item.set_main_start(axes, item.main_start(axes) - (minimum - current));
        }
        item.set_main_size(axes, minimum);
        changed = true;
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
        if item.width() < FlexPhysicalHorizontalSize::new(min_width) {
            item.set_width(FlexPhysicalHorizontalSize::new(min_width));
            changed = true;
        }

        let min_height =
            child.style.padding.top + child.style.padding.bottom + borders.top + borders.bottom;
        if item.height() < FlexPhysicalVerticalSize::new(min_height) {
            item.set_height(FlexPhysicalVerticalSize::new(min_height));
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
            line.cross_start = line.cross_start.min(cross_start);
            line.cross_end = line.cross_end.max(cross_end);
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
                                content_box_pt(height.points()),
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

    item_available.set_definite_cross_size(
        physical_direction,
        stretched_cross_size,
        FlexAvailableSizeSource::DefiniteCrossSize,
    );
    item_available.set_stretched_cross_size(physical_direction, stretched_cross_size);
    item_available
}

pub(in crate::layout::flex) fn stretched_flex_item_cross_size(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexCrossSize> {
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
        let container_cross_size =
            balanced_flex_cross_measurement_size(container_style, physical_direction, available)?;
        Some(
            (container_cross_size
                - FlexCrossLength::new(child_style.margin.top + child_style.margin.bottom))
            .non_negative_size(),
        )
    } else {
        if !child_style.box_values.width.is_auto() {
            return None;
        }
        let container_cross_size =
            balanced_flex_cross_measurement_size(container_style, physical_direction, available)?;
        Some(
            (container_cross_size
                - FlexCrossLength::new(child_style.margin.left + child_style.margin.right))
            .non_negative_size(),
        )
    }
}

/// Select the physical cross-size constraint that flex-item measurement may
/// consume for stretching.
///
/// A balanced container with an explicit line count reserves one equal
/// cross-axis slot for each planned line before item measurement.  That slot
/// is a layout constraint, whereas the unmodified container content box
/// remains the percentage basis for percentage-valued item properties.  Do
/// not recover the slot from `FlexAvailableSpace::cross_basis`: doing so
/// would discard the balanced constraint and make every item measure against
/// the whole container again.
/// <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
fn balanced_flex_cross_measurement_size(
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexCrossSize> {
    let has_explicit_balanced_line_count = container_style.flex_wrap.balances_lines()
        && matches!(
            container_style.flex_line_count,
            css::FlexLineCount::Count(line_count) if line_count.get() > 1
        );
    if has_explicit_balanced_line_count {
        return if physical_direction.is_row_axis() {
            available
                .height_constraint()
                .map(|height| FlexCrossSize::new(height.points()))
        } else {
            Some(FlexCrossSize::new(available.width.points()))
        };
    }
    available.definite_cross_size(physical_direction)
}

impl<'a> LayoutBuilder<'a> {
    /// Measure the line-box span that an already-sized flex item produces in
    /// its independent normal-flow formatting context.
    ///
    /// This probe deliberately uses the exact placed-item replay boundary.
    /// The Taffy rectangle remains useful for resolving flexible lengths, but
    /// it cannot stand in for the selected in-flow line boxes used by the
    /// Flexbox cross-size algorithm.  Snapshot restoration makes the probe
    /// geometry-only: paint, positioned descendants, and fragmentation keep
    /// their ordinary replay ownership.
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
    /// <https://www.w3.org/TR/css-inline-3/#line-box>
    fn measure_final_normal_flow_line_box_spans(
        &mut self,
        items: &[FlexItemLayout],
        estimates: &mut [FlexItemEstimate],
        children: &[StyledChild<'_>],
        container_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available: FlexAvailableSpace,
    ) {
        // Keep the normal-flow probe close to its local origin. The probe
        // deliberately avoids the page-start branch, but subtracting two
        // coordinates near the scratch page origin loses sub-point line-box
        // precision in f32 and makes its result differ from ordinary replay.
        const SCRATCH_TOP: f32 = 1.0;
        let direction = PhysicalFlexDirection::new(physical_flex_direction(container_style));
        for ((item, estimate), child) in items.iter().zip(estimates.iter_mut()).zip(children) {
            if flex_item_is_collapsed(&child.style) {
                continue;
            }
            let replay_dimensions = item.replay_dimensions();
            let mut replay_style = child.style.clone();
            freeze_replayed_item_padding(
                &mut replay_style,
                flex_item_used_padding(&child.style, container_style, available),
            );
            let placed_style = placed_flex_item_style(
                &replay_style,
                replay_dimensions.border_box_width(),
                replay_dimensions.border_box_height(),
                direction,
            );
            let snapshot = self.snapshot();
            let span = self.with_placed_formatting_context(
                PlacedFormattingContext {
                    content_left: 0.0,
                    content_width: replay_dimensions.available_width_for_replay(),
                    content_height: Some(replay_dimensions.available_height_for_replay()),
                    table_wrapper_border_box_block_size: auto_table_wrapper_block_size_override(
                        &child.style,
                        replay_dimensions.border_box_height(),
                    ),
                    writing_mode: placed_style.writing_mode,
                    scope_content_logical_inline_size: child.anonymous_content().is_some(),
                    cursor_y: SCRATCH_TOP,
                    page_start_margin_policy: PageStartMarginPolicy::Suppress,
                    float_scope: ReplayFloatScope::IsolatedFormattingContext,
                },
                &placed_style,
                |layout| {
                    layout.layout_flex_item_contents(
                        child,
                        &placed_style,
                        stylesheets,
                        PercentageBasis::indefinite(),
                    );
                    PhysicalContentHeight::new(content_box_pt(
                        (SCRATCH_TOP - layout.cursor_y).max(0.0),
                    ))
                },
            );
            self.restore(snapshot);
            estimate.set_normal_flow_line_box_span(span);
        }
    }
}

/// Replace the provisional physical block span of an automatic flex item
/// with the span selected by its final normal-flow line boxes.
///
/// A column item's physical block axis is its flex main axis, so this must be
/// done before canonical line collection and main-axis repacking.  For rows,
/// only baseline participants need this early replacement: stretch receives
/// its used cross size from the resolved line slot later in the algorithm.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
fn apply_final_normal_flow_item_block_spans(
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        if !final_normal_flow_block_span_replaces_provisional_height(
            &child.style,
            physical_direction,
            estimate.main_size_provenance,
        ) {
            continue;
        }
        let Some(span) = estimate.normal_flow_line_box_span() else {
            continue;
        };
        let baseline_participant = flex_baseline_set(&child.style, container_style).is_some()
            && !flex_item_has_auto_cross_margin(&child.style, physical_direction)
            && flex_item_baseline_axis_is_parallel_to_main_axis(&child.style, physical_direction);
        if physical_direction.is_column_axis() || baseline_participant {
            item.set_height(FlexPhysicalVerticalSize::new(span.points()));
        }
    }
}

/// Return whether final normal-flow block geometry replaces Taffy's
/// provisional physical height for this item.
///
/// A row item's physical height is its cross-size, so its authored `height`
/// remains authoritative unless the established automatic-height path needs
/// the final line-box span. For a column, physical height is the main size:
/// only normal-flow content can replace a provisional span. Taffy's numeric
/// flex result no longer carries that distinction, so the resolved-basis
/// provenance travels with the estimate to this correction boundary.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
fn final_normal_flow_block_span_replaces_provisional_height(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    main_size_provenance: FlexMainSizeProvenance,
) -> bool {
    if physical_direction.is_column_axis() {
        main_size_provenance.permits_final_normal_flow_block_span()
    } else {
        style.box_values.height.is_auto()
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
            item.border_box_height(),
            FlexDefiniteSizeSource::ResolvedLineCrossSize,
        );
    }

    // A row flex container with a definite cross size gives every final
    // in-flow item a definite containing-block height for replay. This also
    // covers a non-stretched item whose automatic height was constrained by
    // a percentage `min-height` or `max-height`: the constraint resolves
    // against the container before the item's normal-flow contents replay.
    // Without this boundary, replay re-resolves that percentage against an
    // invented zero height and drops the item's block contents.
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
    // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
    if physical_direction.is_row_axis() && available.height_basis.is_definite() {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            item.border_box_height(),
            FlexDefiniteSizeSource::ResolvedLineCrossSize,
        );
    }

    if physical_direction.is_column_axis() && available.height_basis.is_definite() {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            item.border_box_height(),
            FlexDefiniteSizeSource::PostFlexingMainSizeFromDefiniteContainer,
        );
    }

    if physical_direction.is_column_axis()
        && definite_post_flexing_main_size(&child.style, physical_direction, available).is_some()
    {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            item.border_box_height(),
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
            border_box_pt(stretched_height.points()),
            FlexDefiniteSizeSource::StretchedCrossSizeFromDefiniteSingleLineContainer,
        );
    }

    // An auto-sized row container determines a stretched item's line cross
    // size during flex layout. That size is definite for laying out its
    // descendants, including descendants with percentage heights. Auto
    // cross-axis margins suppress stretch, however, so they must not turn the
    // item's content-derived used height into a percentage basis:
    // <https://drafts.csswg.org/css-flexbox/#definite-sizes>.
    if physical_direction.is_row_axis()
        && !available.height_basis.is_definite()
        && !flex_item_has_auto_cross_margin(&child.style, physical_direction)
    {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            item.border_box_height(),
            FlexDefiniteSizeSource::ResolvedLineCrossSize,
        );
    }

    PercentageBasis::indefinite()
}

fn definite_flex_basis_main_size(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexMainSize> {
    let css::ComputedFlexBasis::LengthPercentage(ref length) = style.flex_basis else {
        return None;
    };
    used_length_percentage_or_auto_with_basis(
        css::ComputedLengthPercentageOrAuto::LengthPercentage(length.value.clone()),
        available.main_basis(physical_direction),
    )
    .map(flex_main_size_from_layout_extent)
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
) -> Option<FlexMainSize> {
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
            .map(flex_main_size_from_content_box)
        } else {
            used_content_box_height_or_auto_with_basis(
                style,
                available.height_basis,
                non_content_pt(
                    style.padding.top + style.padding.bottom + vertical_border_width(style),
                ),
            )
            .map(flex_main_size_from_content_box)
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
) -> Option<FlexMainSize> {
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
            .map(PhysicalContentHeight::content_box_length)
            .map(flex_cross_size_from_content_box)
    } else {
        item_available
            .stretched_width
            .map(PhysicalContentWidth::content_box_length)
            .map(flex_cross_size_from_content_box)
    };
    let preferred_aspect_ratio = child_style
        .aspect_ratio
        .preferred_ratio(child.is_replaced_element(), estimate.preferred_aspect_ratio);
    let (specified_min, estimated_min, mut preferred_size, overflow) = if direction.is_row_axis() {
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
            ),
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
            ),
            flex_item_main_axis_overflow(child_style, direction),
        )
    };
    // CSS Tables supplies a used grid minimum independently from the
    // preferred wrapper size. A specified `width`/`height` can establish the
    // flex base, but it must not cap that table-specific floor to zero during
    // the generic Flexbox automatic-minimum calculation.
    // <https://drafts.csswg.org/css-tables-3/#used-min-width-of-table>
    // <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>
    if child_style.display.is_table() {
        preferred_size = None;
    }
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
    let transferred = if direction.is_row_axis() {
        automatic_minimum_transferred_size_suggestion(
            child_style,
            FlexDirection::Row,
            available
                .height_basis_content_box_length()
                .map(flex_cross_size_from_content_box),
            stretched_cross_size,
            preferred_aspect_ratio,
            estimate.min_height,
            estimate.content_height,
        )
    } else {
        automatic_minimum_transferred_size_suggestion(
            child_style,
            FlexDirection::Column,
            available
                .width_basis_content_box_length()
                .map(flex_cross_size_from_content_box),
            stretched_cross_size,
            preferred_aspect_ratio,
            estimate.min_width,
            estimate.content_width,
        )
    };
    let selection = AutomaticFlexMinimum::from_suggestions(
        content_size_suggestion,
        transferred,
        preferred_size,
        child.is_replaced_element(),
    );
    selection.debug_assert_consistent(child.is_replaced_element());
    let mut minimum = selection.used_content_box;
    // `calc-size(auto, …)` substitutes the computed content-based automatic
    // minimum after Flexbox has combined its content, transferred, and
    // specified-size suggestions.  Keep this post-layout safeguard in sync
    // with the Taffy adapter above; otherwise Taffy's final size can be
    // corrected only to the untransformed `auto` value.
    // <https://drafts.csswg.org/css-values-5/#calc-size> and
    // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>.
    let (min_content, max_content, percentage_basis, stretch) = if direction.is_row_axis() {
        (
            content_size_suggestion,
            estimate.content_width,
            available.width_basis,
            available.width.content_box_length(),
        )
    } else {
        (
            content_size_suggestion,
            estimate.content_height,
            available.height_basis,
            available
                .height
                .map(PhysicalContentHeight::content_box_length)
                .unwrap_or(estimate.content_height),
        )
    };
    minimum = specified_min
        .calc_size_with_auto_basis()
        .map(|value| {
            value
                .used_value(
                    minimum.points(),
                    min_content.points(),
                    max_content.points(),
                    minimum.points(),
                    stretch.points(),
                    PercentageBasis::definite(layout_pt(percentage_basis.points().unwrap_or(0.0))),
                )
                .cast_unit()
        })
        .unwrap_or(minimum)
        .max(content_box_pt(0.0));
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
    Some(flex_main_size_from_layout_extent(
        content_box_to_border_box_length(minimum, non_content_pt(non_content)).into_layout_length(),
    ))
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
    taffy_bridge::alignment_safety(safety)
}

pub(in crate::layout::flex) fn taffy_content_alignment(
    keyword: ContentAlignmentKeyword,
    safety: AlignmentSafety,
) -> taffy_layout::AlignContent {
    taffy_bridge::content_alignment(keyword, safety)
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
            &*child_style.box_values.height,
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
    taffy_bridge::item_alignment(
        alignment,
        if for_align_self {
            taffy_bridge::TaffyAutoAlignment::Preserve
        } else {
            taffy_bridge::TaffyAutoAlignment::Stretch
        },
    )
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
    let side = flex_unreversed_cross_start_side(style);
    if matches!(
        style.flex_wrap,
        FlexWrap::WrapReverse | FlexWrap::BalanceReverse
    ) {
        side.opposite()
    } else {
        side
    }
}

/// Return the flex cross-start side before `wrap-reverse` changes line
/// stacking.
///
/// A flex line's first/last baseline alignment edge is defined by the
/// container's ordinary cross axis. `wrap-reverse` changes the direction in
/// which whole flex lines are stacked, but does not reverse first/last
/// baseline alignment inside each line:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property> and
/// <https://www.w3.org/TR/css-flexbox-1/#valdef-align-items-baseline>.
pub(in crate::layout::flex) fn flex_unreversed_cross_start_side(
    style: &ComputedStyle,
) -> PhysicalSide {
    if style.flex_direction.is_row_axis() {
        block_start_side(style.writing_mode)
    } else {
        inline_start_side(style.writing_mode, style.used_direction())
    }
}

pub(in crate::layout::flex) fn flex_cross_end_side(style: &ComputedStyle) -> PhysicalSide {
    flex_cross_start_side(style).opposite()
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
        inline_start_side(child_style.writing_mode, child_style.used_direction())
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
        inline_end_side(child_style.writing_mode, child_style.used_direction())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_reverse_flips_the_physical_flex_cross_start_side() {
        let mut style = ComputedStyle::initial();
        style.flex_direction = FlexDirection::Row;
        style.flex_wrap = FlexWrap::Wrap;
        assert_eq!(flex_cross_start_side(&style), PhysicalSide::Top);
        assert_eq!(flex_cross_end_side(&style), PhysicalSide::Bottom);

        style.flex_wrap = FlexWrap::WrapReverse;
        assert_eq!(flex_cross_start_side(&style), PhysicalSide::Bottom);
        assert_eq!(flex_cross_end_side(&style), PhysicalSide::Top);
        assert_eq!(flex_unreversed_cross_start_side(&style), PhysicalSide::Top);
    }

    #[test]
    fn auto_cross_margin_does_not_make_an_auto_row_item_percentage_definite() {
        let mut child_style = ComputedStyle::initial();
        child_style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::Auto;
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: child_style,
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(20.0, 10.0),
        ));
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(100.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
        };

        assert_eq!(
            flex_item_final_percentage_height_basis(
                &item,
                &child,
                &ComputedStyle::initial(),
                FlexDirection::Row,
                available,
            ),
            PercentageBasis::indefinite(),
        );
    }

    fn definite_height_style() -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(40.0),
            ),
        );
        style
    }

    #[test]
    fn content_basis_column_uses_final_block_span_despite_definite_height() {
        let mut style = definite_height_style();
        style.flex_basis = css::ComputedFlexBasis::Content;

        assert!(final_normal_flow_block_span_replaces_provisional_height(
            &style,
            FlexDirection::Column,
            FlexMainSizeProvenance::NormalFlowContent,
        ));
    }

    #[test]
    fn column_final_block_span_replacement_requires_normal_flow_content() {
        let style = definite_height_style();

        for provenance in [
            FlexMainSizeProvenance::AspectRatioTransfer,
            FlexMainSizeProvenance::MainSizeProperty,
            FlexMainSizeProvenance::DefiniteFlexBasis,
        ] {
            assert!(!final_normal_flow_block_span_replaces_provisional_height(
                &style,
                FlexDirection::Column,
                provenance,
            ));
        }
    }

    #[test]
    fn row_content_basis_does_not_replace_definite_cross_height() {
        let mut style = definite_height_style();
        style.flex_basis = css::ComputedFlexBasis::Content;

        assert!(!final_normal_flow_block_span_replaces_provisional_height(
            &style,
            FlexDirection::Row,
            FlexMainSizeProvenance::NormalFlowContent,
        ));
    }
}
