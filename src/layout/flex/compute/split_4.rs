use super::*;

pub(in crate::layout::flex) fn synthesis_writing_mode(
    child_style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_line_axis: PhysicalAxis,
) -> WritingMode {
    if block_start_side(child_style.writing_mode).axis() != baseline_line_axis {
        return child_style.writing_mode;
    }
    if block_start_side(container_style.writing_mode).axis() != baseline_line_axis {
        return container_style.writing_mode;
    }
    match (child_style.writing_mode, child_style.direction) {
        (
            WritingMode::VerticalRl
            | WritingMode::VerticalLr
            | WritingMode::SidewaysRl
            | WritingMode::SidewaysLr,
            _,
        ) => WritingMode::HorizontalTb,
        (WritingMode::HorizontalTb, Direction::Ltr) => WritingMode::VerticalLr,
        (WritingMode::HorizontalTb, Direction::Rtl) => WritingMode::VerticalRl,
    }
}

pub(in crate::layout::flex) fn line_under_side(writing_mode: WritingMode) -> PhysicalSide {
    css::line_under_side(writing_mode)
}

pub(in crate::layout::flex) fn line_over_side(writing_mode: WritingMode) -> PhysicalSide {
    css::line_over_side(writing_mode)
}

/// Return a flex item's absolute baseline coordinate in the flex line cross
/// axis.
///
/// CSS Flexbox aligns row flex-line baseline sets in the row cross axis. For
/// horizontal writing modes that coordinate is physical y; for vertical
/// writing modes the row cross axis is physical x, so Quire uses the
/// vertical-text horizontal baseline estimates recorded from inline painting:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn measured_item_cross_axis_baseline(
    item: &FlexItemLayout,
    estimate: &FlexItemEstimate,
    style: &ComputedStyle,
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> FlexCrossOffset {
    if physical_direction.is_row_axis() {
        let baseline = flex_item_vertical_border_box_baseline_coordinate(
            item,
            measured_item_border_box_baseline(item, estimate, style, container_style, baseline_set),
        );
        return FlexCrossOffset::new(baseline.points());
    }
    let baseline = flex_item_horizontal_border_box_baseline_coordinate(
        item,
        measured_item_horizontal_border_box_baseline(
            item,
            estimate,
            style,
            container_style,
            baseline_set,
        ),
    );
    FlexCrossOffset::new(baseline.points())
}

/// The spec-selected source of one main-axis flex-container baseline.
///
/// Keeping shared, measured-item, and synthesized-item sources distinct makes
/// the priority order in CSS Flexbox 8.5 explicit. In particular, an absent
/// requested sharing group must check the opposite sharing group before an
/// item baseline or border-edge synthesis is considered:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlexContainerMainAxisBaselineSource {
    Shared {
        baseline_set: FlexBaselineSet,
    },
    Item {
        index: usize,
        baseline_set: FlexBaselineSet,
    },
    SynthesizedItem {
        index: usize,
        baseline_set: FlexBaselineSet,
    },
}

/// Return the first and last baseline sets exported by a flex container.
///
/// Flexbox first identifies the startmost/endmost finalized flex line. When
/// the compatible exported baseline is item-derived rather than a shared
/// main-axis line baseline, item selection remains scoped to that selected
/// line. Both stages operate after `order`, `flex-direction`, and final line
/// placement:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn flex_container_baselines(
    lines: &[FlexLineLayout],
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexContainerBaselineSets {
    let Some((first_line, last_line)) = flex_container_baseline_lines(lines, container_style)
    else {
        return FlexContainerBaselineSets::default();
    };
    let inline_axis = inline_start_side(
        container_style.writing_mode,
        container_style.used_direction(),
    )
    .axis();
    let main_axis = flex_baseline_line_axis(container_style);
    let first = if inline_axis == main_axis {
        flex_container_main_axis_baseline(
            first_line,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::First,
            physical_direction,
        )
        .map(|baseline| flex_cross_offset_as_physical_baseline(baseline, physical_direction))
    } else {
        flex_container_baseline_item(
            first_line,
            items,
            children,
            container_style,
            physical_direction,
            FlexBaselineSet::First,
        )
        .map(|index| {
            flex_item_baseline_for_container_axis(
                index,
                items,
                estimates,
                children,
                container_style,
                FlexBaselineSet::First,
                inline_axis,
            )
        })
    };
    let last = if inline_axis == main_axis {
        flex_container_main_axis_baseline(
            last_line,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::Last,
            physical_direction,
        )
        .map(|baseline| flex_cross_offset_as_physical_baseline(baseline, physical_direction))
    } else {
        flex_container_baseline_item(
            last_line,
            items,
            children,
            container_style,
            physical_direction,
            FlexBaselineSet::Last,
        )
        .map(|index| {
            flex_item_baseline_for_container_axis(
                index,
                items,
                estimates,
                children,
                container_style,
                FlexBaselineSet::Last,
                inline_axis,
            )
        })
    };
    FlexContainerBaselineSets {
        vertical: FlexItemBaselinePair {
            first: match first {
                Some(FlexPhysicalBaselineOffset::Vertical(baseline)) => Some(baseline),
                _ => None,
            },
            last: match last {
                Some(FlexPhysicalBaselineOffset::Vertical(baseline)) => Some(baseline),
                _ => None,
            },
        },
        horizontal: FlexItemBaselinePair {
            first: match first {
                Some(FlexPhysicalBaselineOffset::Horizontal(baseline)) => Some(baseline),
                _ => None,
            },
            last: match last {
                Some(FlexPhysicalBaselineOffset::Horizontal(baseline)) => Some(baseline),
                _ => None,
            },
        },
        vertical_metric: flex_container_baseline_metric(container_style),
        horizontal_metric: flex_container_baseline_metric(container_style),
    }
}

#[allow(clippy::too_many_arguments)]
fn flex_container_main_axis_baseline(
    fallback_line: &FlexLineLayout,
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    physical_direction: FlexDirection,
) -> Option<FlexCrossOffset> {
    let source = flex_container_main_axis_baseline_source(
        fallback_line,
        estimates,
        children,
        container_style,
        baseline_set,
    )?;
    match source {
        FlexContainerMainAxisBaselineSource::Shared { baseline_set } => match baseline_set {
            FlexBaselineSet::First => fallback_line.first_baseline,
            FlexBaselineSet::Last => fallback_line.last_baseline,
        },
        FlexContainerMainAxisBaselineSource::Item {
            index,
            baseline_set,
        }
        | FlexContainerMainAxisBaselineSource::SynthesizedItem {
            index,
            baseline_set,
        } => Some(measured_item_cross_axis_baseline(
            &items[index],
            &estimates[index],
            &children[index].style,
            container_style,
            baseline_set,
            physical_direction,
        )),
    }
}

