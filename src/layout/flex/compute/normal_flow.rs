use super::*;
use crate::layout::flex::layout::placed_flex_item_style;
use crate::units::{Definite, border_box_to_content_box_length, content_box_to_border_box_length};

impl<'a> LayoutBuilder<'a> {
    /// Measure the line-box span that an already-sized flex item produces in
    /// its independent normal-flow formatting context.
    ///
    /// This probe deliberately uses the exact placed-item replay boundary.
    /// The Taffy rectangle remains useful for resolving flexible lengths, but
    /// it cannot stand in for the selected in-flow line boxes used by the
    /// Flexbox cross-size algorithm. A detached replay transaction makes the
    /// probe geometry-only: paint, positioned descendants, and fragmentation
    /// stay out of the committed document.
    /// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
    /// <https://www.w3.org/TR/css-inline-3/#line-box>
    pub(super) fn measure_final_normal_flow_line_box_spans(
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
                available
                    .definite_cross_size(physical_flex_direction(container_style))
                    .is_some(),
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
                estimate,
                child,
                container_style,
                physical_flex_direction(container_style),
                available,
            );
            let span = self.with_speculative_layout(|layout| {
                layout.with_placed_formatting_context(
                    PlacedFormattingContext {
                        content_left: 0.0,
                        content_width: replay_dimensions.available_width_for_replay(),
                        // A row probe measures automatic cross size after Flexbox
                        // resolves its width. A column content-basis probe instead
                        // measures the main size itself, where a provisional Taffy
                        // height would make a nested wrapped flexbox wrap against
                        // its own estimate rather than its max-content extent.
                        // <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>
                        content_height: (!measurement_mode.measures_automatic_block_size()).then(
                            || Definite::new(replay_dimensions.available_height_for_replay()),
                        ),
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
                            .flatten()
                            .or_else(|| {
                                Some(
                                    replay_dimensions
                                        .logical_inline_content_size_for_replay(&placed_style),
                                )
                            }),
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
                )
            });
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
pub(in crate::layout::flex) fn final_normal_flow_content_block_span(
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
pub(in crate::layout::flex) fn apply_final_normal_flow_item_block_spans(
    items: &mut [FlexItemLayout],
    estimates: &mut [FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    cross_size_is_definite: bool,
) {
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        if !final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
            &child.style,
            physical_direction,
            estimate.main_size_provenance,
            cross_size_is_definite,
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

/// Determine whether normal-flow measurement may replace Taffy's provisional
/// row cross size.
///
/// A final line's definite cross size must be replayed into an item with a
/// percentage min/max cross constraint. Measuring that item again as an
/// unconstrained automatic block box would discard the resolved percentage
/// and let fragmentainer height become its flex-line contribution.
/// <https://drafts.csswg.org/css-flexbox-1/#algo-cross-item>
pub(in crate::layout::flex) fn final_normal_flow_block_span_replaces_provisional_height_with_cross_definiteness(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    main_size_provenance: FlexMainSizeProvenance,
    cross_size_is_definite: bool,
) -> bool {
    physical_direction.is_row_axis()
        && style.box_values.height.is_auto()
        && main_size_provenance.permits_final_normal_flow_block_span()
        && !(cross_size_is_definite
            && (!style.box_values.min_height.is_auto() || !style.box_values.max_height.is_auto()))
}

pub(in crate::layout::flex) fn assign_flex_item_percentage_height_bases(
    states: &mut [FlexItemSizingState],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) {
    for (state, child) in states.iter_mut().zip(children) {
        let basis = flex_item_final_percentage_height_basis(
            state.allocation(),
            state.estimate(),
            child,
            container_style,
            physical_direction,
            available,
        );
        let item = state.allocation_mut();
        item.percentage_height_basis = basis;
    }
}

pub(in crate::layout::flex) fn flex_item_final_percentage_height_basis(
    item: &FlexItemLayout,
    estimate: FlexItemEstimate,
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
        && (estimate.main_size_provenance.is_definite()
            || definite_post_flexing_main_size(&child.style, physical_direction, available)
                .is_some())
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
