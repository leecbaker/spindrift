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
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    if physical_direction.is_row_axis() {
        return item.cross_start(axes)
            + FlexCrossLength::new(style.margin.top)
            + flex_cross_length_from_vertical_baseline(measured_item_border_box_baseline(
                item,
                estimate,
                style,
                container_style,
                baseline_set,
            ));
    }
    item.cross_start(axes)
        + FlexCrossLength::new(style.margin.left)
        + flex_cross_length_from_horizontal_baseline(measured_item_horizontal_border_box_baseline(
            item,
            estimate,
            style,
            container_style,
            baseline_set,
        ))
}

/// Return the first and last baseline sets exported by a flex container.
///
/// CSS Flexbox derives a container's baseline sets from its first and last
/// flex lines. A shared line baseline wins; otherwise the startmost/endmost
/// item contributes its parallel baseline (or a synthesized border-box
/// baseline). This runs after the flex lines and items have their final
/// placement, so wrapping, `order`, `wrap-reverse`, and `align-content` are
/// already reflected in the exported coordinates:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
pub(in crate::layout::flex) fn flex_container_baselines(
    lines: &[FlexLineLayout],
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexContainerBaselineEstimate {
    let Some((first_line, last_line)) = flex_container_baseline_lines(lines, container_style)
    else {
        return FlexContainerBaselineEstimate::default();
    };

    // In horizontal writing mode, a column flex container exports the text
    // baseline of its first/last item along the physical vertical main axis.
    // This is not the column's physical horizontal cross-axis coordinate.
    // The old implementation recognized only the wrapped case and therefore
    // made ordinary inline column flexboxes fall back to a synthesized atom
    // baseline.
    if container_style.writing_mode == WritingMode::HorizontalTb
        && container_style.flex_direction.is_column_axis()
        && physical_direction.is_column_axis()
    {
        let main_axis_baseline = |line: &FlexLineLayout, baseline_set| {
            let index = flex_line_baseline_item_index(line, physical_direction, baseline_set)?;
            Some(FlexVerticalBaselineOffset::new(
                items[index].y().points()
                    + children[index].style.margin.top
                    + measured_item_vertical_border_box_baseline_for_line_axis(
                        &items[index],
                        &estimates[index],
                        &children[index].style,
                        container_style,
                        baseline_set,
                        PhysicalAxis::Horizontal,
                    )
                    .points(),
            ))
        };
        let baselines = FlexContainerBaselineEstimate {
            vertical: FlexItemBaselinePair {
                first: main_axis_baseline(first_line, FlexBaselineSet::First),
                last: main_axis_baseline(last_line, FlexBaselineSet::Last),
            },
            horizontal: FlexItemBaselinePair::default(),
        };
        return baselines;
    }

    let first = flex_line_content_baseline(
        first_line,
        items,
        estimates,
        children,
        container_style,
        FlexBaselineSet::First,
        physical_direction,
    );
    let last = flex_line_content_baseline(
        last_line,
        items,
        estimates,
        children,
        container_style,
        FlexBaselineSet::Last,
        physical_direction,
    );
    if physical_direction.is_row_axis() {
        FlexContainerBaselineEstimate {
            vertical: FlexItemBaselinePair {
                first: first.map(|baseline| FlexVerticalBaselineOffset::new(baseline.points())),
                last: last.map(|baseline| FlexVerticalBaselineOffset::new(baseline.points())),
            },
            horizontal: FlexItemBaselinePair::default(),
        }
    } else {
        FlexContainerBaselineEstimate {
            vertical: FlexItemBaselinePair::default(),
            horizontal: FlexItemBaselinePair {
                first: first.map(|baseline| FlexHorizontalBaselineOffset::new(baseline.points())),
                last: last.map(|baseline| FlexHorizontalBaselineOffset::new(baseline.points())),
            },
        }
    }
}

/// Select the startmost and endmost flex lines for container baseline export.
///
/// Flex container baseline sets are defined after `order` and flex-direction,
/// from the startmost/endmost line rather than the incidental storage order of
/// line records.  This matters for `wrap-reverse`, vertical writing modes,
/// and later `align-content` adjustments:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
fn flex_container_baseline_lines<'a>(
    lines: &'a [FlexLineLayout],
    container_style: &ComputedStyle,
) -> Option<(&'a FlexLineLayout, &'a FlexLineLayout)> {
    let cross_start_is_low_coordinate = flex_cross_start_side(container_style).is_start_edge();
    let first = if cross_start_is_low_coordinate {
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
    }?;
    let last = if cross_start_is_low_coordinate {
        lines.iter().max_by(|left, right| {
            left.cross_end
                .partial_cmp(&right.cross_end)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    } else {
        lines.iter().min_by(|left, right| {
            left.cross_start
                .partial_cmp(&right.cross_start)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }?;
    Some((first, last))
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
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    let mut changed = false;
    for line in lines {
        let line_cross_size =
            flex_line_item_stretch_cross_size(line, lines, context.line_cross_gap);
        for &index in &line.item_indices {
            let child = &children[index];
            let remeasure_kind = flex_item_line_cross_remeasurement_kind(
                &child.style,
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
            // after the line's cross size has been collected.  The line is
            // therefore its definite fit-content constraint, even when its
            // resolved size overflows the container's preferred cross size.
            // Using the original container width here leaves an earlier
            // shrink-to-fit estimate in place and makes float descendants
            // wrap against a narrower, obsolete containing block.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
            // <https://www.w3.org/TR/css-sizing-3/#fit-content-sizing>
            .then_some(line_cross_size);
            let cross_resolution = FlexItemLineCrossSizeResolution::for_item(
                &child.style,
                physical_direction,
                line_cross_size,
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
                if remeasure_kind == FlexLineCrossRemeasureKind::Stretch
                    && matches!(child.style.flex_basis, css::ComputedFlexBasis::Auto)
                {
                    let borders = used_border_widths(&child.style);
                    let automatic_main_size = if physical_direction.is_row_axis() {
                        child.style.box_values.width.is_auto().then(|| {
                            remeasured.width.points()
                                + child.style.padding.left
                                + child.style.padding.right
                                + borders.left
                                + borders.right
                        })
                    } else {
                        child.style.box_values.height.is_auto().then(|| {
                            remeasured.height.points()
                                + child.style.padding.top
                                + child.style.padding.bottom
                                + borders.top
                                + borders.bottom
                        })
                    };
                    if let Some(automatic_main_size) = automatic_main_size
                        && (items[index].main_size(axes) - FlexMainSize::new(automatic_main_size))
                            .abs()
                            > 0.01
                    {
                        items[index].set_main_size(axes, FlexMainSize::new(automatic_main_size));
                        changed = true;
                    }
                }
            }

            if remeasure_kind == FlexLineCrossRemeasureKind::ColumnShrinkToFit {
                // Preserve intrinsic min/max contributions for future flex
                // calculations, but record the fit-content used width for
                // replay and line metadata. `estimate.width` is the used
                // content-box metric; `content_width` remains max-content.
                remeasured.width = cross_resolution.used_content_size(border_cross_size);
            }

            if (items[index].cross_size(axes) - FlexCrossSize::new(border_cross_size.points()))
                .abs()
                > 0.01
            {
                items[index].set_cross_size(axes, FlexCrossSize::new(border_cross_size.points()));
                changed = true;
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
                update_flex_item_estimate_cross_axis(
                    &mut estimates[index],
                    remeasured,
                    physical_direction,
                );
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
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    let mut changed = false;
    for ((item, estimate), child) in items.iter_mut().zip(estimates).zip(children) {
        let child_style = &child.style;
        let cross_size_is_auto = if physical_direction.is_row_axis() {
            child_style.box_values.height.is_auto()
        } else {
            child_style.box_values.width.is_auto()
        };
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
    /// The final flex-line cross-size used by wrapped-column fit-content
    /// sizing. This remains separate from `line_slot` for callers that do
    /// not need an explicit fit-content constraint.
    fit_content_available_slot: Option<FlexCrossSize>,
    margin_size: FlexCrossLength,
    non_content_size: NonContentLength,
}

impl FlexItemLineCrossSizeResolution {
    fn for_item(
        child_style: &ComputedStyle,
        physical_direction: FlexDirection,
        line_slot: FlexCrossSize,
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
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
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
        let Some(ratio) = child_style
            .aspect_ratio
            .preferred_ratio(child.is_replaced_element(), estimate.preferred_aspect_ratio)
        else {
            continue;
        };
        let borders = used_border_widths(child_style);
        let (main_non_content, cross_non_content) = if physical_direction.is_row_axis() {
            (
                child_style.padding.left + child_style.padding.right + borders.left + borders.right,
                child_style.padding.top + child_style.padding.bottom + borders.top + borders.bottom,
            )
        } else {
            (
                child_style.padding.top + child_style.padding.bottom + borders.top + borders.bottom,
                child_style.padding.left + child_style.padding.right + borders.left + borders.right,
            )
        };
        let main_size = item.main_size(axes).points();
        let main_content_size = (main_size - main_non_content).max(0.0);
        let mut cross_content_size = if child_style.aspect_ratio.uses_content_box_for_non_replaced()
            || child_style.box_sizing == BoxSizing::ContentBox
        {
            if physical_direction.is_row_axis() {
                main_content_size / ratio
            } else {
                main_content_size * ratio
            }
        } else if physical_direction.is_row_axis() {
            (main_size / ratio - cross_non_content).max(0.0)
        } else {
            (main_size * ratio - cross_non_content).max(0.0)
        };
        let percentage_basis = available.width.content_box_length();
        let (min_cross, max_cross) = if physical_direction.is_row_axis() {
            (
                used_min_height(child_style, PercentageBasis::definite(percentage_basis))
                    .map(SemanticLengthExt::points),
                used_max_height(child_style, PercentageBasis::definite(percentage_basis))
                    .map(SemanticLengthExt::points),
            )
        } else {
            (
                used_min_width(child_style, PercentageBasis::definite(percentage_basis))
                    .map(SemanticLengthExt::points),
                used_max_width(child_style, PercentageBasis::definite(percentage_basis))
                    .map(SemanticLengthExt::points),
            )
        };
        if let Some(min_cross) = min_cross {
            cross_content_size = cross_content_size.max(min_cross);
        }
        if let Some(max_cross) = max_cross {
            cross_content_size = cross_content_size.min(max_cross);
        }
        let border_cross_size = (cross_content_size + cross_non_content).max(0.0);
        if (item.cross_size(axes) - FlexCrossSize::new(border_cross_size)).abs() > 0.01 {
            item.set_cross_size(axes, FlexCrossSize::new(border_cross_size));
            changed = true;
        }
        if physical_direction.is_row_axis() {
            estimate.height = content_box_pt(cross_content_size);
            estimate.content_height = content_box_pt(cross_content_size);
        } else {
            estimate.width = content_box_pt(cross_content_size);
            estimate.content_width = content_box_pt(cross_content_size);
        }
    }
    changed
}

pub(in crate::layout::flex) fn flex_item_line_cross_remeasurement_kind(
    child_style: &ComputedStyle,
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
    let cross_size_is_auto = if physical_direction.is_row_axis() {
        child_style.box_values.height.is_auto()
    } else {
        child_style.box_values.width.is_auto()
    };
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
        estimate.set_fragmentable_overflow_height(remeasured.fragmentable_overflow_height);
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
    fn wrapped_column_fit_content_keeps_a_non_stretched_item_within_container_cross_size() {
        let child_style = ComputedStyle::initial();
        let resolution = FlexItemLineCrossSizeResolution::for_item(
            &child_style,
            FlexDirection::Column,
            // Model the provisional max-content line Taffy can expose before
            // Flex reconciliation. The definite container cross size remains
            // the fit-content constraint.
            FlexCrossSize::new(300.0),
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
    fn horizontal_column_synthesizes_a_vertical_inline_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0],
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
            Some(FlexVerticalBaselineOffset::new(10.0)),
        );
    }

    #[test]
    fn horizontal_column_exports_its_first_item_main_axis_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0],
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
        estimate.baselines.vertical.first = Some(FlexVerticalBaselineOffset::new(5.0));
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
            Some(FlexVerticalBaselineOffset::new(8.0)),
        );
    }

    #[test]
    fn horizontal_column_synthesizes_a_vertical_export_baseline() {
        let line = FlexLineLayout {
            item_indices: vec![0],
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
            Some(FlexVerticalBaselineOffset::new(13.0)),
        );
    }

    #[test]
    fn horizontal_column_reverse_uses_the_startmost_ordered_item() {
        let line = FlexLineLayout {
            item_indices: vec![0, 1],
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
        first.baselines.vertical.first = Some(FlexVerticalBaselineOffset::new(2.0));
        let mut second = first;
        second.baselines.vertical.first = Some(FlexVerticalBaselineOffset::new(5.0));
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
            Some(FlexVerticalBaselineOffset::new(5.0)),
        );
    }

    #[test]
    fn row_export_falls_back_to_the_first_item_when_first_line_has_no_set() {
        let lines = vec![
            FlexLineLayout {
                item_indices: vec![0, 1],
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
        estimates[0].baselines.vertical.first = Some(FlexVerticalBaselineOffset::new(4.0));
        estimates[3].baselines.vertical.last = Some(FlexVerticalBaselineOffset::new(8.0));
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
            Some(FlexVerticalBaselineOffset::new(4.0)),
        );
        assert_eq!(
            exported.vertical.last,
            Some(FlexVerticalBaselineOffset::new(18.0)),
        );
    }
}