fn flex_container_main_axis_baseline_source(
    fallback_line: &FlexLineLayout,
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> Option<FlexContainerMainAxisBaselineSource> {
    let shared_baseline = |line: &FlexLineLayout, set| match set {
        FlexBaselineSet::First => line.first_baseline,
        FlexBaselineSet::Last => line.last_baseline,
    };
    // Baseline-sharing priority belongs to the selected finalized
    // startmost/endmost flex line. `wrap-reverse` changes which physical line
    // occupies that edge, so order-modified line rank must not replace the
    // line selected by `flex_container_baseline_lines`.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
    if shared_baseline(fallback_line, baseline_set).is_some() {
        return Some(FlexContainerMainAxisBaselineSource::Shared { baseline_set });
    }
    let opposite_set = baseline_set.opposite();
    if shared_baseline(fallback_line, opposite_set).is_some() {
        return Some(FlexContainerMainAxisBaselineSource::Shared {
            baseline_set: opposite_set,
        });
    }

    let baseline_line_axis = flex_baseline_line_axis(container_style);
    let measured_set = |index: usize| {
        [baseline_set, opposite_set]
            .into_iter()
            .find(|&set| {
                flex_item_baseline_source(&estimates[index], set, baseline_line_axis)
                    == FlexBaselineSource::Measured
            })
            .map(|set| FlexContainerMainAxisBaselineSource::Item {
                index,
                baseline_set: set,
            })
    };
    let active_indices = || {
        fallback_line.item_indices.iter().copied().filter(|&index| {
            children
                .get(index)
                .is_some_and(|child| !flex_item_is_collapsed(&child.style))
        })
    };
    let item_source = match baseline_set {
        FlexBaselineSet::First => active_indices().find_map(measured_set),
        FlexBaselineSet::Last => active_indices().rev().find_map(measured_set),
    };
    if item_source.is_some() {
        return item_source;
    }

    let index = match baseline_set {
        FlexBaselineSet::First => active_indices().next(),
        FlexBaselineSet::Last => active_indices().next_back(),
    }?;
    Some(FlexContainerMainAxisBaselineSource::SynthesizedItem {
        index,
        baseline_set,
    })
}

fn flex_cross_offset_as_physical_baseline(
    baseline: FlexCrossOffset,
    physical_direction: FlexDirection,
) -> FlexPhysicalBaselineOffset {
    if physical_direction.is_row_axis() {
        FlexPhysicalBaselineOffset::Vertical(flex_vertical_baseline_from_points(baseline.points()))
    } else {
        FlexPhysicalBaselineOffset::Horizontal(flex_horizontal_baseline_from_points(
            baseline.points(),
        ))
    }
}

fn flex_container_baseline_item(
    line: &FlexLineLayout,
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    baseline_set: FlexBaselineSet,
) -> Option<usize> {
    let axes = WritingModeAxes::new(
        container_style.writing_mode,
        container_style.used_direction(),
    );
    let main_start = match container_style.flex_direction {
        FlexDirection::Row => LogicalSide::InlineStart,
        FlexDirection::RowReverse => LogicalSide::InlineEnd,
        FlexDirection::Column => LogicalSide::BlockStart,
        FlexDirection::ColumnReverse => LogicalSide::BlockEnd,
    };
    let main_start = axes.physical_side(main_start);

    // `FlexLineLayout::item_indices` preserves order-modified order.  Final
    // main-axis edges select the CSS startmost/endmost item, while this rank
    // resolves geometrically coincident items without falling back to DOM
    // order.  In particular, a reverse direction changes the physical edge
    // which is startmost; it must not be implemented by reversing a source
    // list after layout.
    // <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>
    let ordered_items = line
        .item_indices
        .iter()
        .copied()
        .filter(|&index| {
            children
                .get(index)
                .is_some_and(|child| !flex_item_is_collapsed(&child.style))
        })
        .enumerate()
        .collect::<Vec<_>>();
    let main_progress = |index: usize| {
        let (start, end) = item_outer_main_bounds(
            items.get(index).expect("flex line item has final geometry"),
            &children[index].style,
            physical_direction,
        );
        if main_start.is_start_edge() {
            start.points()
        } else {
            -end.points()
        }
    };
    let compare = |left: &(usize, usize), right: &(usize, usize)| {
        main_progress(left.1)
            .partial_cmp(&main_progress(right.1))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    };
    match baseline_set {
        FlexBaselineSet::First => ordered_items
            .iter()
            .min_by(|left, right| compare(left, right))
            .map(|(_, index)| *index),
        FlexBaselineSet::Last => ordered_items
            .iter()
            .max_by(|left, right| compare(left, right))
            .map(|(_, index)| *index),
    }
}

#[allow(clippy::too_many_arguments)]
fn flex_item_baseline_for_container_axis(
    index: usize,
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
    baseline_line_axis: PhysicalAxis,
) -> FlexPhysicalBaselineOffset {
    let item = &items[index];
    let child_style = &children[index].style;
    match baseline_line_axis {
        PhysicalAxis::Horizontal => {
            FlexPhysicalBaselineOffset::Vertical(flex_item_vertical_border_box_baseline_coordinate(
                item,
                measured_item_vertical_border_box_baseline_for_line_axis(
                    item,
                    &estimates[index],
                    child_style,
                    container_style,
                    baseline_set,
                    baseline_line_axis,
                ),
            ))
        }
        PhysicalAxis::Vertical => FlexPhysicalBaselineOffset::Horizontal(
            flex_item_horizontal_border_box_baseline_coordinate(
                item,
                measured_item_horizontal_border_box_baseline_for_line_axis(
                    item,
                    &estimates[index],
                    child_style,
                    container_style,
                    baseline_set,
                    baseline_line_axis,
                ),
            ),
        ),
    }
}

/// Select the startmost and endmost finalized flex lines for baseline export.
///
/// This is intentionally based on final physical line geometry, rather than
/// the order-modified line membership. The startmost/endmost terms are relative
/// to the container's ordinary writing-mode cross axis: `wrap-reverse` changes
/// flex-line stacking, but does not exchange those baseline-export edges.
/// `align-content` translations still have to be reflected before export:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
fn flex_container_baseline_lines<'a>(
    lines: &'a [FlexLineLayout],
    container_style: &ComputedStyle,
) -> Option<(&'a FlexLineLayout, &'a FlexLineLayout)> {
    let cross_start = flex_unreversed_cross_start_side(container_style);
    let (first, last) = if cross_start.is_start_edge() {
        (
            lines.iter().min_by(|left, right| {
                left.cross_start
                    .partial_cmp(&right.cross_start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            lines.iter().max_by(|left, right| {
                left.cross_end
                    .partial_cmp(&right.cross_end)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        )
    } else {
        (
            lines.iter().max_by(|left, right| {
                left.cross_end
                    .partial_cmp(&right.cross_end)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            lines.iter().min_by(|left, right| {
                left.cross_start
                    .partial_cmp(&right.cross_start)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        )
    };
    let first = first?;
    let last = last?;
    Some((first, last))
}

/// The coherent result of remeasuring a flex item after its line has resolved
/// a definite cross size.
///
/// Intrinsic measurement and the line-assigned cross border box describe one
/// post-line sizing transition. The flex-resolved main border box is not part
/// of this transition: flexible-length resolution has already finalized it.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>
#[derive(Debug, Clone, Copy)]
struct FlexPostLineRemeasurement {
    estimate: FlexItemEstimate,
    cross_border_size: FlexCrossSize,
}

impl FlexPostLineRemeasurement {
    fn new(estimate: FlexItemEstimate, cross_border_size: FlexCrossSize) -> Self {
        Self {
            estimate,
            cross_border_size,
        }
    }

    /// Commit the final used flex-item geometry from this remeasurement.
    ///
    fn apply_to_layout(self, item: &mut FlexItemLayout, axes: PhysicalFlexDirection) -> bool {
        let cross_size_changed = (item.cross_size(axes) - self.cross_border_size).abs() > 0.01;
        if cross_size_changed {
            item.set_cross_size(axes, self.cross_border_size);
        }
        // Flexible-length resolution precedes stretch; only a changed cross
        // size needs another cross-size pass.
        cross_size_changed
    }
}

/// Recompute auto cross-size flex items that depend on their flex line.
///
/// CSS Flexbox uses each item's hypothetical cross size for non-stretch
/// alignments, but that hypothetical size is still measured from block layout
/// with the flex line's available cross size. For column flex items this can
/// change shrink-to-fit auto widths and therefore float layout. Stretch items
/// also relayout against the resolved line cross size. Quire's wrapped-line
/// metadata may span to the next line start for alignment and fragmentation, so
/// this pass excludes the following cross-axis gap from the item stretch slot:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>,
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>, and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>.
pub(in crate::layout::flex) fn apply_line_cross_size_dependent_item_remeasurements(
    layout: &mut LayoutBuilder<'_>,
    items: &mut [FlexItemLayout],
    estimates: &mut [FlexItemEstimate],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    context: FlexLineCrossRemeasureContext<'_>,
) -> bool {
    let physical_direction = context.physical_direction;
    let axes = PhysicalFlexDirection::new(physical_direction);
    let mut changed = false;
    for line in lines {
        let line_cross_size =
            flex_line_item_stretch_cross_size(line, lines, context.line_cross_gap);
        for &index in &line.item_indices {
            let child = &children[index];
            let remeasure_kind = flex_item_line_cross_remeasurement_kind(
                &child.style,
                &estimates[index],
                context.container_style,
                physical_direction,
            );
            if remeasure_kind == FlexLineCrossRemeasureKind::None {
                continue;
            }

            let wrapped_column_fit_content_slot = (remeasure_kind
                == FlexLineCrossRemeasureKind::ColumnShrinkToFit
                && context.container_style.flex_wrap.wraps())
            // A wrapped column item's automatic cross size is remeasured
            // after the line's cross size has been collected. The line is
            // therefore its definite fit-content constraint, even when its
            // resolved size overflows the container's preferred cross size.
            // Using the original container width here leaves an earlier
            // shrink-to-fit estimate in place and makes float descendants
            // wrap against a narrower, obsolete containing block.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
            // <https://www.w3.org/TR/css-sizing-3/#fit-content-sizing>
            .then_some(line_cross_size);
            let cross_sizing_phase = match remeasure_kind {
                FlexLineCrossRemeasureKind::Stretch => FlexCrossSizingPhase::StretchToLine {
                    line_outer_cross_size: line_cross_size,
                },
                FlexLineCrossRemeasureKind::ColumnShrinkToFit => FlexCrossSizingPhase::Hypothetical,
                FlexLineCrossRemeasureKind::None => FlexCrossSizingPhase::Hypothetical,
            };
            let cross_resolution = FlexItemLineCrossSizeResolution::for_item(
                &child.style,
                physical_direction,
                line_cross_size,
                cross_sizing_phase,
                wrapped_column_fit_content_slot,
            );

            let mut item_available = flex_item_line_cross_available_space(
                &child.style,
                physical_direction,
                context.available,
                line_cross_size,
            );
            if remeasure_kind == FlexLineCrossRemeasureKind::Stretch
                && physical_direction.is_row_axis()
                && context.container_style.flex_wrap.wraps()
                && context.container_style.writing_mode == WritingMode::HorizontalTb
                && child.style.display.is_flex()
                && child.style.flex_wrap.wraps()
                && physical_flex_direction(&child.style).is_row_axis()
                && child.style.writing_mode == WritingMode::HorizontalTb
                && child.style.box_values.height.is_auto()
            {
                // A nested wrapped row's in-flow height depends on the
                // outer item's flexed width. `align-self: stretch` owns the
                // item's eventual used cross size, but it must not erase the
                // intrinsic cross contribution that establishes this line.
                // Re-measure against the resolved main size before refreshing
                // the line's cross slot.
                // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
                // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
                let borders = used_border_widths(&child.style);
                let horizontal_non_content = child.style.padding.left
                    + child.style.padding.right
                    + borders.left
                    + borders.right;
                let used_content_width =
                    (items[index].width().points() - horizontal_non_content).max(0.0);
                item_available.set_definite_width(
                    PhysicalContentWidth::new(content_box_pt(used_content_width)),
                    FlexAvailableSizeSource::PostFlexingMainSize,
                );
            }
            let mut remeasured = layout.estimate_flex_item_size(
                child,
                context.stylesheets,
                item_available,
                physical_direction,
            );
            let mut border_cross_size = match remeasure_kind {
                FlexLineCrossRemeasureKind::Stretch => stretched_flex_item_line_cross_border_size(
                    &child.style,
                    physical_direction,
                    line_cross_size,
                    context.available.cross_basis(physical_direction),
                ),
                FlexLineCrossRemeasureKind::ColumnShrinkToFit => cross_resolution
                    .wrapped_column_fit_content_border_size(
                        remeasured.min_width,
                        remeasured.content_width,
                    ),
                FlexLineCrossRemeasureKind::None => continue,
            };
            // Taffy's stretch adapter accepts only numeric maxima. Preserve
            // CSS intrinsic maxima here, then remeasure auto main sizes at the
            // used cross size so float and inline wrapping observe the same
            // constraint as final replay:
            // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes> and
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>.
            let intrinsic_max_cross_size = match physical_direction.is_row_axis() {
                true => match child.style.box_values.max_height {
                    css::ComputedLengthPercentageOrAuto::MinContent => Some(
                        remeasured_flex_item_cross_border_size(
                            &child.style,
                            FlexItemEstimate::fixed(
                                PhysicalContentWidth::new(remeasured.width),
                                PhysicalContentHeight::new(remeasured.min_height),
                            ),
                            physical_direction,
                        )
                        .points(),
                    ),
                    css::ComputedLengthPercentageOrAuto::MaxContent => Some(
                        remeasured_flex_item_cross_border_size(
                            &child.style,
                            FlexItemEstimate::fixed(
                                PhysicalContentWidth::new(remeasured.width),
                                PhysicalContentHeight::new(remeasured.content_height),
                            ),
                            physical_direction,
                        )
                        .points(),
                    ),
                    _ => None,
                },
                false => match child.style.box_values.max_width {
                    css::ComputedLengthPercentageOrAuto::MinContent => Some(
                        remeasured_flex_item_cross_border_size(
                            &child.style,
                            FlexItemEstimate::fixed(
                                PhysicalContentWidth::new(remeasured.min_width),
                                PhysicalContentHeight::new(remeasured.height),
                            ),
                            physical_direction,
                        )
                        .points(),
                    ),
                    css::ComputedLengthPercentageOrAuto::MaxContent => Some(
                        remeasured_flex_item_cross_border_size(
                            &child.style,
                            FlexItemEstimate::fixed(
                                PhysicalContentWidth::new(remeasured.content_width),
                                PhysicalContentHeight::new(remeasured.height),
                            ),
                            physical_direction,
                        )
                        .points(),
                    ),
                    _ => None,
                },
            };
            if let Some(max_cross_size) = intrinsic_max_cross_size
                && max_cross_size + 0.01 < border_cross_size.points()
            {
                border_cross_size = border_box_pt(max_cross_size);
            }

            // A stretch-fit size is clamped by definite min/max constraints
            // before the flex item's contents are laid out. Remeasure an
            // automatic main size against that final cross size; otherwise a
            // narrower `max-width` can leave inline content measured at the
            // unconstrained stretch width and incorrectly avoid wrapping.
            // <https://drafts.csswg.org/css-flexbox-1/#algo-stretch> and
            // <https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing>.
            let used_line_cross_size = if physical_direction.is_row_axis() {
                border_cross_size.points() + child.style.margin.top + child.style.margin.bottom
            } else {
                border_cross_size.points() + child.style.margin.left + child.style.margin.right
            };
            if (used_line_cross_size - line_cross_size.points()).abs() > 0.01
                && remeasure_kind != FlexLineCrossRemeasureKind::ColumnShrinkToFit
            {
                remeasured = layout.estimate_flex_item_size(
                    child,
                    context.stylesheets,
                    flex_item_line_cross_available_space(
                        &child.style,
                        physical_direction,
                        context.available,
                        FlexCrossSize::new(used_line_cross_size),
                    ),
                    physical_direction,
                );
            }

            let remeasurement = FlexPostLineRemeasurement::new(
                remeasured,
                FlexCrossSize::new(border_cross_size.points()),
                // Flexible lengths have already resolved the item's main
                // size. A final cross-size remeasurement may update its
                // cross contribution, but must never replace that allocation
                // with an unrelated intrinsic formatting-context span.
                // <https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths>
            );

            changed |= remeasurement.apply_to_layout(&mut items[index], axes);
            remeasured = remeasurement.estimate;

            if remeasure_kind == FlexLineCrossRemeasureKind::ColumnShrinkToFit {
                // Preserve intrinsic min/max contributions for future flex
                // calculations, but record the fit-content used width for
                // replay and line metadata. `estimate.width` is the used
                // content-box metric; `content_width` remains max-content.
                remeasured.width = cross_resolution.used_content_size(border_cross_size);
            }

            let stretched_content_based_row = remeasure_kind == FlexLineCrossRemeasureKind::Stretch
                && physical_direction.is_row_axis()
                && context.container_style.writing_mode == WritingMode::HorizontalTb
                && context.cross_constraint == FlexLineCrossConstraint::ContentBased;
            // Stretch happens after a content-based line has been sized from
            // the item's hypothetical cross contribution. This later replay
            // remeasurement must not replace that pre-line contribution with
            // the stretched used box: <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>.
            if !stretched_content_based_row {
                if remeasure_kind == FlexLineCrossRemeasureKind::Stretch
                    && physical_direction.is_row_axis()
                    && child.style.writing_mode.has_vertical_lines()
                    && matches!(
                        context.cross_constraint,
                        FlexLineCrossConstraint::BalancedLineSlot(_)
                    )
                {
                    // A balanced row's reserved cross slot is the vertical
                    // writing item's logical inline measurement constraint.
                    // Its inline children wrap into additional physical
                    // columns, rather than extending the page's physical
                    // block direction. The max-content probe can retain that
                    // unwrapped vertical span as fragmentable overflow; cap
                    // it at the resolved cross box before fragment replay
                    // turns it into an extra page slice.
                    //
                    // <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
                    // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
                    remeasured.set_fragmentable_overflow_height(PhysicalContentHeight::new(
                        content_box_pt(border_cross_size.points()),
                    ));
                }
                update_flex_item_estimate_cross_axis(
                    &mut estimates[index],
                    remeasured,
                    physical_direction,
                );
                // The completed cross-size phase owns this item's final
                // formatting-context replay.  Its source extent must come
                // from that same definite cross-size measurement: retaining
                // a larger intrinsic probe would replay a cyclic percentage
                // as though it were final content and manufacture an extra
                // fragmentainer slice.
                // <https://drafts.csswg.org/css-flexbox-1/#algo-cross-item>
                // <https://drafts.csswg.org/css-flexbox-1/#definite-sizes>
                estimates[index]
                    .set_fragmentable_overflow_height(remeasured.fragmentable_overflow_height);
            }
        }
    }

    changed
}

/// Recompute automatic cross sizes after flexing has fixed an item's main
/// size.
///
/// Initial intrinsic measurement may use an item's max-content main size in
/// order to determine its flex base size. Once flexible lengths have resolved,
/// inline content must instead lay out against the used main size before the
/// flex line derives its cross size. Stretch items follow the line-owned
/// remeasurement pass above; this pass owns the non-stretch case.
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
pub(in crate::layout::flex) struct PostFlexingMainSizeCrossRemeasureContext<'a> {
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) stylesheets: &'a Stylesheets<'a>,
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) available: FlexAvailableSpace,
}

pub(in crate::layout::flex) fn apply_post_flexing_main_size_cross_remeasurements(
    layout: &mut LayoutBuilder<'_>,
    items: &mut [FlexItemLayout],
    estimates: &mut [FlexItemEstimate],
    children: &[StyledChild<'_>],
    context: PostFlexingMainSizeCrossRemeasureContext<'_>,
) -> bool {
    let PostFlexingMainSizeCrossRemeasureContext {
        container_style,
        stylesheets,
        physical_direction,
        available,
    } = context;
    let axes = PhysicalFlexDirection::new(physical_direction);
    let mut changed = false;
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        let child_style = &child.style;
        let cross_size_is_auto = flex_item_cross_size_is_auto(child_style, physical_direction);
        let aligns_stretch = matches!(
            effective_align_self(child_style, container_style).keyword,
            SelfAlignmentKeyword::Auto
                | SelfAlignmentKeyword::Normal
                | SelfAlignmentKeyword::Stretch
        );
        let wrapped_column_line_remeasurement_owns_cross_size =
            physical_direction.is_column_axis() && container_style.flex_wrap.wraps();
        // Stretch owns an item's automatic cross size after the flex line has
        // been formed. This also holds for an auto-sized single-line
        // container: the line derives from hypothetical item sizes, then
        // stretch assigns its resulting cross size. Re-measuring that item
        // here would replace the assigned line size with an intrinsic cross
        // size, notably for orthogonal writing modes. Wrapped columns use the
        // same ownership rule for non-stretched auto widths: the earlier
        // line-aware pass has already applied the definite-container
        // fit-content constraint, while this main-size pass only sees the
        // max-content estimate and would incorrectly overwrite it.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
        if !cross_size_is_auto
            || aligns_stretch
            || wrapped_column_line_remeasurement_owns_cross_size
            || flex_item_has_auto_cross_margin(child_style, physical_direction)
        {
            continue;
        }

        let borders = used_border_widths(child_style);
        let horizontal_non_content =
            child_style.padding.left + child_style.padding.right + borders.left + borders.right;
        let vertical_non_content =
            child_style.padding.top + child_style.padding.bottom + borders.top + borders.bottom;
        let used_main_content_size = if physical_direction.is_row_axis() {
            (item.main_size(axes).points() - horizontal_non_content).max(0.0)
        } else {
            (item.main_size(axes).points() - vertical_non_content).max(0.0)
        };
        // Equality with an intrinsic main contribution does not prove that
        // the cross contribution was measured against the used main size. A
        // `flex-basis: content` float item can have the same max-content
        // width while its initial height was measured using the container's
        // constrained available width. The symmetric case arises for a
        // vertical-writing item in a physical column. Always establish the
        // post-flex main basis before deriving an automatic cross size.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>

        let mut item_available = flex_item_estimate_available_space(
            child_style,
            container_style,
            physical_direction,
            available,
        );
        if physical_direction.is_row_axis() {
            item_available.set_definite_width(
                PhysicalContentWidth::new(content_box_pt(used_main_content_size)),
                FlexAvailableSizeSource::PostFlexingMainSize,
            );
        } else {
            item_available.set_definite_height(
                PhysicalContentHeight::new(content_box_pt(used_main_content_size)),
                FlexAvailableSizeSource::PostFlexingMainSize,
            );
        }
        let remeasured =
            layout.estimate_flex_item_size(child, stylesheets, item_available, physical_direction);
        let remeasured_border_cross_size = if physical_direction.is_row_axis() {
            FlexCrossSize::new((remeasured.height.points() + vertical_non_content).max(0.0))
        } else {
            FlexCrossSize::new((remeasured.width.points() + horizontal_non_content).max(0.0))
        };
        if (item.cross_size(axes) - remeasured_border_cross_size).abs() > 0.01 {
            item.set_cross_size(axes, remeasured_border_cross_size);
            changed = true;
        }
        update_flex_item_estimate_cross_axis(estimate, remeasured, physical_direction);
    }
    changed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout::flex) enum FlexLineCrossRemeasureKind {
    None,
    Stretch,
    ColumnShrinkToFit,
}

/// The cross-axis constraint used to reconcile one flex item after Taffy has
/// selected its line.
///
/// A flex item's intrinsic estimate deliberately retains both its min-content
/// and max-content contributions.  Those are not, by themselves, its used
/// automatic cross size: a non-stretched item in a wrapped column container
/// uses fit-content sizing against the container's definite cross size.  Keep
/// that distinction explicit so a descendant remeasurement cannot promote a
/// max-content contribution back into the final flex-item border box.
///
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item>
/// <https://drafts.csswg.org/css-sizing-3/#fit-content-sizing>
#[derive(Debug, Clone, Copy)]
struct FlexItemLineCrossSizeResolution {
    /// The cross-axis slot allocated to the line, including item margins.
    line_slot: FlexCrossSize,
    /// The algorithm phase that selected the item-local cross constraint.
    cross_sizing_phase: FlexCrossSizingPhase,
    /// The final flex-line cross-size used by wrapped-column fit-content
    /// sizing. It remains distinct from a line stretch's used outer size.
    fit_content_available_slot: Option<FlexCrossSize>,
    margin_size: FlexCrossLength,
    non_content_size: NonContentLength,
}

impl FlexItemLineCrossSizeResolution {
    fn for_item(
        child_style: &ComputedStyle,
        physical_direction: FlexDirection,
        line_slot: FlexCrossSize,
        cross_sizing_phase: FlexCrossSizingPhase,
        fit_content_available_slot: Option<FlexCrossSize>,
    ) -> Self {
        let borders = used_border_widths(child_style);
        let (margin_size, non_content_size) = if physical_direction.is_row_axis() {
            (
                FlexCrossLength::new(child_style.margin.top + child_style.margin.bottom),
                non_content_pt(
                    child_style.padding.top
                        + child_style.padding.bottom
                        + borders.top
                        + borders.bottom,
                ),
            )
        } else {
            (
                FlexCrossLength::new(child_style.margin.left + child_style.margin.right),
                non_content_pt(
                    child_style.padding.left
                        + child_style.padding.right
                        + borders.left
                        + borders.right,
                ),
            )
        };
        Self {
            line_slot,
            cross_sizing_phase,
            fit_content_available_slot,
            margin_size,
            non_content_size,
        }
    }

    /// Return the used border-box cross size for a non-stretched wrapped
    /// column item. The line remains the placement slot; the definite
    /// container cross-size is the fit-content constraint.
    fn wrapped_column_fit_content_border_size(
        self,
        min_content: ContentBoxLength,
        max_content: ContentBoxLength,
    ) -> BorderBoxLength {
        debug_assert!(matches!(
            self.cross_sizing_phase,
            FlexCrossSizingPhase::Hypothetical
        ));
        let available_slot = self.fit_content_available_slot.unwrap_or(self.line_slot);
        let available_content = ((available_slot - self.margin_size)
            .non_negative_size()
            .points()
            - self.non_content_size.points())
        .max(0.0);
        let used_content = max_content
            .points()
            .min(min_content.points().max(available_content));
        border_box_pt((used_content + self.non_content_size.points()).max(0.0))
    }

    fn used_content_size(self, border_size: BorderBoxLength) -> ContentBoxLength {
        content_box_pt((border_size.points() - self.non_content_size.points()).max(0.0))
    }
}

pub(in crate::layout::flex) struct FlexLineCrossRemeasureContext<'a> {
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) stylesheets: &'a Stylesheets<'a>,
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) available: FlexAvailableSpace,
    pub(in crate::layout::flex) line_cross_gap: FlexCrossSize,
    pub(in crate::layout::flex) cross_constraint: FlexLineCrossConstraint,
}

/// Resolve auto cross sizes that depend on the final flexed main size.
///
/// CSS Flexbox determines each item's hypothetical cross size after flexing has
/// produced a used main size. CSS Sizing says a preferred aspect ratio makes
/// the auto axis ratio-dependent when the opposite axis is definite:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
/// <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
pub(in crate::layout::flex) fn apply_main_size_aspect_ratio_cross_size_corrections(
    items: &mut [FlexItemLayout],
    estimates: &mut [FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
) -> bool {
    let axes = PhysicalFlexDirection::new(physical_direction);
    let mut changed = false;
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        let child_style = &child.style;
        if flex_item_has_auto_cross_margin(child_style, physical_direction) {
            continue;
        }
        let cross_size_is_auto = if physical_direction.is_row_axis() {
            child_style.box_values.height.is_auto()
        } else {
            child_style.box_values.width.is_auto()
        };
        if !cross_size_is_auto {
            continue;
        }
        let stretch_with_definite_cross_size = matches!(
            effective_align_self(child_style, container_style).keyword,
            SelfAlignmentKeyword::Auto
                | SelfAlignmentKeyword::Normal
                | SelfAlignmentKeyword::Stretch
        ) && if physical_direction.is_row_axis() {
            // The percentage basis inherited from the containing block does
            // not make an auto-height flex container's own cross size
            // definite. Only its resolved used content height can suppress
            // the item's ratio-dependent automatic cross size here.
            // <https://www.w3.org/TR/css-flexbox-1/#definite-sizes>
            available.height.is_some()
        } else {
            available.width_basis.is_definite()
        };
        if stretch_with_definite_cross_size {
            continue;
        }
        let borders = used_border_widths(child_style);
        let horizontal_non_content = non_content_pt(
            child_style.padding.left + child_style.padding.right + borders.left + borders.right,
        );
        let vertical_non_content = non_content_pt(
            child_style.padding.top + child_style.padding.bottom + borders.top + borders.bottom,
        );
        let (main_non_content, cross_non_content) = if physical_direction.is_row_axis() {
            (horizontal_non_content, vertical_non_content)
        } else {
            (vertical_non_content, horizontal_non_content)
        };
        let main_size = item.main_size(axes).points();
        let main_content_size =
            border_box_to_content_box_length(border_box_pt(main_size), main_non_content);
        let cross_content_size = if let Some(sizing) = estimate.aspect_ratio_sizing {
            if physical_direction.is_row_axis() {
                sizing
                    .constraints
                    .constrain_height(sizing.ratio.height_from_width(main_content_size))
            } else {
                sizing
                    .constraints
                    .constrain_width(sizing.ratio.width_from_height(main_content_size))
            }
        } else {
            let Some(ratio) = child_style
                .aspect_ratio
                .preferred_ratio(child.is_replaced_element(), estimate.preferred_aspect_ratio)
            else {
                continue;
            };
            let transferred = flex_aspect_ratio_transferred_content_main_size(
                child_style,
                main_content_size,
                if physical_direction.is_row_axis() {
                    FlexDirection::Column
                } else {
                    FlexDirection::Row
                },
                ratio,
            );
            let percentage_basis = PercentageBasis::definite(available.width.content_box_length());
            if physical_direction.is_row_axis() {
                constrain_height_with_intrinsic(
                    child_style,
                    transferred,
                    estimate.min_height,
                    estimate.content_height,
                    percentage_basis,
                    vertical_non_content,
                )
            } else {
                constrain_width_with_intrinsic(
                    child_style,
                    transferred,
                    estimate.min_width,
                    estimate.content_width,
                    percentage_basis,
                    horizontal_non_content,
                )
            }
        };
        let border_cross_size =
            content_box_to_border_box_length(cross_content_size, cross_non_content).points();
        if (item.cross_size(axes) - FlexCrossSize::new(border_cross_size)).abs() > 0.01 {
            item.set_cross_size(axes, FlexCrossSize::new(border_cross_size));
            changed = true;
        }
        if physical_direction.is_row_axis() {
            estimate.height = cross_content_size;
            estimate.content_height = cross_content_size;
        } else {
            estimate.width = cross_content_size;
            estimate.content_width = cross_content_size;
        }
    }
    changed
}

pub(in crate::layout::flex) fn flex_item_line_cross_remeasurement_kind(
    child_style: &ComputedStyle,
    _estimate: &FlexItemEstimate,
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexLineCrossRemeasureKind {
    if flex_item_has_auto_cross_margin(child_style, physical_direction) {
        return FlexLineCrossRemeasureKind::None;
    }
    let align_stretches = matches!(
        effective_align_self(child_style, container_style).keyword,
        SelfAlignmentKeyword::Auto | SelfAlignmentKeyword::Normal | SelfAlignmentKeyword::Stretch
    );
    let cross_size_is_auto = flex_item_cross_size_is_auto(child_style, physical_direction);
    if !cross_size_is_auto {
        return FlexLineCrossRemeasureKind::None;
    }
    if align_stretches {
        return FlexLineCrossRemeasureKind::Stretch;
    }
    if physical_direction.is_column_axis() {
        return FlexLineCrossRemeasureKind::ColumnShrinkToFit;
    }
    FlexLineCrossRemeasureKind::None
}

pub(in crate::layout::flex) fn flex_line_cross_gap(
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
    physical_gap_width: css::ComputedGap,
    physical_gap_height: css::ComputedGap,
) -> FlexCrossSize {
    if container_style.flex_wrap == FlexWrap::NoWrap {
        return FlexCrossSize::new(0.0);
    }
    if physical_direction.is_row_axis() {
        flex_cross_gap_size(used_flex_gap_with_basis(
            physical_gap_height,
            available.height_basis,
        ))
    } else {
        flex_cross_gap_size(used_flex_gap_with_basis(
            physical_gap_width,
            available.width_basis,
        ))
    }
}

pub(in crate::layout::flex) fn flex_line_item_stretch_cross_size(
    line: &FlexLineLayout,
    lines: &[FlexLineLayout],
    line_cross_gap: FlexCrossSize,
) -> FlexCrossSize {
    let cross_size = line.cross_size();
    if line_cross_gap == FlexCrossSize::new(0.0) {
        return cross_size;
    }
    let tolerance = FlexCrossSize::new(0.01);
    let has_following_adjacent_line = lines.iter().any(|other| {
        other.cross_start > line.cross_start + tolerance
            && other.cross_start <= line.cross_end + tolerance
    });
    if has_following_adjacent_line {
        (cross_size - line_cross_gap).non_negative_size()
    } else {
        cross_size
    }
}

pub(in crate::layout::flex) fn flex_item_line_cross_available_space(
    child_style: &ComputedStyle,
    physical_direction: FlexDirection,
    available: FlexAvailableSpace,
    line_cross_size: FlexCrossSize,
) -> FlexItemAvailableSpace {
    let mut item_available = FlexItemAvailableSpace::from_container(available);
    let margins = if physical_direction.is_row_axis() {
        child_style.margin.top + child_style.margin.bottom
    } else {
        child_style.margin.left + child_style.margin.right
    };
    item_available.set_definite_cross_size(
        physical_direction,
        (line_cross_size - FlexCrossLength::new(margins)).non_negative_size(),
        FlexAvailableSizeSource::DefiniteCrossSize,
    );
    item_available
}

pub(in crate::layout::flex) fn remeasured_flex_item_cross_border_size(
    style: &ComputedStyle,
    estimate: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> BorderBoxLength {
    let borders = used_border_widths(style);
    border_box_pt(
        if physical_direction.is_row_axis() {
            estimate.height.points()
                + style.padding.top
                + style.padding.bottom
                + borders.top
                + borders.bottom
        } else {
            estimate.width.points()
                + style.padding.left
                + style.padding.right
                + borders.left
                + borders.right
        }
        .max(0.0),
    )
}

pub(in crate::layout::flex) fn stretched_flex_item_line_cross_border_size(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    line_cross_size: FlexCrossSize,
    percentage_basis: FlexAvailablePercentageBasis,
) -> BorderBoxLength {
    let borders = used_border_widths(style);
    let border_size = if physical_direction.is_row_axis() {
        let non_content = style.padding.top + style.padding.bottom + borders.top + borders.bottom;
        let outer_cross_size =
            (line_cross_size.points() - style.margin.top - style.margin.bottom).max(0.0);
        constrain_content_height(
            style,
            content_box_pt((outer_cross_size - non_content).max(0.0)),
            percentage_basis,
        )
        .points()
            + non_content
    } else {
        let non_content = style.padding.left + style.padding.right + borders.left + borders.right;
        let outer_cross_size =
            (line_cross_size.points() - style.margin.left - style.margin.right).max(0.0);
        constrain_content_width(
            style,
            content_box_pt((outer_cross_size - non_content).max(0.0)),
            percentage_basis,
        )
        .points()
            + non_content
    };
    border_box_pt(border_size.max(0.0))
}

pub(in crate::layout::flex) fn update_flex_item_estimate_cross_axis(
    estimate: &mut FlexItemEstimate,
    remeasured: FlexItemEstimate,
    physical_direction: FlexDirection,
) {
    if physical_direction.is_row_axis() {
        estimate.height = remeasured.height;
        estimate.min_height = remeasured.min_height;
        estimate.content_height = remeasured.content_height;
        // Cross-axis remeasurement refines the used Flex line contribution,
        // but it must not discard a source extent committed before Flex
        // resolved the item's main size.  That extent is solely for
        // fragmentation replay and can be longer than the used border box.
        estimate.merge_fragmentable_overflow_height(remeasured.fragmentable_overflow_height);
    } else {
        estimate.width = remeasured.width;
        estimate.min_width = remeasured.min_width;
        estimate.content_width = remeasured.content_width;
    }
    estimate.baselines = remeasured.baselines;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_line_remeasurement_preserves_flex_resolved_main_size() {
        let mut item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(40.0, 50.0),
        ));
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(100.0)),
            PhysicalContentHeight::new(content_box_pt(50.0)),
        );
        let axes = PhysicalFlexDirection::new(FlexDirection::Row);

        let cross_size_changed = FlexPostLineRemeasurement::new(estimate, FlexCrossSize::new(50.0))
            .apply_to_layout(&mut item, axes);

        assert!(!cross_size_changed);
        assert_eq!(item.main_size(axes), FlexMainSize::new(40.0));
        assert_eq!(item.cross_size(axes), FlexCrossSize::new(50.0));
    }

    #[test]
    fn wrapped_column_fit_content_keeps_a_non_stretched_item_within_container_cross_size() {
        let child_style = ComputedStyle::initial();
        let resolution = FlexItemLineCrossSizeResolution::for_item(
            &child_style,
            FlexDirection::Column,
            // Model the provisional max-content line Taffy can expose before
            // Flex reconciliation. The definite container cross size remains
            // the fit-content constraint.
            FlexCrossSize::new(300.0),
            FlexCrossSizingPhase::Hypothetical,
            Some(FlexCrossSize::new(100.0)),
        );

        let border_size = resolution
            .wrapped_column_fit_content_border_size(content_box_pt(100.0), content_box_pt(300.0));

        assert_eq!(border_size, border_box_pt(100.0));
        assert_eq!(
            resolution.used_content_size(border_size),
            content_box_pt(100.0)
        );
    }

    #[test]
    fn wrapped_column_fit_content_respects_cross_axis_box_model_extras() {
        let mut child_style = ComputedStyle::initial();
        child_style.margin.left = 5.0;
        child_style.margin.right = 5.0;
        child_style.padding.left = 10.0;
        child_style.padding.right = 10.0;
        child_style.border_widths.left = 2.0;
        child_style.border_widths.right = 2.0;
        let resolution = FlexItemLineCrossSizeResolution::for_item(
            &child_style,
            FlexDirection::Column,
            FlexCrossSize::new(200.0),
            FlexCrossSizingPhase::Hypothetical,
            Some(FlexCrossSize::new(100.0)),
        );

        let border_size = resolution
            .wrapped_column_fit_content_border_size(content_box_pt(20.0), content_box_pt(300.0));

        // 100px container slot - 10px margins - 20px padding. The initial
        // style keeps its borders `none`, so its configured border widths do
        // not contribute used box-model space.
        assert_eq!(border_size, border_box_pt(90.0));
        assert_eq!(
            resolution.used_content_size(border_size),
            content_box_pt(70.0)
        );
    }

    #[test]
    fn exported_item_baselines_use_final_border_box_origins_without_reapplying_margins() {
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(13.0, 17.0),
            ContainerSize::new(30.0, 40.0),
        ));
        let mut estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(30.0)),
            PhysicalContentHeight::new(content_box_pt(40.0)),
        );
        estimate.baselines.vertical.first = Some(flex_vertical_baseline_from_points(7.0));
        estimate.baselines.horizontal.first = Some(flex_horizontal_baseline_from_points(11.0));
        let mut child_style = ComputedStyle::initial();
        child_style.margin.top = 19.0;
        child_style.margin.left = 23.0;
        let container_style = ComputedStyle::initial();

        // The final item rect has already incorporated these margins. Both
        // cross-axis sharing and container export must therefore use only its
        // border-box origin plus the measured border-box baseline.
        assert_eq!(
            measured_item_cross_axis_baseline(
                &item,
                &estimate,
                &child_style,
                &container_style,
                FlexBaselineSet::First,
                FlexDirection::Row,
            ),
            FlexCrossOffset::new(24.0),
        );
        assert_eq!(
            measured_item_cross_axis_baseline(
                &item,
                &estimate,
                &child_style,
                &container_style,
                FlexBaselineSet::First,
                FlexDirection::Column,
            ),
            FlexCrossOffset::new(24.0),
        );

        let children = vec![StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: child_style,
        }];
        let items = vec![item];
        let estimates = vec![estimate];
        assert_eq!(
            flex_item_baseline_for_container_axis(
                0,
                &items,
                &estimates,
                &children,
                &container_style,
                FlexBaselineSet::First,
                PhysicalAxis::Horizontal,
            ),
            FlexPhysicalBaselineOffset::Vertical(flex_vertical_baseline_from_points(24.0)),
        );
        assert_eq!(
            flex_item_baseline_for_container_axis(
                0,
                &items,
                &estimates,
                &children,
                &container_style,
                FlexBaselineSet::First,
                PhysicalAxis::Vertical,
            ),
            FlexPhysicalBaselineOffset::Horizontal(flex_horizontal_baseline_from_points(24.0)),
        );
    }

    #[test]
    fn horizontal_column_synthesizes_a_vertical_inline_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(10.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 0.0),
            ContainerSize::new(10.0, 10.0),
        ));
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };

        let mut container = ComputedStyle::initial();
        container.flex_direction = FlexDirection::Column;
        assert_eq!(
            flex_container_baselines(
                &[line],
                &[item],
                &[estimate],
                &[child],
                &container,
                FlexDirection::Column,
            )
            .vertical
            .first,
            Some(flex_vertical_baseline_from_points(10.0)),
        );
    }

    #[test]
    fn horizontal_column_exports_its_first_item_main_axis_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(10.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 3.0),
            ContainerSize::new(10.0, 10.0),
        ));
        let mut estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        estimate.baselines.vertical.first = Some(flex_vertical_baseline_from_points(5.0));
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };
        let mut container = ComputedStyle::initial();
        container.flex_direction = FlexDirection::Column;
        assert_eq!(
            flex_container_baselines(
                &[line],
                &[item],
                &[estimate],
                &[child],
                &container,
                FlexDirection::Column,
            )
            .vertical
            .first,
            Some(flex_vertical_baseline_from_points(8.0)),
        );
    }

    #[test]
    fn horizontal_column_synthesizes_a_vertical_export_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(10.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let item = FlexItemLayout::new(ContainerRect::new(
            ContainerPoint::new(0.0, 3.0),
            ContainerSize::new(10.0, 10.0),
        ));
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };
        let mut container = ComputedStyle::initial();
        container.flex_direction = FlexDirection::Column;
        assert_eq!(
            flex_container_baselines(
                &[line],
                &[item],
                &[estimate],
                &[child],
                &container,
                FlexDirection::Column,
            )
            .vertical
            .first,
            Some(flex_vertical_baseline_from_points(13.0)),
        );
    }

    #[test]
    fn horizontal_column_reverse_uses_the_final_main_start_item() {
        let line = FlexLineLayout {
            item_indices: vec![0, 1],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 2,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(20.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(10.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 3.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 13.0),
                ContainerSize::new(10.0, 10.0),
            )),
        ];
        let mut first = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        first.baselines.vertical.first = Some(flex_vertical_baseline_from_points(2.0));
        let mut second = first;
        second.baselines.vertical.first = Some(flex_vertical_baseline_from_points(5.0));
        let children = vec![
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            },
            StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            },
        ];
        let mut container = ComputedStyle::initial();
        container.flex_direction = FlexDirection::ColumnReverse;

        assert_eq!(
            flex_container_baselines(
                &[line],
                &items,
                &[first, second],
                &children,
                &container,
                FlexDirection::ColumnReverse,
            )
            .vertical
            .first,
            Some(flex_vertical_baseline_from_points(18.0)),
        );
    }

    #[test]
    fn row_export_falls_back_to_the_first_item_when_first_line_has_no_set() {
        let lines = vec![
            FlexLineLayout {
                item_indices: vec![0, 1],
                logical_cross_start_rank: 0,
                source_start: 0,
                source_end: 2,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(20.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(10.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
            FlexLineLayout {
                item_indices: vec![2, 3],
                logical_cross_start_rank: 1,
                source_start: 2,
                source_end: 4,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(20.0),
                cross_start: FlexCrossOffset::new(10.0),
                cross_end: FlexCrossOffset::new(20.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
        ];
        let items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(10.0, 0.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 10.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(10.0, 10.0),
                ContainerSize::new(10.0, 10.0),
            )),
        ];
        let mut estimates = vec![
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            );
            4
        ];
        estimates[0].baselines.vertical.first = Some(flex_vertical_baseline_from_points(4.0));
        estimates[3].baselines.vertical.last = Some(flex_vertical_baseline_from_points(8.0));
        let children = (0..4)
            .map(|_| StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            })
            .collect::<Vec<_>>();

        let exported = flex_container_baselines(
            &lines,
            &items,
            &estimates,
            &children,
            &ComputedStyle::initial(),
            FlexDirection::Row,
        );
        assert_eq!(
            exported.vertical.first,
            Some(flex_vertical_baseline_from_points(4.0)),
        );
        assert_eq!(
            exported.vertical.last,
            Some(flex_vertical_baseline_from_points(18.0)),
        );
    }

    #[test]
    fn wrap_reverse_export_uses_unreversed_writing_mode_edges() {
        let lines = vec![
            FlexLineLayout {
                // This is order-modified first, but `wrap-reverse` packed it
                // at the block-end side of a horizontal writing-mode box.
                item_indices: vec![2, 3],
                logical_cross_start_rank: 0,
                source_start: 2,
                source_end: 4,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(20.0),
                cross_start: FlexCrossOffset::new(10.0),
                cross_end: FlexCrossOffset::new(20.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
            FlexLineLayout {
                item_indices: vec![0, 1],
                logical_cross_start_rank: 1,
                source_start: 0,
                source_end: 2,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(20.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(10.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
        ];
        let mut style = ComputedStyle::initial();
        style.flex_wrap = FlexWrap::WrapReverse;

        let (first, last) = flex_container_baseline_lines(&lines, &style).unwrap();
        assert_eq!(first.logical_cross_start_rank, 1);
        assert_eq!(last.logical_cross_start_rank, 0);
    }

    #[test]
    fn wrap_reverse_exports_shared_baselines_from_final_startmost_and_endmost_lines() {
        let lines = [
            FlexLineLayout {
                // Order-modified first, but wrap-reverse placed this line at
                // the physical block-end edge.
                item_indices: vec![0],
                logical_cross_start_rank: 0,
                source_start: 0,
                source_end: 1,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(10.0),
                cross_start: FlexCrossOffset::new(20.0),
                cross_end: FlexCrossOffset::new(30.0),
                first_baseline: Some(FlexCrossOffset::new(24.0)),
                last_baseline: Some(FlexCrossOffset::new(26.0)),
                collapsed_struts: Vec::new(),
            },
            FlexLineLayout {
                // Order-modified last, but this is the finalized physical
                // block-start line selected for first-baseline export.
                item_indices: vec![1],
                logical_cross_start_rank: 1,
                source_start: 1,
                source_end: 2,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(10.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(10.0),
                first_baseline: Some(FlexCrossOffset::new(4.0)),
                last_baseline: Some(FlexCrossOffset::new(6.0)),
                collapsed_struts: Vec::new(),
            },
        ];
        let items = [
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 20.0),
                ContainerSize::new(10.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(10.0, 10.0),
            )),
        ];
        let estimates = vec![
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            );
            2
        ];
        let children = (0..2)
            .map(|_| StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            })
            .collect::<Vec<_>>();
        let mut style = ComputedStyle::initial();
        style.flex_wrap = FlexWrap::WrapReverse;

        let exported = flex_container_baselines(
            &lines,
            &items,
            &estimates,
            &children,
            &style,
            FlexDirection::Row,
        );
        assert_eq!(
            exported.vertical.first,
            Some(flex_vertical_baseline_from_points(4.0)),
        );
        assert_eq!(
            exported.vertical.last,
            Some(flex_vertical_baseline_from_points(26.0)),
        );
    }

    #[test]
    fn later_flex_line_sharing_group_does_not_replace_first_line_item_fallback() {
        let lines = [
            FlexLineLayout {
                item_indices: vec![0],
                logical_cross_start_rank: 0,
                source_start: 0,
                source_end: 1,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(10.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(10.0),
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
            FlexLineLayout {
                item_indices: vec![1],
                logical_cross_start_rank: 1,
                source_start: 1,
                source_end: 2,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(10.0),
                cross_start: FlexCrossOffset::new(10.0),
                cross_end: FlexCrossOffset::new(20.0),
                first_baseline: Some(FlexCrossOffset::new(17.0)),
                last_baseline: None,
                collapsed_struts: Vec::new(),
            },
        ];
        let mut first = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        first.baselines.vertical.first = Some(flex_vertical_baseline_from_points(4.0));
        let estimates = vec![
            first,
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(10.0)),
                PhysicalContentHeight::new(content_box_pt(10.0)),
            ),
        ];
        let children = (0..2)
            .map(|_| StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            flex_container_main_axis_baseline_source(
                &lines[0],
                &estimates,
                &children,
                &ComputedStyle::initial(),
                FlexBaselineSet::First,
            ),
            Some(FlexContainerMainAxisBaselineSource::Item {
                index: 0,
                baseline_set: FlexBaselineSet::First,
            }),
        );
    }

    #[test]
    fn container_export_checks_opposite_shared_baseline_before_items() {
        let line = FlexLineLayout {
            item_indices: vec![0],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 1,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(10.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(20.0),
            first_baseline: None,
            last_baseline: Some(FlexCrossOffset::new(13.0)),
            collapsed_struts: Vec::new(),
        };
        let estimate = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        let child = StyledChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: ComputedStyle::initial(),
        };

        assert_eq!(
            flex_container_main_axis_baseline_source(
                &line,
                &[estimate],
                &[child],
                &ComputedStyle::initial(),
                FlexBaselineSet::First,
            ),
            Some(FlexContainerMainAxisBaselineSource::Shared {
                baseline_set: FlexBaselineSet::Last,
            }),
        );
    }

    #[test]
    fn container_export_prefers_measured_item_before_synthesis() {
        let line = FlexLineLayout {
            item_indices: vec![0, 1],
            logical_cross_start_rank: 0,
            source_start: 0,
            source_end: 2,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(20.0),
            cross_start: FlexCrossOffset::new(0.0),
            cross_end: FlexCrossOffset::new(20.0),
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        };
        let missing = FlexItemEstimate::fixed(
            PhysicalContentWidth::new(content_box_pt(10.0)),
            PhysicalContentHeight::new(content_box_pt(10.0)),
        );
        let mut measured = missing;
        measured.baselines.vertical.last = Some(flex_vertical_baseline_from_points(7.0));
        let children = (0..2)
            .map(|_| StyledChild {
                kind: FormattingContextChildKind::AnonymousContent {
                    children: Vec::new(),
                },
                style: ComputedStyle::initial(),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            flex_container_main_axis_baseline_source(
                &line,
                &[missing, measured],
                &children,
                &ComputedStyle::initial(),
                FlexBaselineSet::First,
            ),
            Some(FlexContainerMainAxisBaselineSource::Item {
                index: 1,
                baseline_set: FlexBaselineSet::Last,
            }),
        );
        assert_eq!(
            flex_container_main_axis_baseline_source(
                &line,
                &[missing, missing],
                &children,
                &ComputedStyle::initial(),
                FlexBaselineSet::Last,
            ),
            Some(FlexContainerMainAxisBaselineSource::SynthesizedItem {
                index: 1,
                baseline_set: FlexBaselineSet::Last,
            }),
        );
    }
}
