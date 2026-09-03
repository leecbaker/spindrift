use super::super::estimate::FlexIntrinsicItem;
use super::item_setup::prepare_flex_items;
use super::*;
use crate::layout::taffy_bridge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexCollapseMode {
    IncludeCollapsed,
    OmitCollapsed,
}

/// Selects the sizing semantics for the isolated normal-flow measurement that
/// finalizes a flex item's physical block contribution.
///
/// Taffy's rectangle is the used flex geometry for ordinary replay, but it
/// must not become an input to the measurement that replaces a
/// content-derived size.  In particular, a column item's physical block axis
/// is its main axis: replaying Taffy's provisional height as a definite CSS
/// height makes `flex-basis: content` measure itself rather than its content.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FinalNormalFlowMeasurementMode {
    ReplayedUsedGeometry,
    RowAutomaticCrossSize,
    ColumnContentMainSize,
}

impl FinalNormalFlowMeasurementMode {
    pub(super) fn for_item(
        child_style: &ComputedStyle,
        physical_direction: FlexDirection,
        main_size_provenance: FlexMainSizeProvenance,
        is_replaced: bool,
        cross_size_is_definite: bool,
    ) -> Self {
        if is_replaced {
            return Self::ReplayedUsedGeometry;
        }
        // In a column flex container `flex-basis: content` ignores the
        // preferred main-size property while finding its content basis. The
        // final probe must therefore restore `height:auto` even though the
        // allocated main size remains authoritative and is never overwritten
        // by the resulting cursor span.
        // <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>
        if physical_direction.is_column_axis()
            && main_size_provenance.permits_final_normal_flow_block_span()
        {
            return Self::ColumnContentMainSize;
        }
        if !final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
            child_style,
            physical_direction,
            main_size_provenance,
            cross_size_is_definite,
        ) {
            return Self::ReplayedUsedGeometry;
        }
        if physical_direction.is_row_axis() {
            Self::RowAutomaticCrossSize
        } else {
            Self::ColumnContentMainSize
        }
    }

    pub(super) fn measures_automatic_block_size(self) -> bool {
        !matches!(self, Self::ReplayedUsedGeometry)
    }

    /// Restore the source block-axis constraints that a used-geometry replay
    /// intentionally freezes. They are needed while measuring the automatic
    /// block extent of a column item's content basis.
    pub(super) fn prepare_placed_style(
        self,
        placed_style: &mut ComputedStyle,
        replay_style: &ComputedStyle,
    ) {
        if !self.measures_automatic_block_size() {
            return;
        }
        *placed_style.box_values.height = css::ComputedLengthPercentageOrAuto::Auto;
        if matches!(self, Self::ColumnContentMainSize) {
            placed_style.box_values.min_height = replay_style.box_values.min_height.clone();
            placed_style.box_values.max_height = replay_style.box_values.max_height.clone();
        }
    }
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
        // Resolve the flex container's logical-inline box-edge basis once at
        // the sizing boundary.  Taffy receives these edges through its own
        // typed adapter, while the intrinsic and final-line passes consume
        // the legacy scalar cache on `ComputedStyle`.  Keeping a private
        // resolved copy makes those consumers agree without mutating the
        // durable child style used to rebuild descendants for replay.
        //
        // In particular, an automatic inline flex container first measures a
        // cyclic percentage padding as zero, then reruns this layout with its
        // final definite inline size.  Its item's final padding must enlarge
        // the line's cross contribution as well as the replayed paint box.
        // <https://www.w3.org/TR/css-box-3/#padding-physical>
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
        let prepared_items = prepare_flex_items(children, style, available);
        let children = prepared_items.children.as_slice();
        let mut tree: taffy_layout::TaffyTree<FlexItemEstimate> = taffy_layout::TaffyTree::new();
        // CSS Flexbox used sizes are real-valued CSS lengths. Taffy rounds final
        // layouts by default for screen pixels; PDF emission must preserve the
        // unrounded layout and let rasterizers antialias at their output DPI.
        tree.disable_rounding();
        let flex_axes = FlexAxes::for_style(style);
        let physical_direction = flex_axes.taffy_flex_direction();
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
            let automatic_main_min_content = self.estimate_flex_item_automatic_main_min_content(
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
            set_flex_item_automatic_main_minimum_inputs(
                &mut estimated_size,
                child_style,
                physical_direction,
                automatic_main_min_content,
                preferred_aspect_ratio,
                child.is_replaced_element(),
                available,
            );
            // Preserve the fully selected inputs in the source-indexed
            // estimate too. The final post-Taffy guard reads this exact
            // record rather than reconstructing suggestions from metrics.
            estimates[source_index] = estimated_size;
            let cross_size_is_auto = flex_item_cross_size_is_auto(child_style, physical_direction);
            // Flexbox assigns ordinary stretch only after collecting and
            // sizing flex lines. The sole pre-line stretch input is the
            // explicit balanced-line slot from Flexbox Level 2; a definite
            // container cross size alone must not turn an automatic item into
            // a stretched hypothetical size.
            let premeasured_stretch = flex_item_premeasure_stretched_cross_size(
                child_style,
                style,
                physical_direction,
                item_measure_available,
            );
            let stretched_cross_size = premeasured_stretch
                .filter(|_| preferred_aspect_ratio.is_some())
                .map(FlexPremeasureCrossSize::size)
                .or_else(|| {
                    (style.flex_wrap.balances_lines() && style.flex_line_count.get() > 1)
                        .then(|| {
                            stretched_flex_item_cross_size(
                                child_style,
                                style,
                                physical_direction,
                                item_measure_available,
                            )
                        })
                        .flatten()
                });
            // A table is an independent formatting context whose caption and
            // grid are measured after Flexbox assigns the final cross slot.
            // Give Taffy that definite wrapper constraint while constructing
            // the line, rather than letting its generic leaf measurement
            // retain the pre-stretch caption-only height. Table replay then
            // consumes the same wrapper border-box span through
            // `PlacedFormattingContext`.
            // <https://drafts.csswg.org/css-flexbox-1/#algo-stretch>
            // <https://drafts.csswg.org/css-tables-3/#computing-the-table-height>
            let cross_axis_has_constraint = if physical_direction.is_row_axis() {
                !child_style.box_values.min_height.is_auto()
                    || !child_style.box_values.max_height.is_auto()
            } else {
                !child_style.box_values.min_width.is_auto()
                    || !child_style.box_values.max_width.is_auto()
            };
            // Spindrift resolves an authored non-replaced ratio in its flex
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
            let ratio_only_replaced_base_size = ratio_only_replaced_flex_base_size(
                child_style,
                &estimated_size,
                item_available,
                child_margin,
                child_padding,
                child_borders,
                preferred_aspect_ratio,
            );
            // CSS Tables defines a table's used `min-width` as at least its
            // min-content width. An authored minimum may raise that floor;
            // it must not be replaced by it. In particular, `min-width: 50%`
            // remains 50% of the definite flex container rather than being
            // silently converted to `min-content`.
            // <https://drafts.csswg.org/css-tables/#used-min-width-of-table>
            let flex_min_width = child_style.box_values.min_width.clone();
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
                ratio_only_replaced_base_size,
            };
            let resolved_flex_basis =
                resolve_taffy_flex_basis(child_style, &estimated_size, flex_basis_context);
            let resolved_flex_basis_is_definite = resolved_flex_basis.is_definite();
            estimated_size.set_main_size_provenance(resolved_flex_basis.provenance);
            debug_assert_eq!(
                estimated_size.main_size_provenance.is_definite(),
                resolved_flex_basis_is_definite
            );
            let taffy_flex_basis = resolved_flex_basis.dimension;
            // Taffy's `size` fallback participates in its leaf flex-base
            // measurement even when Flexbox supplies a definite basis.  For
            // a ratio-only replaced item, keep that fallback in sync with the
            // temporary basis rather than letting the CSS Images default
            // object size re-enter through the bridge.
            let temporary_main_content_size = ratio_only_replaced_base_size
                .map(|size| size.main_content_size(physical_direction));
            // Taffy asks a leaf measure function for an automatic main size
            // even when the flex basis is a definite length. Its generic
            // fallback would then return this item's authored main-size
            // estimate, which is incorrect: Flexbox uses the resolved flex
            // basis. Keep the full estimate for intrinsic contributions and
            // automatic minimums, but give Taffy's measure callback the
            // resolved content-box flex basis on the main axis.
            // <https://drafts.csswg.org/css-flexbox-1/#algo-main-item>.
            let mut taffy_measure_estimate = estimated_size;
            if let Some(base_size) = ratio_only_replaced_base_size {
                // This applies only to Taffy's isolated flex-base measurement.
                // The CSS cross-size property remains `auto`, so final stretch
                // and post-flex cross-size reconciliation still own the used
                // cross size after Flexbox has formed its lines.
                if physical_direction.is_row_axis() {
                    taffy_measure_estimate.height =
                        base_size.cross_content_size(physical_direction);
                } else {
                    taffy_measure_estimate.width = base_size.cross_content_size(physical_direction);
                }
            }
            if has_authored_non_replaced_ratio {
                // The leaf measure callback otherwise reintroduces the
                // authored ratio when Taffy supplies a known cross size.
                // Spindrift owns this ratio's flex sizing so it can preserve CSS
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
                                if physical_direction.is_row_axis() {
                                    temporary_main_content_size.unwrap_or(estimated_size.width)
                                } else {
                                    estimated_size.width
                                },
                                estimated_size.min_width,
                                estimated_size.content_width,
                                FlexItemSizeDimensionContext {
                                    flex_direction: physical_direction,
                                    dimension_axis: FlexDirection::Row,
                                    percentage_basis: available.width_basis,
                                    stretch: horizontal_stretch,
                                    flex_basis_overrides_main_size,
                                    cross_sizing_phase: stretched_cross_size
                                        .map(|line_outer_cross_size| {
                                            FlexCrossSizingPhase::StretchToLine {
                                                line_outer_cross_size,
                                            }
                                        })
                                        .unwrap_or(FlexCrossSizingPhase::Hypothetical),
                                    hypothetical_automatic_cross_size:
                                        wrapping_column_cross_fit_content
                                            .map(|used_content_size| {
                                                FlexHypotheticalAutomaticCrossSize::FitContent {
                                                    used_content_size,
                                                }
                                            })
                                            .unwrap_or(
                                                FlexHypotheticalAutomaticCrossSize::Intrinsic,
                                            ),
                                },
                            ),
                            height: flex_item_size_dimension(
                                child_style.box_values.height.value().clone(),
                                if physical_direction.is_column_axis() {
                                    temporary_main_content_size.unwrap_or(estimated_size.height)
                                } else {
                                    estimated_size.height
                                },
                                estimated_size.min_height,
                                estimated_size.content_height,
                                FlexItemSizeDimensionContext {
                                    flex_direction: physical_direction,
                                    dimension_axis: FlexDirection::Column,
                                    percentage_basis: available.height_basis,
                                    stretch: vertical_stretch,
                                    flex_basis_overrides_main_size,
                                    cross_sizing_phase: stretched_cross_size
                                        .map(|line_outer_cross_size| {
                                            FlexCrossSizingPhase::StretchToLine {
                                                line_outer_cross_size,
                                            }
                                        })
                                        .unwrap_or(FlexCrossSizingPhase::Hypothetical),
                                    hypothetical_automatic_cross_size:
                                        FlexHypotheticalAutomaticCrossSize::Intrinsic,
                                },
                            ),
                        },
                        aspect_ratio: taffy_aspect_ratio,
                        min_size: taffy_layout::Size {
                            width: if child_style.display.is_table()
                                && matches!(
                                    flex_min_width,
                                    css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
                                ) {
                                table_length_percentage_min_dimension(
                                    flex_min_width,
                                    available.width_basis,
                                    estimated_size.min_width,
                                )
                            } else {
                                flex_min_size_dimension(
                                    flex_min_width,
                                    physical_direction
                                        .is_row_axis()
                                        .then_some(automatic_main_min_content)
                                        .flatten()
                                        .unwrap_or(estimated_size.min_width),
                                    estimated_size.content_width,
                                    FlexMinSizeDimensionContext {
                                        style: child_style,
                                        direction: FlexDirection::Row,
                                        automatic_minimum_inputs: physical_direction
                                            .is_row_axis()
                                            .then_some(estimated_size.automatic_main_minimum_inputs)
                                            .flatten(),
                                        available_cross_size: available
                                            .height_basis_content_box_length()
                                            .map(flex_cross_size_from_content_box),
                                        cross_stretch: vertical_stretch,
                                        stretched_cross_size: physical_direction
                                            .is_row_axis()
                                            .then_some(stretched_cross_size)
                                            .flatten(),
                                        is_main_axis: physical_direction.is_row_axis(),
                                        overflow: flex_item_main_axis_overflow(
                                            child_style,
                                            physical_direction,
                                        ),
                                        percentage_basis: available.width_basis,
                                        stretch: horizontal_stretch,
                                    },
                                )
                            },
                            height: if child_style.display.is_table()
                                && matches!(
                                    flex_min_height,
                                    css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
                                ) {
                                table_length_percentage_min_dimension(
                                    flex_min_height,
                                    available.height_basis,
                                    estimated_size.min_height,
                                )
                            } else if child_style.display.is_table()
                                && physical_direction.is_column_axis()
                            {
                                // A table grid/caption cannot be compressed
                                // below its own intrinsic block minimum merely
                                // because Flexbox received `min-height: 0`.
                                taffy_layout::LengthPercentageAuto::length(
                                    estimated_size.min_height.points(),
                                )
                            } else {
                                flex_min_size_dimension(
                                    flex_min_height,
                                    physical_direction
                                        .is_column_axis()
                                        .then_some(automatic_main_min_content)
                                        .flatten()
                                        .unwrap_or(estimated_size.min_height),
                                    estimated_size.content_height,
                                    FlexMinSizeDimensionContext {
                                        style: child_style,
                                        direction: FlexDirection::Column,
                                        automatic_minimum_inputs: physical_direction
                                            .is_column_axis()
                                            .then_some(estimated_size.automatic_main_minimum_inputs)
                                            .flatten(),
                                        available_cross_size: available
                                            .width_basis_content_box_length()
                                            .map(flex_cross_size_from_content_box),
                                        cross_stretch: horizontal_stretch,
                                        stretched_cross_size: physical_direction
                                            .is_column_axis()
                                            .then_some(stretched_cross_size)
                                            .flatten(),
                                        is_main_axis: physical_direction.is_column_axis(),
                                        overflow: flex_item_main_axis_overflow(
                                            child_style,
                                            physical_direction,
                                        ),
                                        percentage_basis: available.height_basis,
                                        stretch: vertical_stretch,
                                    },
                                )
                            },
                        },
                        max_size: taffy_layout::Size {
                            width: taffy_bridge::min_max_constraint(
                                taffy_intrinsic_dimension_with_basis_and_stretch(
                                    child_style.box_values.max_width.clone(),
                                    available.width_basis,
                                    estimated_size.min_width,
                                    estimated_size.content_width,
                                    horizontal_stretch,
                                ),
                            ),
                            height: taffy_bridge::min_max_constraint(
                                taffy_intrinsic_dimension_with_basis_and_stretch(
                                    child_style.box_values.max_height.clone(),
                                    available.height_basis,
                                    estimated_size.min_height,
                                    estimated_size.content_height,
                                    vertical_stretch,
                                ),
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

        // Taffy 0.14 dispatches a node without children through its leaf
        // algorithm, even when its display is Flex.  A zero-contribution
        // probe keeps an otherwise-empty Flex container on the Flex path so
        // its authored size and CSS Align static-position rectangle remain
        // available to Spindrift.  It is never exposed as an item or a line.
        let mut layout_nodes = nodes.clone();
        if layout_nodes.is_empty() {
            let zero = taffy_layout::Dimension::length(0.0);
            let zero_constraint = taffy_layout::LengthPercentageAuto::length(0.0);
            let probe = tree
                .new_leaf_with_context(
                    taffy_layout::Style {
                        size: taffy_layout::Size {
                            width: zero,
                            height: zero,
                        },
                        min_size: taffy_layout::Size {
                            width: zero_constraint,
                            height: zero_constraint,
                        },
                        max_size: taffy_layout::Size {
                            width: zero_constraint,
                            height: zero_constraint,
                        },
                        ..Default::default()
                    },
                    FlexItemEstimate::fixed(
                        PhysicalContentWidth::new(content_box_pt(0.0)),
                        PhysicalContentHeight::new(content_box_pt(0.0)),
                    ),
                )
                .ok()?;
            layout_nodes.push(probe);
        }
        let root = tree
            .new_with_children(
                taffy_layout::Style {
                    display: taffy_layout::Display::Flex,
                    box_sizing: taffy_layout::BoxSizing::BorderBox,
                    direction: flex_axes.taffy_layout_direction(),
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
                    flex_wrap: taffy_flex_wrap(style, physical_direction, available),
                    // Taffy 0.14 owns its native Level 2 balance selection.
                    // The value is also meaningful for ordinary wrapping,
                    // where it supplies a definite per-line cross-axis
                    // measurement slot.
                    flex_line_count: u16::try_from(style.flex_line_count.get()).unwrap_or(u16::MAX),
                    justify_content: Some(taffy_justify_content(style.justify_content, flex_axes)),
                    align_content: Some(taffy_align_content(style.align_content)),
                    align_items: Some(taffy_align_items(style.align_items)),
                    gap: taffy_layout::Size {
                        width: taffy_gap(physical_gap_width.clone(), available.width_basis),
                        height: taffy_gap(physical_gap_height.clone(), available.height_basis),
                    },
                    ..Default::default()
                },
                &layout_nodes,
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
            |input, _node_id, node_context, _style| taffy_flex_measurement(input, node_context),
        )
        .ok()?;

        let root_rect = taffy_rect_from_layout(tree.layout(root).ok()?);
        let mut items = vec![FlexItemLayout::new(ContainerRect::zero()); children.len()];
        let container_cross_size = FlexCrossSize::new(if physical_direction.is_row_axis() {
            root_rect.size.height
        } else {
            root_rect.size.width
        });
        let mut active_items = nodes
            .iter()
            .map(|&node| {
                let layout = tree.layout(node).ok()?;
                Some(FlexItemLayout::from_taffy_rect(taffy_rect_from_layout(
                    layout,
                )))
            })
            .collect::<Option<Vec<_>>>()?;
        reproject_taffy_item_cross_axis_coordinates(
            &mut active_items,
            flex_axes,
            container_cross_size,
        );
        let mut active_sizing_states = active_estimates
            .into_iter()
            .zip(active_items)
            .map(|(estimate, allocation)| FlexItemSizingState::new(estimate, allocation))
            .collect::<Vec<_>>();
        self.measure_final_normal_flow_line_box_spans(
            &mut active_sizing_states,
            &active_children,
            style,
            stylesheets,
            available,
        );
        let (mut active_items, mut active_estimates) =
            FlexItemSizingState::into_parts(active_sizing_states);
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
        // The graph-backed remeasurement refreshes line baselines and other
        // intrinsic metadata at the resolved main size.  It is still an
        // estimate, however: the placed formatting-context probe above owns
        // the actual in-flow block span that Flexbox uses for line sizing.
        // Apply that result only after the metadata refresh so a second
        // approximation cannot overwrite the selected line-box geometry.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
        // <https://www.w3.org/TR/css-inline-3/#line-box>
        apply_final_normal_flow_item_block_spans(
            &mut active_items,
            &mut active_estimates,
            &active_children,
            style,
            physical_direction,
            available.definite_cross_size(physical_direction).is_some(),
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
        // Taffy provides the flexible lengths, but CSS Align distribution
        // fallbacks depend on Spindrift's resolved free space, including negative
        // overflow. Repack a row only when one of those distribution keywords
        // can select a different final placement; ordinary row placement
        // remains Taffy's allocation, which preserves its already-resolved
        // signed-margin geometry. A column always replays because normal-flow
        // measurement can refine its automatic block extent.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-main-align>
        // <https://www.w3.org/TR/css-align-3/#distribution-values>
        let distribution_may_need_final_main_axis_replay = matches!(
            style.justify_content.keyword,
            ContentAlignmentKeyword::Stretch
                | ContentAlignmentKeyword::SpaceBetween
                | ContentAlignmentKeyword::SpaceAround
                | ContentAlignmentKeyword::SpaceEvenly
        );
        if physical_direction.is_column_axis() || distribution_may_need_final_main_axis_replay {
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
                    minimum_line_count: style.flex_line_count.get(),
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
        let mut final_sizing_states = active_estimates
            .into_iter()
            .zip(active_items)
            .map(|(estimate, allocation)| FlexItemSizingState::new(estimate, allocation))
            .collect::<Vec<_>>();
        assign_flex_item_percentage_height_bases(
            &mut final_sizing_states,
            &active_children,
            style,
            physical_direction,
            available,
        );
        self.remeasure_nested_flex_fragmentable_overflow_extents(
            &mut final_sizing_states,
            &active_children,
            style,
            stylesheets,
            physical_direction,
            available,
        );
        assign_flex_item_fragmentation_heights(&mut final_sizing_states, &active_children);
        let (active_items, active_estimates) = FlexItemSizingState::into_parts(final_sizing_states);
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

/// Adapt CSS `flex-wrap` to Taffy after accounting for Flexbox line
/// collection's available-main-size rule.
///
/// A wrapping container whose main size is indefinite collects every item
/// into a single line. Taffy's max-content root probe otherwise treats an
/// automatic column height as a finite wrapping slot and can materialize a
/// second physical column before Spindrift has determined the used main size.
/// The same rule applies to `wrap-reverse`; reversal has no effect when only
/// one line exists.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-line-break>
pub(in crate::layout::flex) fn taffy_flex_wrap(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> taffy_layout::FlexWrap {
    let container_main_size_is_definite = if physical_direction.is_row_axis() {
        true
    } else {
        used_content_box_height_or_auto_with_basis(
            style,
            available.height_basis,
            non_content_pt(style.padding.top + style.padding.bottom + vertical_border_width(style)),
        )
        .is_some()
            || matches!(
                available.height_basis,
                PercentageBasis::Definite {
                    source: FlexAvailableSizeSource::AspectRatioDerived,
                    ..
                }
            )
            // An automatic column flex container still uses a resolved
            // `max-height` to collect wrapped flex lines. The maximum does
            // not make descendant percentage heights definite, but Taffy's
            // wrapping input must see the same finite main-axis limit as the
            // later canonical line collection.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-line-break>
            // <https://www.w3.org/TR/css-sizing-3/#max-size>
            || (!style.box_values.max_height.is_auto() && available.height_constraint().is_some())
    };
    if physical_direction.is_column_axis() && !container_main_size_is_definite {
        return taffy_layout::FlexWrap::NoWrap;
    }
    match style.flex_wrap {
        FlexWrap::NoWrap => taffy_layout::FlexWrap::NoWrap,
        FlexWrap::Wrap => taffy_layout::FlexWrap::Wrap,
        FlexWrap::WrapReverse => taffy_layout::FlexWrap::WrapReverse,
        FlexWrap::Balance => taffy_layout::FlexWrap::Balance,
        FlexWrap::BalanceReverse => taffy_layout::FlexWrap::BalanceReverse,
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
