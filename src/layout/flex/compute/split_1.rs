use super::super::estimate::{FlexIntrinsicItem, constrain_flex_item_estimated_height};
use super::*;
use crate::layout::flex::layout::placed_flex_item_style;
use crate::layout::taffy_bridge;
use crate::units::{
    Definite, IntoLayoutLength, border_box_to_content_box_length, content_box_to_border_box_length,
};

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
enum FinalNormalFlowMeasurementMode {
    ReplayedUsedGeometry,
    RowAutomaticCrossSize,
    ColumnContentMainSize,
}

impl FinalNormalFlowMeasurementMode {
    fn for_item(
        child_style: &ComputedStyle,
        physical_direction: FlexDirection,
        main_size_provenance: FlexMainSizeProvenance,
        is_replaced: bool,
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
        if !final_normal_flow_block_span_replaces_provisional_height(
            child_style,
            physical_direction,
            main_size_provenance,
        ) {
            return Self::ReplayedUsedGeometry;
        }
        if physical_direction.is_row_axis() {
            Self::RowAutomaticCrossSize
        } else {
            Self::ColumnContentMainSize
        }
    }

    fn measures_automatic_block_size(self) -> bool {
        !matches!(self, Self::ReplayedUsedGeometry)
    }

    /// Restore the source block-axis constraints that a used-geometry replay
    /// intentionally freezes. They are needed while measuring the automatic
    /// block extent of a column item's content basis.
    fn prepare_placed_style(self, placed_style: &mut ComputedStyle, replay_style: &ComputedStyle) {
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

/// Produce the one-way used-style view consumed by Flex sizing.
///
/// Item formatting contexts retain their source styles so descendant replay
/// can resolve against their own final containing blocks. Flex itself, by
/// contrast, owns the parent-relative margin and padding resolution that
/// participates in its item and line geometry. This view therefore updates
/// only the scalar used-edge cache; the typed computed values remain intact
/// for the adapters that need percentage provenance.
/// <https://www.w3.org/TR/css-box-3/#padding-physical>
/// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
fn flex_sizing_children_with_used_box_edges<'a>(
    children: &[StyledChild<'a>],
    container_style: &ComputedStyle,
    available: FlexAvailableSpace,
) -> Vec<StyledChild<'a>> {
    let mut sizing_children = children.to_vec();
    let inline_percentage_basis = available.logical_inline_basis(container_style);
    for child in &mut sizing_children {
        apply_used_box_metrics_for_logical_inline_basis(&mut child.style, inline_percentage_basis);
    }
    sizing_children
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
        let sizing_children = flex_sizing_children_with_used_box_edges(children, style, available);
        let children = sizing_children.as_slice();
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
                ratio_only_replaced_base_size,
            };
            let resolved_flex_basis =
                resolve_taffy_flex_basis(child_style, &estimated_size, flex_basis_context);
            estimated_size.set_main_size_provenance(resolved_flex_basis.provenance);
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
                            width: flex_min_size_dimension(
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
                            ),
                            height: flex_min_size_dimension(
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
                    justify_content: taffy_justify_content(style.justify_content, flex_axes),
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
        // fallbacks depend on Quire's resolved free space, including negative
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
/// second physical column before Quire has determined the used main size.
/// The same rule applies to `wrap-reverse`; reversal has no effect when only
/// one line exists.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-line-break>
fn taffy_flex_wrap(
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
        FlexWrap::Balance => taffy_layout::FlexWrap::Wrap,
        FlexWrap::BalanceReverse => taffy_layout::FlexWrap::WrapReverse,
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
                .map(FlexItemLayout::from_taffy_rect)
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
    states: &mut [FlexItemSizingState],
    children: &[StyledChild<'_>],
) {
    for (state, child) in states.iter_mut().zip(children) {
        let estimate = state.estimate();
        let item = state.allocation_mut();
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
        } else if !child.style.overflow_y.is_scrollable() {
            // Scrollable overflow remains inside the flex item's scrollport.
            // It may contribute visual/clipped descendant paint, but it must
            // not manufacture additional page-fragment slices beyond the used
            // flex item border box; doing so turns `overflow: hidden` into an
            // overflowing, page-long item after the flex algorithm correctly
            // resolved its automatic minimum to zero.
            // <https://www.w3.org/TR/css-overflow-3/#scrollable>
            // <https://www.w3.org/TR/css-flexbox-1/#min-size-auto>
            let content_overflow = estimate.fragmentable_overflow_height.points().max(0.0);
            // The intrinsic content extent is already measured from the item's
            // border-box block start. Do not append its padding or border here:
            // the used box owns those decorations, and appending its block-end
            // edge would manufacture a later source continuation after descendant
            // overflow has been consumed.
            // <https://www.w3.org/TR/css-break-3/#box-splitting>
            item.set_fragmentation_height(PhysicalContentHeight::new(content_box_pt(
                content_overflow,
            )));
        }
        let decoration = FragmentDecoration::for_box_decoration_break(
            child.style.box_decoration_break,
            false,
            false,
        );
        if decoration.is_clone() {
            let borders = used_border_widths(&child.style);
            let reservation = FragmentDecorationReservation::new(
                decoration,
                non_content_pt(borders.top + child.style.padding.top),
                non_content_pt(child.style.padding.bottom + borders.bottom),
            );
            let source_height = (item.fragmentation_height().points()
                - reservation.block_start().points()
                - reservation.block_end().points())
            .max(0.0);
            item.configure_cloned_fragment_source(
                PhysicalContentHeight::new(content_box_pt(source_height)),
                reservation,
            );
        }
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
        states: &mut [FlexItemSizingState],
        children: &[StyledChild<'_>],
        container_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        physical_direction: FlexDirection,
        available: FlexAvailableSpace,
    ) {
        if !physical_direction.is_row_axis() {
            return;
        }
        for (state, child) in states.iter_mut().zip(children) {
            if !child.style.display.is_flex() || !child.style.box_values.height.is_auto() {
                continue;
            }
            let estimate = state.estimate();
            let item = state.allocation();
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
            state
                .estimate_mut()
                .merge_fragmentable_overflow_height(remeasured.fragmentable_overflow_height);
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
    let axes = PhysicalFlexDirection::new(physical_direction);
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
    let Some(premeasure_cross_size) = flex_item_premeasure_stretched_cross_size(
        child_style,
        container_style,
        physical_direction,
        available,
    ) else {
        return item_available;
    };
    let stretched_cross_size = premeasure_cross_size.size();

    item_available.set_definite_cross_size(
        physical_direction,
        stretched_cross_size,
        premeasure_cross_size.available_size_source(),
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

/// Return a stretch cross size that is known before flex-base calculation.
///
/// An explicit balanced line slot is known early, as is the cross size of a
/// definite single-line container. Other stretch sizes belong to final replay
/// and must not feed an item's own content-based flex base back into line
/// formation.
/// <https://drafts.csswg.org/css-flexbox/#algo-main-item>
/// <https://drafts.csswg.org/css-flexbox/#definite-sizes>
fn flex_item_premeasure_stretched_cross_size(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexPremeasureCrossSize> {
    if !matches!(
        effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    ) || flex_item_has_auto_cross_margin(child_style, physical_direction)
    {
        return None;
    }

    let container_cross_size = if container_style.flex_wrap.balances_lines()
        && container_style.flex_line_count.get() > 1
    {
        balanced_flex_cross_measurement_size(container_style, physical_direction, available)
            .map(FlexPremeasureCrossSize::BalancedLineSlot)
    } else if !container_style.flex_wrap.wraps() {
        let size = if physical_direction.is_row_axis() {
            flex_cross_size_from_content_box(available.height_basis_content_box_length()?)
        } else {
            flex_cross_size_from_content_box(available.width_basis_content_box_length()?)
        };
        Some(FlexPremeasureCrossSize::DefiniteSingleLineContainer(size))
    } else {
        None
    }?;

    let item_cross_size = if physical_direction.is_row_axis() {
        if !child_style.box_values.height.is_auto() {
            return None;
        }
        let stretch_size = (container_cross_size.size()
            - FlexCrossLength::new(child_style.margin.top + child_style.margin.bottom))
        .non_negative_size();
        // Flexbox clamps the stretched used cross size by the item's min/max
        // cross constraints before using it as the definite cross size for
        // flex-base measurement.
        // <https://drafts.csswg.org/css-flexbox/#algo-cross-item>
        flex_cross_size_from_content_box(constrain_flex_item_estimated_height(
            child_style,
            flex_cross_content_box_length(stretch_size),
            flex_cross_content_box_length(stretch_size),
            flex_cross_content_box_length(stretch_size),
            available.height_basis,
            non_content_pt(
                child_style.padding.top
                    + child_style.padding.bottom
                    + vertical_border_width(child_style),
            ),
        ))
    } else {
        if !child_style.box_values.width.is_auto() {
            return None;
        }
        let stretch_size = (container_cross_size.size()
            - FlexCrossLength::new(child_style.margin.left + child_style.margin.right))
        .non_negative_size();
        flex_cross_size_from_content_box(constrain_content_width(
            child_style,
            flex_cross_content_box_length(stretch_size),
            available.width_basis,
        ))
    };
    Some(match container_cross_size {
        FlexPremeasureCrossSize::BalancedLineSlot(_) => {
            FlexPremeasureCrossSize::BalancedLineSlot(item_cross_size)
        }
        FlexPremeasureCrossSize::DefiniteSingleLineContainer(_) => {
            FlexPremeasureCrossSize::DefiniteSingleLineContainer(item_cross_size)
        }
    })
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
    let has_explicit_balanced_line_count =
        container_style.flex_wrap.balances_lines() && container_style.flex_line_count.get() > 1;
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
        states: &mut [FlexItemSizingState],
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
        for (state, child) in states.iter_mut().zip(children) {
            if flex_item_is_collapsed(&child.style) {
                continue;
            }
            // This probe derives a `PhysicalContentHeight` from the scratch
            // layout's physical Y cursor. In vertical writing modes the
            // logical block axis instead projects to physical X, so using the
            // result would overwrite a resolved flex main size with an
            // unrelated cursor delta. Keep the replay geometry until the
            // final normal-flow probe has a typed orthogonal-axis result.
            // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
            if child.style.writing_mode.has_vertical_lines() {
                continue;
            }
            let estimate = state.estimate();
            let replay_dimensions = state.allocation().replay_dimensions();
            let mut replay_style = child.style.clone();
            freeze_replayed_item_padding(
                &mut replay_style,
                flex_item_used_padding(&child.style, container_style, available),
            );
            let measurement_mode = FinalNormalFlowMeasurementMode::for_item(
                &child.style,
                physical_flex_direction(container_style),
                estimate.main_size_provenance,
                child.is_replaced_element(),
            );
            let mut placed_style = placed_flex_item_style(
                &replay_style,
                replay_dimensions.border_box_width(),
                replay_dimensions.border_box_height(),
                direction,
            );
            measurement_mode.prepare_placed_style(&mut placed_style, &replay_style);
            // This is a final replay probe, not an intrinsic flex-base
            // measurement. Flexbox has already allocated this item, so its
            // typed used block-size basis must be available to descendants.
            // In particular, a winning percentage max-height must constrain
            // the image's line box as well as its earlier inline-size
            // contribution.
            // <https://drafts.csswg.org/css-flexbox-1/#definite-sizes>
            let percentage_height_basis = flex_item_final_percentage_height_basis(
                state.allocation(),
                child,
                container_style,
                physical_flex_direction(container_style),
                available,
            );
            let snapshot = self.snapshot();
            let span = self.with_placed_formatting_context(
                PlacedFormattingContext {
                    content_left: 0.0,
                    content_width: replay_dimensions.available_width_for_replay(),
                    // A row probe measures automatic cross size after Flexbox
                    // resolves its width. A column content-basis probe instead
                    // measures the main size itself, where a provisional Taffy
                    // height would make a nested wrapped flexbox wrap against
                    // its own estimate rather than its max-content extent.
                    // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
                    content_height: (!measurement_mode.measures_automatic_block_size())
                        .then(|| Definite::new(replay_dimensions.available_height_for_replay())),
                    table_wrapper_border_box_block_size: (!measurement_mode
                        .measures_automatic_block_size())
                    .then(|| {
                        auto_table_wrapper_block_size_override(
                            &child.style,
                            replay_dimensions.border_box_height(),
                        )
                    })
                    .flatten(),
                    replay_logical_inline_size: child
                        .anonymous_content()
                        .is_some()
                        .then(|| {
                            replay_dimensions
                                .logical_inline_size_for_replay(WritingMode::HorizontalTb, None)
                        })
                        .flatten(),
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
                        percentage_height_basis,
                        PrincipalBoxPaintMode::RootPaints,
                    );
                    // The replay cursor advances across the item's border
                    // box. Flex intrinsic metrics, however, carry the
                    // content-box contribution and add padding/borders only
                    // at the flex line-sizing boundary. Returning the raw
                    // cursor delta would therefore count the item's vertical
                    // decoration twice and retain a taller provisional line.
                    final_normal_flow_content_block_span(
                        border_box_pt((SCRATCH_TOP - layout.cursor_y).max(0.0)),
                        &placed_style,
                    )
                },
            );
            self.restore(snapshot);
            state.estimate_mut().set_normal_flow_line_box_span(span);
        }
    }
}

/// Convert a placed item's measured border-box replay extent into the
/// content-box contribution consumed by Flexbox line sizing.
///
/// Flex item intrinsic metrics are content-box quantities; the line algorithm
/// adds padding and borders through `estimated_outer_cross_size`. Keeping this
/// conversion at the final formatting-context handoff prevents replay geometry
/// from being decorated twice.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
fn final_normal_flow_content_block_span(
    replayed_border_box_span: BorderBoxLength,
    placed_style: &ComputedStyle,
) -> PhysicalContentHeight {
    PhysicalContentHeight::new(border_box_to_content_box_length(
        replayed_border_box_span,
        non_content_pt(
            placed_style.padding.top
                + placed_style.padding.bottom
                + vertical_border_width(placed_style),
        ),
    ))
}

/// Refine an automatic row item's cross contribution from the span selected by
/// its final normal-flow line boxes.
///
/// A column item's physical block axis is its flex main axis. Its Taffy
/// allocation is therefore the flex-resolved used main size and must not be
/// replaced with a formatting-context cursor extent: that extent includes
/// normal-flow overflow and can be larger than a max-clamped or flexed item.
/// Rows use the physical block axis as their cross axis, where the final line
/// boxes are the input to Flexbox's cross-size calculation. Stretch receives
/// its used cross size from the resolved line slot later in the algorithm.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
fn apply_final_normal_flow_item_block_spans(
    items: &mut [FlexItemLayout],
    estimates: &mut [FlexItemEstimate],
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
        if physical_direction.is_row_axis() {
            // The graph-backed intrinsic pass refreshed the baselines above;
            // now make its used cross contribution agree with the block
            // formatting context that selected the line boxes.  Keep the
            // fragmentable source extent independent: descendant overflow is
            // replay state, not a flex-line sizing input.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
            estimate.replace_row_cross_metrics_with_final_normal_flow_span(span);
        }
        let baseline_participant = flex_baseline_set(&child.style, container_style).is_some()
            && !flex_item_has_auto_cross_margin(&child.style, physical_direction)
            && flex_item_baseline_axis_is_parallel_to_main_axis(&child.style, physical_direction);
        if baseline_participant {
            let border_box_span = content_box_to_border_box_length(
                span.content_box_length(),
                non_content_pt(
                    child.style.padding.top
                        + child.style.padding.bottom
                        + vertical_border_width(&child.style),
                ),
            );
            item.set_height(FlexPhysicalVerticalSize::new(border_box_span.points()));
        }
    }
}

/// Return whether final normal-flow block geometry replaces Taffy's
/// provisional physical height for this item.
///
/// A row item's physical height is its cross-size, so its authored `height`
/// remains authoritative unless the established automatic-height path needs
/// the final line-box span. A column item's physical height is its flex main
/// size and remains the Taffy allocation even when its descendants' normal
/// flow extends beyond it. The resolved-basis provenance travels with the
/// estimate to this cross-size correction boundary.
/// <https://www.w3.org/TR/css-flexbox-1/#flex-basis-property>
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
fn final_normal_flow_block_span_replaces_provisional_height(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    main_size_provenance: FlexMainSizeProvenance,
) -> bool {
    physical_direction.is_row_axis()
        && style.box_values.height.is_auto()
        && main_size_provenance.permits_final_normal_flow_block_span()
}

fn assign_flex_item_percentage_height_bases(
    states: &mut [FlexItemSizingState],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) {
    for (state, child) in states.iter_mut().zip(children) {
        let basis = flex_item_final_percentage_height_basis(
            state.allocation(),
            child,
            container_style,
            physical_direction,
            available,
        );
        let item = state.allocation_mut();
        item.percentage_height_basis = basis;
    }
}

fn flex_item_final_percentage_height_basis(
    item: &FlexItemLayout,
    child: &StyledChild<'_>,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> FlexPercentageBasis {
    let vertical_non_content = non_content_pt(
        child.style.padding.top + child.style.padding.bottom + vertical_border_width(&child.style),
    );
    // A row flex item's specified physical height is already definite before
    // cross-axis alignment. Preserve it as the descendant percentage basis
    // even when the container's own percentage basis is indefinite.
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
    if physical_direction.is_row_axis()
        && used_content_box_height_or_auto_with_basis(
            &child.style,
            available.height_basis,
            vertical_non_content,
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

    // A stretch replay makes the final line slot available to an element's
    // descendants. Anonymous flex items have no descendant formatting
    // context that can consume a block-size percentage, so their automatic
    // line span remains only a numeric layout result; promoting it would
    // incorrectly feed an intrinsic probe back into itself.
    // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
    if physical_direction.is_row_axis()
        && child.element_parts().is_some()
        && child.style.box_values.height.is_auto()
        && !flex_item_has_auto_cross_margin(&child.style, physical_direction)
        && matches!(
            effective_align_self(&child.style, container_style).keyword,
            SelfAlignmentKeyword::Auto
                | SelfAlignmentKeyword::Normal
                | SelfAlignmentKeyword::Stretch
        )
    {
        return flex_item_replay_percentage_height_basis(
            &child.style,
            item.border_box_height(),
            FlexDefiniteSizeSource::StretchedCrossSizeFromResolvedLine,
        );
    }

    // A used line cross span from an auto-height row container is a numeric
    // layout result, not a definite CSS percentage basis. Treating it as
    // definite feeds descendant percentage heights back into the very
    // content contribution that selected the line size. Only the definite
    // sources above may cross the replay boundary:
    // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>.
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

/// Resolves the content-box automatic minimum main size of a flex item.
///
/// CSS Flexbox computes automatic minimum sizes from the content-based minimum
/// size for non-scrollable overflow. A preferred aspect ratio can transfer a
/// definite cross size into that minimum; non-replaced items use the larger of
/// the content and transferred suggestions, while replaced items use the smaller:
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#transferred-size-suggestion>.
///
/// This exposes the shared content-box result consumed by both intrinsic
/// contribution sizing and final flex layout. Callers apply their own outer
/// box-model conversion at the boundary where it is required.
pub(in crate::layout::flex) fn automatic_minimum_main_content_size(
    child: &StyledChild<'_>,
    estimate: &FlexItemEstimate,
    container_style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<ContentBoxLength> {
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
    let (specified_min, percentage_basis, stretch) = if direction.is_row_axis() {
        (
            child_style.box_values.min_width.clone(),
            available.width_basis,
            FlexStretchFitContext {
                available_margin_box_size: available
                    .width_basis_content_box_length()
                    .map(IntoLayoutLength::into_layout_length),
                margin_size: layout_pt(0.0),
                non_content_size: non_content_pt(0.0),
                box_sizing: child_style.box_sizing,
            },
        )
    } else {
        (
            child_style.box_values.min_height.clone(),
            available.height_basis,
            FlexStretchFitContext {
                available_margin_box_size: available
                    .height_basis_content_box_length()
                    .map(IntoLayoutLength::into_layout_length),
                margin_size: layout_pt(0.0),
                non_content_size: non_content_pt(0.0),
                box_sizing: child_style.box_sizing,
            },
        )
    };
    resolve_automatic_flex_minimum(
        specified_min,
        FlexMinSizeDimensionContext {
            style: child_style,
            direction,
            automatic_minimum_inputs: estimate.automatic_main_minimum_inputs,
            available_cross_size: if direction.is_row_axis() {
                available
                    .height_basis_content_box_length()
                    .map(flex_cross_size_from_content_box)
            } else {
                available
                    .width_basis_content_box_length()
                    .map(flex_cross_size_from_content_box)
            },
            stretched_cross_size,
            is_main_axis: true,
            overflow: flex_item_main_axis_overflow(child_style, direction),
            percentage_basis,
            stretch,
        },
    )
    .map(|minimum| minimum.used_content_box)
}

/// Resolves the automatic minimum border-box main size of a final flex item.
///
/// Taffy's final item rectangle is border-box geometry, while the shared
/// automatic-minimum resolver produces content-box geometry. Keep that
/// conversion at this final-layout boundary; intrinsic contributions instead
/// add their signed outer edges directly.
/// <https://www.w3.org/TR/css-flexbox-1/#min-size-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-item-contributions>.
pub(in crate::layout::flex) fn automatic_minimum_main_size(
    child: &StyledChild<'_>,
    estimate: &FlexItemEstimate,
    container_style: &ComputedStyle,
    direction: FlexDirection,
    available: FlexAvailableSpace,
) -> Option<FlexMainSize> {
    let child_style = &child.style;
    let minimum = automatic_minimum_main_content_size(
        child,
        estimate,
        container_style,
        direction,
        available,
    )?;
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
    FlexAxes::for_style(style).cross_start_side()
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
    FlexAxes::for_style(style).unreversed_cross_start_side()
}

pub(in crate::layout::flex) fn flex_cross_end_side(style: &ComputedStyle) -> PhysicalSide {
    FlexAxes::for_style(style).cross_end_side()
}

pub(in crate::layout::flex) fn child_self_start_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_start = flex_cross_start_side(container_style);
    let cross_axis = cross_start.axis();
    let child_axes = FlowAxes::for_style(child_style);
    let block_start = child_axes.block_start_side();
    if block_start.axis() == cross_axis {
        block_start
    } else {
        child_axes.inline_start_side()
    }
}

pub(in crate::layout::flex) fn child_self_end_side(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
) -> PhysicalSide {
    let cross_start = flex_cross_start_side(container_style);
    let cross_axis = cross_start.axis();
    let child_axes = FlowAxes::for_style(child_style);
    let block_end = child_axes.block_start_side().opposite();
    if block_end.axis() == cross_axis {
        block_end
    } else {
        child_axes.inline_start_side().opposite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_flex_sizing_view_resolves_percentage_box_edges_once() {
        let mut child_style = ComputedStyle::initial();
        child_style.box_values.padding.top = css::ComputedLengthPercentage::from_percent(1.0);
        child_style.box_values.padding.right = css::ComputedLengthPercentage::from_percent(1.0);
        child_style.box_values.padding.bottom = css::ComputedLengthPercentage::from_percent(1.0);
        child_style.box_values.padding.left = css::ComputedLengthPercentage::from_percent(1.0);
        let children = [StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: child_style,
        }];
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(12.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(12.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: None,
            height_basis: PercentageBasis::indefinite(),
        };

        let sizing_children = flex_sizing_children_with_used_box_edges(
            &children,
            &ComputedStyle::initial(),
            available,
        );

        assert_eq!(
            sizing_children[0].style.padding,
            css::Edges {
                top: 12.0,
                right: 12.0,
                bottom: 12.0,
                left: 12.0,
            }
        );
        assert!(
            sizing_children[0]
                .style
                .box_values
                .padding
                .top
                .contains_percentage()
        );
        assert_eq!(children[0].style.padding, css::Edges::ZERO);
    }

    #[test]
    fn indefinite_column_main_size_forces_a_single_taffy_line() {
        let indefinite = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(100.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            // A fragmentainer can impose a numeric layout limit without
            // making this auto-height flex container's own main size
            // definite.
            height: Some(PhysicalContentHeight::new(content_box_pt(300.0))),
            height_basis: PercentageBasis::indefinite(),
        };
        let mut auto_height = ComputedStyle::initial();
        auto_height.flex_wrap = FlexWrap::Wrap;

        assert_eq!(
            taffy_flex_wrap(&auto_height, FlexDirection::Column, indefinite),
            taffy_layout::FlexWrap::NoWrap,
        );
        auto_height.flex_wrap = FlexWrap::WrapReverse;
        assert_eq!(
            taffy_flex_wrap(&auto_height, FlexDirection::Column, indefinite),
            taffy_layout::FlexWrap::NoWrap,
        );

        let definite = FlexAvailableSpace {
            height_basis: PercentageBasis::definite_from(
                content_box_pt(100.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            ..indefinite
        };
        let mut definite_height = ComputedStyle::initial();
        definite_height.flex_wrap = FlexWrap::Wrap;
        *definite_height.box_values.height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(100.0),
        );
        assert_eq!(
            taffy_flex_wrap(&definite_height, FlexDirection::Column, definite),
            taffy_layout::FlexWrap::Wrap,
        );
    }

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

    #[test]
    fn auto_row_line_cross_span_does_not_make_percentage_height_definite() {
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
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
    fn column_final_block_span_never_replaces_the_flexed_main_size() {
        let mut style = definite_height_style();
        style.flex_basis = css::ComputedFlexBasis::Content;

        assert!(!final_normal_flow_block_span_replaces_provisional_height(
            &style,
            FlexDirection::Column,
            FlexMainSizeProvenance::NormalFlowContent,
        ));
    }

    #[test]
    fn column_final_block_span_does_not_replace_any_main_size_provenance() {
        let style = definite_height_style();

        for provenance in [
            FlexMainSizeProvenance::NormalFlowContent,
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

    #[test]
    fn row_content_basis_uses_final_block_span_for_automatic_cross_size() {
        let mut style = ComputedStyle::initial();
        style.flex_basis = css::ComputedFlexBasis::Content;

        assert!(final_normal_flow_block_span_replaces_provisional_height(
            &style,
            FlexDirection::Row,
            FlexMainSizeProvenance::NormalFlowContent,
        ));
    }

    #[test]
    fn row_final_block_span_does_not_replace_aspect_ratio_transfer() {
        let style = ComputedStyle::initial();

        assert!(!final_normal_flow_block_span_replaces_provisional_height(
            &style,
            FlexDirection::Row,
            FlexMainSizeProvenance::AspectRatioTransfer,
        ));
    }

    #[test]
    fn column_content_basis_measurement_restores_auto_height_and_source_bounds() {
        let mut source_style = definite_height_style();
        source_style.flex_basis = css::ComputedFlexBasis::Content;
        source_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(12.0),
        );
        source_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(48.0),
        );
        let mut placed_style = definite_height_style();
        placed_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(24.0),
        );
        placed_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(24.0),
        );

        let mode = FinalNormalFlowMeasurementMode::for_item(
            &source_style,
            FlexDirection::Column,
            FlexMainSizeProvenance::NormalFlowContent,
            false,
        );
        mode.prepare_placed_style(&mut placed_style, &source_style);

        assert_eq!(mode, FinalNormalFlowMeasurementMode::ColumnContentMainSize);
        assert!(placed_style.box_values.height.is_auto());
        assert_eq!(
            placed_style.box_values.min_height,
            source_style.box_values.min_height
        );
        assert_eq!(
            placed_style.box_values.max_height,
            source_style.box_values.max_height
        );
    }

    #[test]
    fn row_automatic_cross_measurement_keeps_frozen_height_bounds() {
        let mut source_style = ComputedStyle::initial();
        source_style.flex_basis = css::ComputedFlexBasis::Content;
        let mut placed_style = definite_height_style();
        let frozen_min_height = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(24.0),
        );
        placed_style.box_values.min_height = frozen_min_height.clone();
        placed_style.box_values.max_height = frozen_min_height.clone();

        let mode = FinalNormalFlowMeasurementMode::for_item(
            &source_style,
            FlexDirection::Row,
            FlexMainSizeProvenance::NormalFlowContent,
            false,
        );
        mode.prepare_placed_style(&mut placed_style, &source_style);

        assert_eq!(mode, FinalNormalFlowMeasurementMode::RowAutomaticCrossSize);
        assert!(placed_style.box_values.height.is_auto());
        assert_eq!(placed_style.box_values.min_height, frozen_min_height);
        assert_eq!(placed_style.box_values.max_height, frozen_min_height);
    }

    #[test]
    fn fixed_main_size_provenance_uses_replayed_geometry_measurement() {
        let style = definite_height_style();

        for provenance in [
            FlexMainSizeProvenance::AspectRatioTransfer,
            FlexMainSizeProvenance::MainSizeProperty,
            FlexMainSizeProvenance::DefiniteFlexBasis,
        ] {
            assert_eq!(
                FinalNormalFlowMeasurementMode::for_item(
                    &style,
                    FlexDirection::Column,
                    provenance,
                    false,
                ),
                FinalNormalFlowMeasurementMode::ReplayedUsedGeometry,
            );
        }
    }

    #[test]
    fn replaced_content_basis_uses_replayed_geometry_measurement() {
        let mut style = ComputedStyle::initial();
        style.flex_basis = css::ComputedFlexBasis::Content;

        assert_eq!(
            FinalNormalFlowMeasurementMode::for_item(
                &style,
                FlexDirection::Column,
                FlexMainSizeProvenance::NormalFlowContent,
                true,
            ),
            FinalNormalFlowMeasurementMode::ReplayedUsedGeometry,
        );
    }

    #[test]
    fn final_normal_flow_span_converts_replay_border_box_to_content_box() {
        let mut style = ComputedStyle::initial();
        style.padding.top = 2.0;
        style.padding.bottom = 3.0;
        style.border_widths.top = 1.0;
        style.border_widths.bottom = 4.0;
        style.border_styles.top = css::BorderStyle::Solid;
        style.border_styles.bottom = css::BorderStyle::Solid;

        let span = final_normal_flow_content_block_span(border_box_pt(24.0), &style);

        assert_eq!(span.points(), 14.0);
    }
}
