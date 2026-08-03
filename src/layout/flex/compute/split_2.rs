use super::*;

/// Corrects Taffy's placeholder placement for `self-start` and `self-end`.
///
/// CSS Box Alignment defines these keywords from the alignment subject's own
/// writing mode, while flex cross-axis placement aligns the subject within the
/// current flex line. Taffy exposes only the container-axis keyword, so this
/// pass keeps Taffy responsible for sizing and line construction and adjusts
/// only the final cross-axis offset for values that need subject-axis mapping:
/// <https://www.w3.org/TR/css-align-3/#self-position> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>.
pub(in crate::layout::flex) fn apply_subject_axis_self_alignment_offsets(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    lines: &[FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    if !children.iter().any(|child| {
        matches!(
            effective_align_self(&child.style, container_style).keyword,
            SelfAlignmentKeyword::SelfStart | SelfAlignmentKeyword::SelfEnd
        )
    }) {
        return;
    }

    for line in lines {
        for &index in &line.item_indices {
            let child_style = &children[index].style;
            let alignment = effective_align_self(child_style, container_style);
            let subject_side = match alignment.keyword {
                SelfAlignmentKeyword::SelfStart => {
                    child_self_start_side(child_style, container_style)
                }
                SelfAlignmentKeyword::SelfEnd => child_self_end_side(child_style, container_style),
                _ => continue,
            };
            if flex_item_has_auto_cross_margin(child_style, physical_direction) {
                continue;
            }
            let outer_size = item_outer_cross_size(&items[index], child_style, physical_direction);
            let target_side = if alignment.safety == AlignmentSafety::Safe
                && (line.cross_size() - outer_size).is_negative()
            {
                flex_cross_start_side(container_style)
            } else {
                subject_side
            };
            align_item_cross_side(
                &mut items[index],
                child_style,
                physical_direction,
                line,
                target_side,
            );
        }
    }
}

/// Inputs that define the stable flex-line topology and cross-axis geometry.
///
/// Grouping these independent container metrics prevents callers from mixing
/// a main-axis wrapping constraint with an unrelated cross-size slot.
pub(in crate::layout::flex) struct FlexLineCollectionContext<'a> {
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) cross_constraint: FlexLineCrossConstraint,
    pub(in crate::layout::flex) hypothetical_outer_main_sizes: &'a [FlexMainSize],
    pub(in crate::layout::flex) container_main_size: Option<FlexMainSize>,
    pub(in crate::layout::flex) main_gap: FlexMainSize,
}

/// The source of a flex line's available cross size.
///
/// A numeric layout constraint does not itself make the container's cross
/// size definite. In particular, a page fragmentainer can cap an automatic
/// row's physical height while the row must still derive its content-based
/// cross size from its hypothetical item contributions. Keeping that state at
/// the line-collection boundary prevents a provisional Taffy root rectangle
/// from being treated as Flexbox's definite single-line slot:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-item> and
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-align>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::flex) enum FlexLineCrossConstraint {
    DefiniteInnerSize(FlexCrossSize),
    ContentBased,
}

impl FlexLineCrossConstraint {
    /// Construct the line constraint from the container's resolved CSS cross
    /// size. A numeric available-space limit is insufficient: an automatic
    /// row within a definite fragmentainer remains content-based.
    pub(in crate::layout::flex) fn from_container(
        container_style: &ComputedStyle,
        available: FlexAvailableSpace,
        physical_direction: FlexDirection,
        used_inner_size: FlexCrossSize,
    ) -> Self {
        // This typed content-based path currently implements physical
        // horizontal rows only. Preserve the established Taffy reconciliation
        // for column and non-horizontal writing modes until their logical
        // cross-size constraints receive the same representation.
        if !physical_direction.is_row_axis()
            || container_style.writing_mode != WritingMode::HorizontalTb
        {
            return Self::DefiniteInnerSize(used_inner_size);
        }
        let used_cross_size = if physical_direction.is_row_axis() {
            used_length_percentage_or_auto_with_basis(
                container_style.box_values.height.value().clone(),
                available.height_basis,
            )
        } else {
            used_length_percentage_or_auto_with_basis(
                container_style.box_values.width.clone(),
                available.width_basis,
            )
        };
        if used_cross_size.is_some() {
            Self::DefiniteInnerSize(used_inner_size)
        } else {
            Self::ContentBased
        }
    }

    fn definite_inner_size(self) -> Option<FlexCrossSize> {
        match self {
            Self::DefiniteInnerSize(size) => Some(size),
            Self::ContentBased => None,
        }
    }
}

pub(in crate::layout::flex) fn flex_lines_from_items(
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    estimates: &[FlexItemEstimate],
    context: FlexLineCollectionContext<'_>,
) -> Vec<FlexLineLayout> {
    let topology = collect_flex_line_topology(
        items.len(),
        context.container_style.flex_wrap,
        context.hypothetical_outer_main_sizes,
        context.container_main_size,
        context.main_gap,
    );
    let mut lines = topology
        .into_iter()
        .map(|topology| {
            flex_line_layout_from_topology(topology, items, children, context.physical_direction)
        })
        .collect::<Vec<_>>();
    refresh_flex_line_metadata(
        &mut lines,
        items,
        estimates,
        children,
        context.container_style,
        context.physical_direction,
        context.cross_constraint,
    );
    lines
}

/// Collect immutable ordinary-wrap line membership before item rectangles are
/// adjusted by baseline alignment, stretch, or fragmentation.
///
/// The input sequence is already order-modified by the flex adapter.  A
/// wrapping container with an indefinite main size has a single available
/// line; otherwise this mirrors Flexbox's consecutive hypothetical-size line
/// collection, including the required oversized-first-item behavior:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-item>.
pub(in crate::layout::flex) fn collect_flex_line_topology(
    item_count: usize,
    flex_wrap: FlexWrap,
    hypothetical_outer_main_sizes: &[FlexMainSize],
    container_main_size: Option<FlexMainSize>,
    main_gap: FlexMainSize,
) -> Vec<FlexLineTopology> {
    if item_count == 0 {
        return Vec::new();
    }
    if flex_wrap == FlexWrap::NoWrap || container_main_size.is_none() {
        return vec![FlexLineTopology {
            item_indices: (0..item_count).collect(),
            source_start: 0,
            source_end: item_count,
        }];
    }

    let available_main_size = container_main_size.expect("checked definite main size");
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut current_size = FlexMainSize::new(0.0);
    let tolerance = FlexMainSize::new(0.01);
    for index in 0..item_count {
        let item_size = hypothetical_outer_main_sizes
            .get(index)
            .copied()
            .unwrap_or_else(|| FlexMainSize::new(0.0));
        let candidate = if current.is_empty() {
            item_size
        } else {
            current_size + main_gap + item_size
        };
        if !current.is_empty() && candidate > available_main_size + tolerance {
            let source_start = *current.first().expect("non-empty line");
            let source_end = current.last().expect("non-empty line") + 1;
            lines.push(FlexLineTopology {
                item_indices: std::mem::take(&mut current),
                source_start,
                source_end,
            });
            current_size = item_size;
        } else {
            current_size = candidate;
        }
        current.push(index);
    }
    let source_start = *current.first().expect("items produce a final line");
    let source_end = current.last().expect("items produce a final line") + 1;
    lines.push(FlexLineTopology {
        item_indices: current,
        source_start,
        source_end,
    });
    lines
}

/// Materialize one stable line record from its canonical membership.
///
/// Taffy supplies initial coordinates, but only to seed the line's slot and
/// item bounds.  It never determines membership after this boundary.
fn flex_line_layout_from_topology(
    topology: FlexLineTopology,
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    physical_direction: FlexDirection,
) -> FlexLineLayout {
    let mut cross_bounds: Option<(FlexCrossOffset, FlexCrossOffset)> = None;
    let mut main_bounds: Option<(FlexMainOffset, FlexMainOffset)> = None;
    for &index in &topology.item_indices {
        let cross =
            item_outer_cross_bounds(&items[index], &children[index].style, physical_direction);
        let main =
            item_outer_main_bounds(&items[index], &children[index].style, physical_direction);
        cross_bounds = Some(match cross_bounds {
            Some((start, end)) => (start.min(cross.0), end.max(cross.1)),
            None => cross,
        });
        main_bounds = Some(match main_bounds {
            Some((start, end)) => (start.min(main.0), end.max(main.1)),
            None => main,
        });
    }
    let (cross_start, cross_end) = cross_bounds.expect("line topology is non-empty");
    let (main_start, main_end) = main_bounds.expect("line topology is non-empty");
    FlexLineLayout {
        item_indices: topology.item_indices,
        source_start: topology.source_start,
        source_end: topology.source_end,
        main_start,
        main_end,
        cross_start,
        cross_end,
        first_baseline: None,
        last_baseline: None,
        collapsed_struts: Vec::new(),
    }
}

pub(in crate::layout::flex) fn refresh_flex_line_cross_bounds(
    lines: &mut [FlexLineLayout],
    items: &[FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    cross_constraint: FlexLineCrossConstraint,
) {
    for line in &mut *lines {
        if container_style.flex_wrap == FlexWrap::NoWrap {
            if let Some(container_cross_size) = cross_constraint.definite_inner_size() {
                line.cross_start = FlexCrossOffset::new(0.0);
                line.cross_end = FlexCrossOffset::new(0.0) + container_cross_size;
            }
            if let Some(main_extent) = flex_items_main_extent(items, children, physical_direction) {
                line.main_start = main_extent.0;
                line.main_end = main_extent.1;
            }
            line.cross_end = line
                .cross_end
                .max(line.cross_start + line.largest_collapsed_strut());
            continue;
        }
        if line.item_indices.is_empty() {
            line.cross_end = line
                .cross_end
                .max(line.cross_start + line.largest_collapsed_strut());
            continue;
        }
        let mut main_bounds: Option<(FlexMainOffset, FlexMainOffset)> = None;
        for &index in &line.item_indices {
            let (item_main_start, item_main_end) =
                item_outer_main_bounds(&items[index], &children[index].style, physical_direction);
            main_bounds = Some(match main_bounds {
                Some((main_start, main_end)) => {
                    (main_start.min(item_main_start), main_end.max(item_main_end))
                }
                None => (item_main_start, item_main_end),
            });
        }
        // A wrapped line's cross slot was established from its canonical
        // membership before post-Taffy baseline, stretch, and fragmentation
        // reconciliation.  Those passes are permitted to move or resize an
        // item inside the slot, but must not reconstruct the line from its
        // final margin-box overlap.  Retain only collapsed-item struts here;
        // they are an explicit Flexbox cross-size input rather than observed
        // descendant geometry:
        // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>.
        line.cross_end = line
            .cross_end
            .max(line.cross_start + line.largest_collapsed_strut());
        let (main_start, main_end) = main_bounds.expect("non-empty flex line has item bounds");
        line.main_start = main_start;
        line.main_end = main_end;
    }
}

pub(in crate::layout::flex) fn refresh_flex_line_metadata(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    cross_constraint: FlexLineCrossConstraint,
) {
    refresh_flex_line_cross_bounds(
        lines,
        items,
        children,
        container_style,
        physical_direction,
        cross_constraint,
    );
    resolve_flex_line_cross_sizes(
        lines,
        items,
        estimates,
        children,
        container_style,
        physical_direction,
        cross_constraint,
    );
    if let Some(container_cross_size) = cross_constraint.definite_inner_size() {
        stretch_wrapped_flex_lines_to_container_cross_size(
            lines,
            items,
            container_style,
            physical_direction,
            container_cross_size,
        );
    }
    refresh_flex_line_baselines(
        lines,
        items,
        estimates,
        children,
        container_style,
        physical_direction,
    );
}

/// Distribute positive cross-axis free space equally among wrapped flex lines.
///
/// A single-line wrapped flex container's line fills its inner cross size;
/// multiple wrapped lines receive equal additional space for `normal` and
/// `stretch` alignment.  This happens after the line's hypothetical cross
/// size is known and before stretched items are remeasured against that line
/// slot:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-line-break> and
/// <https://www.w3.org/TR/css-align-3/#distribution-flex>.
fn stretch_wrapped_flex_lines_to_container_cross_size(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
) {
    if lines.is_empty()
        || container_style.flex_wrap == FlexWrap::NoWrap
        || !matches!(
            container_style.align_content.keyword,
            ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch
        )
    {
        return;
    }

    let PhysicalFlexGaps {
        horizontal: physical_gap_width,
        vertical: physical_gap_height,
    } = physical_flex_gaps(container_style);
    let cross_gap = flex_cross_gap_size(used_flex_gap_with_basis::<FlexAvailableSizeSource>(
        if physical_direction.is_row_axis() {
            physical_gap_height
        } else {
            physical_gap_width
        },
        PercentageBasis::<ContentBoxLength, FlexAvailableSizeSource>::indefinite(),
    ));
    let occupied_cross_size = lines
        .iter()
        .fold(FlexCrossSize::new(0.0), |sum, line| sum + line.cross_size())
        + FlexCrossSize::new(cross_gap.points() * lines.len().saturating_sub(1) as f32);
    let free_space = container_cross_size - occupied_cross_size;
    if free_space.is_non_positive() {
        return;
    }

    let extra_per_line = free_space
        .divide(std::num::NonZeroUsize::new(lines.len()).expect("non-empty flex line collection"));
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    let mut physical_order = (0..lines.len()).collect::<Vec<_>>();
    physical_order.sort_by(|&left, &right| {
        lines[left]
            .cross_start
            .partial_cmp(&lines[right].cross_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if flex_cross_start_side(container_style).is_start_edge() {
        let mut next_start = FlexCrossOffset::new(0.0);
        for line_index in physical_order {
            let line = &mut lines[line_index];
            let delta = next_start - line.cross_start;
            if delta.abs() > 0.01 {
                for &item_index in &line.item_indices {
                    items[item_index].translate_cross(axes, delta);
                }
            }
            let stretched_size = line.cross_size() + extra_per_line;
            line.cross_start = next_start;
            line.cross_end = next_start + stretched_size;
            next_start = line.cross_end + cross_gap;
        }
    } else {
        let mut next_end = FlexCrossOffset::new(container_cross_size.points());
        for line_index in physical_order.into_iter().rev() {
            let line = &mut lines[line_index];
            let stretched_size = line.cross_size() + extra_per_line;
            let target_start = next_end - stretched_size;
            let delta = target_start - line.cross_start;
            if delta.abs() > 0.01 {
                for &item_index in &line.item_indices {
                    items[item_index].translate_cross(axes, delta);
                }
            }
            line.cross_start = target_start;
            line.cross_end = next_end;
            next_end = line.cross_start - cross_gap;
        }
    }
}

/// Restore distributed `align-content` offsets after final flex-line sizing.
///
/// Taffy correctly packs the initial line boxes, but Quire subsequently
/// rebuilds their cross sizes from the Flexbox line-sizing inputs. That
/// reconstruction deliberately makes adjacent slots so that later stretch
/// and fragmentation measurements have stable inputs. It must then restore
/// the distributed free space before the source coordinates are captured for
/// fragment replay:
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
pub(in crate::layout::flex) fn restore_distributed_flex_line_offsets(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
    line_cross_gap: FlexCrossSize,
) -> bool {
    if lines.is_empty() || container_style.flex_wrap == FlexWrap::NoWrap {
        return false;
    }

    let alignment = container_style.align_content;
    if !matches!(
        alignment.keyword,
        ContentAlignmentKeyword::SpaceBetween
            | ContentAlignmentKeyword::SpaceAround
            | ContentAlignmentKeyword::SpaceEvenly
    ) {
        return false;
    }

    let line_count = lines.len();
    let occupied = lines
        .iter()
        .fold(FlexCrossSize::new(0.0), |sum, line| sum + line.cross_size())
        + line_cross_gap.scale(line_count.saturating_sub(1) as f32);
    let free_space = container_cross_size - occupied;
    let positive_free_space = FlexCrossLength::new(free_space.points().max(0.0));

    // CSS Align defines the distribution fallbacks used when there is no
    // positive free space: `space-between` falls back to flex-start, while
    // `space-around` and `space-evenly` fall back to center.
    // <https://www.w3.org/TR/css-align-3/#distribution-flex>
    let (initial, distributed_between) = match alignment.keyword {
        ContentAlignmentKeyword::SpaceBetween if line_count > 1 && free_space.is_positive() => (
            FlexCrossLength::new(0.0),
            positive_free_space.divide(
                std::num::NonZeroUsize::new(line_count - 1)
                    .expect("multiple flex lines leave a non-zero divisor"),
            ),
        ),
        ContentAlignmentKeyword::SpaceAround if free_space.is_positive() => {
            let between = positive_free_space.divide(
                std::num::NonZeroUsize::new(line_count).expect("non-empty flex line collection"),
            );
            (between.half(), between)
        }
        ContentAlignmentKeyword::SpaceEvenly if free_space.is_positive() => {
            let spacing = positive_free_space.divide(
                std::num::NonZeroUsize::new(line_count + 1)
                    .expect("line count plus one is non-zero"),
            );
            (spacing, spacing)
        }
        ContentAlignmentKeyword::SpaceAround | ContentAlignmentKeyword::SpaceEvenly => {
            (free_space.half(), FlexCrossLength::new(0.0))
        }
        ContentAlignmentKeyword::SpaceBetween => {
            (FlexCrossLength::new(0.0), FlexCrossLength::new(0.0))
        }
        _ => unreachable!("non-distributed alignment returned above"),
    };

    let (initial, distributed_between) =
        if free_space.is_negative() && alignment.safety == AlignmentSafety::Safe {
            (FlexCrossLength::new(0.0), FlexCrossLength::new(0.0))
        } else {
            (initial, distributed_between)
        };

    let cross_origin = FlexCrossOffset::new(0.0);
    if flex_cross_start_side(container_style).is_start_edge() {
        let mut next_start = cross_origin + initial;
        for line in lines {
            let delta = next_start - line.cross_start;
            shift_flex_line_cross_axis(line, items, physical_direction, delta);
            next_start = line.cross_end + line_cross_gap + distributed_between;
        }
    } else {
        let mut next_end = cross_origin + container_cross_size - initial;
        for line in lines {
            let target_start = next_end - line.cross_size();
            let delta = target_start - line.cross_start;
            shift_flex_line_cross_axis(line, items, physical_direction, delta);
            next_end = line.cross_start - line_cross_gap - distributed_between;
        }
    }
    true
}

/// Recompute a line's exported baselines without changing its resolved cross
/// slots.  Once baseline content alignment has translated complete lines,
/// rerunning cross-size resolution would repack the overlapping line boxes
/// and undo that alignment.
pub(in crate::layout::flex) fn refresh_flex_line_baselines(
    lines: &mut [FlexLineLayout],
    items: &[FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
) {
    for line in lines {
        line.first_baseline = flex_line_baseline(
            &line.item_indices,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::First,
            physical_direction,
        );
        line.last_baseline = flex_line_baseline(
            &line.item_indices,
            items,
            estimates,
            children,
            container_style,
            FlexBaselineSet::Last,
            physical_direction,
        );
    }
}

/// Resolve the used cross-size of canonical wrapped flex lines.
///
/// Taffy's post-layout item rectangles can include `stretch`, but CSS Flexbox
/// calculates each line before stretching its items. Baseline participants
/// instead contribute the greatest baseline-to-outer-cross-start plus the
/// greatest baseline-to-outer-cross-end distance. All other items contribute
/// their hypothetical outer cross size. Keep this rule separate from the
/// final item-bound refresh so reconciliation cannot feed stretched geometry
/// back into the line-sizing step:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>.
fn resolve_flex_line_cross_sizes(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    cross_constraint: FlexLineCrossConstraint,
) {
    if lines.is_empty() {
        return;
    }
    // A single-line flex container uses its inner cross size only when that
    // size is definite. An automatic row instead derives its line from the
    // remeasured hypothetical item contributions below.
    // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
    if container_style.flex_wrap == FlexWrap::NoWrap
        && cross_constraint.definite_inner_size().is_some()
    {
        return;
    }

    let baseline_line_axis = flex_baseline_line_axis(container_style);
    // Flex line sizing uses the baseline alignment edge inside a line. This
    // is deliberately not `flex_cross_start_side`: `wrap-reverse` reverses
    // line stacking, not the first/last baseline edge of each line.
    // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
    let cross_start_side = flex_baseline_alignment_side(container_style, FlexBaselineSet::First);
    let cross_end_side = flex_baseline_alignment_side(container_style, FlexBaselineSet::Last);
    let resolved_sizes = lines
        .iter()
        .map(|line| {
            let mut largest_baseline_start = FlexCrossSize::new(0.0);
            let mut largest_baseline_end = FlexCrossSize::new(0.0);
            let mut has_baseline_participant = false;
            let mut largest_other = FlexCrossSize::new(0.0);
            for &index in &line.item_indices {
                let child_style = &children[index].style;
                let is_baseline_participant = flex_baseline_set(child_style, container_style)
                    .is_some()
                    && !flex_item_has_auto_cross_margin(child_style, physical_direction)
                    && flex_item_baseline_axis_is_parallel_to_main_axis(
                        child_style,
                        physical_direction,
                    );
                if is_baseline_participant {
                    has_baseline_participant = true;
                    let baseline_set = flex_baseline_set(child_style, container_style)
                        .expect("checked baseline participant");
                    let source = flex_item_baseline_source(
                        &estimates[index],
                        baseline_set,
                        baseline_line_axis,
                    );
                    largest_baseline_start =
                        largest_baseline_start.max(item_baseline_distance_to_cross_side(
                            &items[index],
                            &estimates[index],
                            child_style,
                            container_style,
                            baseline_set,
                            physical_direction,
                            source,
                            cross_start_side,
                        ));
                    largest_baseline_end =
                        largest_baseline_end.max(item_baseline_distance_to_cross_side(
                            &items[index],
                            &estimates[index],
                            child_style,
                            container_style,
                            baseline_set,
                            physical_direction,
                            source,
                            cross_end_side,
                        ));
                } else {
                    largest_other = largest_other.max(flex_cross_size_from_layout_extent(
                        estimated_outer_cross_size(
                            child_style,
                            estimates[index],
                            physical_direction,
                        ),
                    ));
                }
            }
            let baseline_size = if has_baseline_participant {
                largest_baseline_start + largest_baseline_end
            } else {
                FlexCrossSize::new(0.0)
            };
            baseline_size
                .max(largest_other)
                .max(line.largest_collapsed_strut())
        })
        .collect::<Vec<_>>();
    let PhysicalFlexGaps {
        horizontal: physical_gap_width,
        vertical: physical_gap_height,
    } = physical_flex_gaps(container_style);
    let cross_gap = flex_cross_gap_size(used_flex_gap_with_basis::<FlexAvailableSizeSource>(
        if physical_direction.is_row_axis() {
            physical_gap_height
        } else {
            physical_gap_width
        },
        PercentageBasis::<ContentBoxLength, FlexAvailableSizeSource>::indefinite(),
    ));
    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    let mut physical_order = (0..lines.len()).collect::<Vec<_>>();
    physical_order.sort_by(|&left, &right| {
        lines[left]
            .cross_start
            .partial_cmp(&lines[right].cross_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if cross_start_side.is_start_edge() {
        let mut next_start = lines[physical_order[0]].cross_start;
        for line_index in physical_order {
            let line = &mut lines[line_index];
            let delta = next_start - line.cross_start;
            if delta.abs() > 0.01 {
                line.cross_start = line.cross_start + delta;
                line.cross_end = line.cross_end + delta;
                for &item_index in &line.item_indices {
                    items[item_index].translate_cross(axes, delta);
                }
            }
            line.cross_end = line.cross_start + resolved_sizes[line_index];
            next_start = line.cross_end + cross_gap;
        }
    } else {
        let mut next_end = lines[*physical_order.last().expect("non-empty lines")].cross_end;
        for line_index in physical_order.into_iter().rev() {
            let line = &mut lines[line_index];
            let target_start = next_end - resolved_sizes[line_index];
            let delta = target_start - line.cross_start;
            if delta.abs() > 0.01 {
                line.cross_start = line.cross_start + delta;
                line.cross_end = line.cross_end + delta;
                for &item_index in &line.item_indices {
                    items[item_index].translate_cross(axes, delta);
                }
            }
            line.cross_start = target_start;
            line.cross_end = next_end;
            next_end = line.cross_start - cross_gap;
        }
    }
}

pub(in crate::layout::flex) fn item_outer_cross_bounds(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> (FlexCrossOffset, FlexCrossOffset) {
    let (start, end) = item.outer_cross_bounds(
        FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction)),
        style,
    );
    (start, end)
}

pub(in crate::layout::flex) fn item_outer_cross_size(
    item: &FlexItemLayout,
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexCrossSize {
    let (cross_start, cross_end) = item_outer_cross_bounds(item, style, physical_direction);
    (cross_end - cross_start).non_negative_size()
}

pub(in crate::layout::flex) fn estimated_outer_cross_size(
    style: &ComputedStyle,
    estimate: FlexItemEstimate,
    physical_direction: FlexDirection,
) -> LayoutLength {
    let borders = used_border_widths(style);
    layout_pt(
        if physical_direction.is_row_axis() {
            estimate.height.points()
                + style.padding.top
                + style.padding.bottom
                + borders.top
                + borders.bottom
                + style.margin.top
                + style.margin.bottom
        } else {
            estimate.width.points()
                + style.padding.left
                + style.padding.right
                + borders.left
                + borders.right
                + style.margin.left
                + style.margin.right
        }
        .max(0.0),
    )
}

pub(in crate::layout::flex) fn collapsed_struts_from_visible_layout(
    children: &[StyledChild<'_>],
    style: &ComputedStyle,
    visible_layout: &FlexLayout,
) -> Vec<FlexCollapsedStrut> {
    let physical_direction = physical_flex_direction(style);
    children
        .iter()
        .enumerate()
        .filter(|(_, child)| flex_item_is_collapsed(&child.style))
        .map(|(item_index, child)| {
            let item = &visible_layout.items[item_index];
            let line = visible_layout
                .lines
                .iter()
                .find(|line| line.item_indices.contains(&item_index));
            FlexCollapsedStrut {
                item_index,
                cross_size: item_outer_cross_size(item, &child.style, physical_direction),
                source_start: line.map(|line| line.source_start).unwrap_or(item_index),
                source_end: line.map(|line| line.source_end).unwrap_or(item_index + 1),
            }
        })
        .collect()
}

pub(in crate::layout::flex) fn attach_collapsed_struts_to_active_lines(
    lines: &mut Vec<FlexLineLayout>,
    source_indices: &[usize],
    collapsed_struts: &[FlexCollapsedStrut],
) {
    if collapsed_struts.is_empty() {
        return;
    }

    for line in lines.iter_mut() {
        let mut source_start = usize::MAX;
        let mut source_end = 0usize;
        for &active_index in &line.item_indices {
            let Some(&source_index) = source_indices.get(active_index) else {
                continue;
            };
            source_start = source_start.min(source_index);
            source_end = source_end.max(source_index + 1);
        }
        if source_start != usize::MAX {
            line.source_start = source_start;
            line.source_end = source_end;
        }
    }

    for strut in collapsed_struts {
        if lines.is_empty() {
            lines.push(FlexLineLayout {
                item_indices: Vec::new(),
                source_start: strut.item_index,
                source_end: strut.item_index + 1,
                main_start: FlexMainOffset::new(0.0),
                main_end: FlexMainOffset::new(0.0),
                cross_start: FlexCrossOffset::new(0.0),
                cross_end: FlexCrossOffset::new(0.0) + strut.cross_size,
                first_baseline: None,
                last_baseline: None,
                collapsed_struts: vec![strut.clone()],
            });
            continue;
        }

        let target_index = lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| collapsed_strut_line_overlap(strut, line))
            .filter(|(_, line)| collapsed_strut_line_overlap(strut, line) > 0)
            .map(|(index, _)| index)
            .or_else(|| {
                lines
                    .iter()
                    .position(|line| strut.item_index < line.source_end)
            })
            .unwrap_or(lines.len() - 1);
        let line = &mut lines[target_index];
        line.source_start = line.source_start.min(strut.source_start);
        line.source_end = line.source_end.max(strut.source_end);
        line.collapsed_struts.push(strut.clone());
        line.cross_end = line
            .cross_end
            .max(line.cross_start + line.largest_collapsed_strut());
    }
}

pub(in crate::layout::flex) fn repack_lines_after_collapsed_struts(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    physical_direction: FlexDirection,
) {
    if lines.len() <= 1 || !lines.iter().any(|line| !line.collapsed_struts.is_empty()) {
        return;
    }

    let axes = FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction));
    let mut line_order = (0..lines.len()).collect::<Vec<_>>();
    line_order.sort_by(|&a, &b| {
        lines[a]
            .cross_start
            .partial_cmp(&lines[b].cross_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut next_cross_start = lines[line_order[0]].cross_start;
    for line_index in line_order {
        let delta = next_cross_start - lines[line_index].cross_start;
        if delta.abs() > 0.01 {
            lines[line_index].cross_start = lines[line_index].cross_start + delta;
            lines[line_index].cross_end = lines[line_index].cross_end + delta;
            for &item_index in &lines[line_index].item_indices {
                items[item_index].translate_cross(axes, delta);
            }
        }
        next_cross_start = lines[line_index].cross_end;
    }
}

/// Repartition a balanced flex container's already-sized items across its
/// normal-wrap line count.
///
/// CSS Flexbox Level 2 keeps the line count selected by ordinary wrapping, but
/// chooses a more even distribution of items where each candidate line still
/// fits the available main size. Taffy does not expose this draft algorithm,
/// so Quire preserves its flex sizing pass, changes only line membership, and
/// then reuses the normal main-axis repacking pass:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexBalanceContext<'a> {
    pub(in crate::layout::flex) physical_direction: FlexDirection,
    pub(in crate::layout::flex) requested_line_count: Option<usize>,
    pub(in crate::layout::flex) hypothetical_main_sizes: Option<&'a [FlexMainSize]>,
    pub(in crate::layout::flex) main_gap: FlexMainSize,
    pub(in crate::layout::flex) cross_gap: FlexCrossSize,
    pub(in crate::layout::flex) available_main_size: FlexMainSize,
}

pub(in crate::layout::flex) fn rebalance_flex_line_membership(
    lines: &mut Vec<FlexLineLayout>,
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    context: FlexBalanceContext<'_>,
) -> bool {
    if lines.is_empty() || !context.available_main_size.is_finite() {
        return false;
    }

    let ordered_items = lines
        .iter()
        .flat_map(|line| line.item_indices.iter().cloned())
        .collect::<Vec<_>>();
    if ordered_items.len() < lines.len() {
        return false;
    }

    let outer_main_sizes = ordered_items
        .iter()
        .map(|&index| {
            let item_main_size = context
                .hypothetical_main_sizes
                .and_then(|sizes| sizes.get(index))
                .cloned()
                .unwrap_or_else(|| item_main_size(&items[index], context.physical_direction));
            (item_main_size
                + fixed_main_before_margin(&children[index].style, context.physical_direction)
                + fixed_main_after_margin(&children[index].style, context.physical_direction))
            .non_negative_size()
        })
        .collect::<Vec<_>>();
    let inferred_line_count = balanced_flex_line_count(
        &outer_main_sizes,
        context.main_gap,
        context.available_main_size,
    );
    // `flex-line-count` is an explicit request for the number of balanced
    // lines.  It must therefore be allowed to reduce the provisional number
    // of ordinary-wrap lines as well as extend it.  Without an explicit
    // value, balance retains the ordinary line count (or the minimum count
    // required by oversized hypothetical main sizes).
    // <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
    let requested_line_count = context
        .requested_line_count
        .unwrap_or_else(|| lines.len().max(inferred_line_count))
        .clamp(1, ordered_items.len());
    if requested_line_count < 2 {
        return false;
    }
    while lines.len() < requested_line_count {
        let Some(last_line) = lines.last().cloned() else {
            return false;
        };
        let cross_size = last_line.cross_size();
        let cross_start = last_line.cross_end + context.cross_gap;
        lines.push(FlexLineLayout {
            item_indices: Vec::new(),
            source_start: last_line.source_end,
            source_end: last_line.source_end,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(0.0),
            cross_start,
            cross_end: cross_start + cross_size,
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        });
    }
    lines.truncate(requested_line_count);
    let line_count = lines.len();
    let Some(partitions) = balanced_flex_line_partitions(
        &ordered_items,
        &outer_main_sizes,
        line_count,
        context.main_gap,
        context.available_main_size,
    ) else {
        return false;
    };

    let axes =
        FlexAxes::from_physical_direction(PhysicalFlexDirection::new(context.physical_direction));
    let slots = lines
        .iter()
        .map(|line| (line.cross_start, line.cross_end))
        .collect::<Vec<_>>();
    let mut changed = false;
    for ((line, item_indices), (cross_start, cross_end)) in
        lines.iter_mut().zip(partitions).zip(slots)
    {
        changed |= line.item_indices != item_indices;
        for (position, &item_index) in item_indices.iter().enumerate() {
            let item = &mut items[item_index];
            let delta = cross_start - item.cross_start(axes);
            item.translate_cross(axes, delta);
            // Main-axis repacking sorts by physical position. A member moved
            // from a later normal-wrap line may retain a stale position, so
            // seed the new source-order partition before that shared pass.
            let provisional_main_start = if matches!(
                context.physical_direction,
                FlexDirection::RowReverse | FlexDirection::ColumnReverse
            ) {
                (item_indices.len() - position) as f32
            } else {
                position as f32
            };
            set_item_main_start(
                item,
                context.physical_direction,
                FlexMainOffset::new(provisional_main_start),
            );
        }
        line.source_start = item_indices
            .iter()
            .cloned()
            .min()
            .unwrap_or(line.source_start);
        line.source_end = item_indices
            .iter()
            .cloned()
            .max()
            .map(|index| index + 1)
            .unwrap_or(line.source_end);
        line.item_indices = item_indices;
        line.cross_start = cross_start;
        line.cross_end = cross_end;
    }
    changed
}

/// Returns the minimum balanced line count required by hypothetical outer sizes.
///
/// This mirrors the Level 2 sequence constraints when ordinary wrapping has
/// already folded a zero-sized item into a preceding overflowing item. An
/// overflowing item occupies its own sequence; a following zero-sized item is
/// assigned to the next sequence as required by the balance algorithm:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>.
fn balanced_flex_line_count(
    outer_main_sizes: &[FlexMainSize],
    main_gap: FlexMainSize,
    available_main_size: FlexMainSize,
) -> usize {
    if outer_main_sizes.is_empty() {
        return 0;
    }
    let mut line_count = 1usize;
    let tolerance = FlexMainSize::new(0.01);
    let mut line_extent = outer_main_sizes[0];
    let mut line_overflows = line_extent > available_main_size + tolerance;
    for size in &outer_main_sizes[1..] {
        let candidate = line_extent + main_gap + *size;
        let must_break = candidate > available_main_size + tolerance
            || (line_overflows && *size == FlexMainSize::new(0.0));
        if must_break {
            line_count += 1;
            line_extent = *size;
            line_overflows = line_extent > available_main_size + tolerance;
        } else {
            line_extent = candidate;
            line_overflows = line_extent > available_main_size + tolerance;
        }
    }
    line_count
}

/// Find the source-order partition that minimizes the total squared line error.
///
/// The Level 2 balancing algorithm searches legal line partitions while
/// retaining the normal-wrap line count. Each line error is its hypothetical
/// outer main extent minus the container main size; the selected partition
/// minimizes the sum of squared errors. Automatic margins and flexible lengths
/// are resolved only after this hypothetical-size partition is selected:
/// <https://drafts.csswg.org/css-flexbox-2/#algo-balance>.
fn balanced_flex_line_partitions(
    item_indices: &[usize],
    outer_main_sizes: &[FlexMainSize],
    line_count: usize,
    main_gap: FlexMainSize,
    available_main_size: FlexMainSize,
) -> Option<Vec<Vec<usize>>> {
    let item_count = item_indices.len();
    if line_count == 0 || line_count > item_count || item_count != outer_main_sizes.len() {
        return None;
    }

    let mut prefix = Vec::with_capacity(item_count + 1);
    prefix.push(FlexMainSize::new(0.0));
    for size in outer_main_sizes {
        prefix.push(*prefix.last().expect("prefix starts with zero") + *size);
    }
    let line_extent = |start: usize, end: usize| {
        (prefix[end] - prefix[start]).non_negative_size()
            + main_gap.scale(end.saturating_sub(start + 1) as f32)
    };

    let mut costs = vec![vec![f32::INFINITY; item_count + 1]; line_count + 1];
    let mut predecessors = vec![vec![None; item_count + 1]; line_count + 1];
    costs[0][0] = 0.0;
    for line in 1..=line_count {
        for end in line..=item_count {
            for start in (line - 1)..end {
                let extent = line_extent(start, end);
                // A sequence may overflow only when it contains one item.
                // This keeps a following zero-sized item out of an already
                // overflowing sequence.
                if (end - start > 1 && extent > available_main_size + FlexMainSize::new(0.01))
                    || !costs[line - 1][start].is_finite()
                {
                    continue;
                }
                let error = extent - available_main_size;
                // Dynamic-programming costs are dimensionless ordering
                // scores. The conversion happens only at this algorithmic
                // boundary; all line geometry remains typed above.
                let candidate = costs[line - 1][start] + error.points().powi(2);
                // Prefer the later break for equal errors. During reverse
                // reconstruction, this gives the draft algorithm's start bias:
                // assign as many items as possible to earlier lines.
                if candidate <= costs[line][end] {
                    costs[line][end] = candidate;
                    predecessors[line][end] = Some(start);
                }
            }
        }
    }
    if !costs[line_count][item_count].is_finite() {
        return None;
    }

    let mut partitions = Vec::with_capacity(line_count);
    let mut end = item_count;
    for line in (1..=line_count).rev() {
        let start = predecessors[line][end]?;
        partitions.push(item_indices[start..end].to_vec());
        end = start;
    }
    partitions.reverse();
    Some(partitions)
}

/// Repack flex lines after Quire-side main-size corrections.
///
/// Taffy performs flexible length resolution and `justify-content` packing
/// before Quire applies the final automatic minimum-size guard for edge cases
/// Taffy cannot represent. When that guard changes a main size, CSS Flexbox's
/// main-axis alignment must be recomputed from the corrected outer sizes:
/// <https://www.w3.org/TR/css-flexbox-1/#algo-main-align> and
/// <https://www.w3.org/TR/css-align-3/#distribution-values>.
pub(in crate::layout::flex) fn repack_lines_after_main_size_adjustment(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_main_size: FlexMainSize,
    container_main_percentage_basis: FlexAvailablePercentageBasis,
) {
    if !container_main_size.is_finite() {
        return;
    }

    let PhysicalFlexGaps {
        horizontal: physical_gap_width,
        vertical: physical_gap_height,
    } = physical_flex_gaps(container_style);
    let main_gap = flex_main_gap_size(used_flex_gap_with_basis(
        if physical_direction.is_row_axis() {
            physical_gap_width
        } else {
            physical_gap_height
        },
        container_main_percentage_basis,
    ));

    for line in lines {
        if line.item_indices.is_empty() {
            continue;
        }

        let mut physical_order = line.item_indices.clone();
        physical_order.sort_by(|&left, &right| {
            let left_start =
                item_outer_main_bounds(&items[left], &children[left].style, physical_direction).0;
            let right_start =
                item_outer_main_bounds(&items[right], &children[right].style, physical_direction).0;
            left_start
                .partial_cmp(&right_start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let item_count = physical_order.len();
        let fixed_outer_size =
            physical_order
                .iter()
                .fold(FlexMainLength::new(0.0), |total, &index| {
                    total
                        + (item_main_size(&items[index], physical_direction)
                            + fixed_main_before_margin(&children[index].style, physical_direction)
                            + fixed_main_after_margin(&children[index].style, physical_direction))
                });
        let total_gap = main_gap.scale(item_count.saturating_sub(1) as f32);
        let free_space = container_main_size - fixed_outer_size - total_gap;
        let auto_margin_count = physical_order
            .iter()
            .map(|&index| main_auto_margin_count(&children[index].style, physical_direction))
            .sum::<usize>();
        let auto_margin = if free_space.is_positive() && auto_margin_count > 0 {
            free_space.divide(
                std::num::NonZeroUsize::new(auto_margin_count)
                    .expect("positive auto-margin count is non-zero"),
            )
        } else {
            FlexMainLength::new(0.0)
        };
        let justification = if auto_margin_count > 0 && free_space.is_positive() {
            FlexMainJustificationOffsets {
                initial: FlexMainLength::new(0.0),
                between: FlexMainLength::new(0.0),
            }
        } else {
            justify_content_offsets(
                container_style.justify_content,
                physical_direction,
                free_space,
                item_count,
            )
        };

        let mut cursor = FlexMainOffset::new(0.0) + justification.initial;
        for (position, &item_index) in physical_order.iter().enumerate() {
            cursor = cursor
                + main_before_margin(&children[item_index].style, physical_direction, auto_margin);
            set_item_main_start(&mut items[item_index], physical_direction, cursor);
            cursor = cursor + item_main_size(&items[item_index], physical_direction);
            cursor = cursor
                + main_after_margin(&children[item_index].style, physical_direction, auto_margin);
            if position + 1 < item_count {
                cursor = cursor + main_gap + justification.between;
            }
        }

        if let Some((main_start, main_end)) =
            flex_line_items_main_extent(line, items, children, physical_direction)
        {
            line.main_start = main_start;
            line.main_end = main_end;
        }
    }
}

pub(in crate::layout::flex) fn item_main_size(
    item: &FlexItemLayout,
    physical_direction: FlexDirection,
) -> FlexMainSize {
    item.main_size(FlexAxes::from_physical_direction(
        PhysicalFlexDirection::new(physical_direction),
    ))
}

pub(in crate::layout::flex) fn set_item_main_start(
    item: &mut FlexItemLayout,
    physical_direction: FlexDirection,
    main_start: FlexMainOffset,
) {
    item.set_main_start(
        FlexAxes::from_physical_direction(PhysicalFlexDirection::new(physical_direction)),
        main_start,
    );
}

pub(in crate::layout::flex) fn fixed_main_before_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            FlexMainLength::new(0.0)
        } else {
            FlexMainLength::new(style.margin.left)
        }
    } else if style.box_values.margin.top.is_auto() {
        FlexMainLength::new(0.0)
    } else {
        FlexMainLength::new(style.margin.top)
    }
}

pub(in crate::layout::flex) fn fixed_main_after_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            FlexMainLength::new(0.0)
        } else {
            FlexMainLength::new(style.margin.right)
        }
    } else if style.box_values.margin.bottom.is_auto() {
        FlexMainLength::new(0.0)
    } else {
        FlexMainLength::new(style.margin.bottom)
    }
}

pub(in crate::layout::flex) fn main_before_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: FlexMainLength,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.left.is_auto() {
            auto_margin
        } else {
            FlexMainLength::new(style.margin.left)
        }
    } else if style.box_values.margin.top.is_auto() {
        auto_margin
    } else {
        FlexMainLength::new(style.margin.top)
    }
}

pub(in crate::layout::flex) fn main_after_margin(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
    auto_margin: FlexMainLength,
) -> FlexMainLength {
    if physical_direction.is_row_axis() {
        if style.box_values.margin.right.is_auto() {
            auto_margin
        } else {
            FlexMainLength::new(style.margin.right)
        }
    } else if style.box_values.margin.bottom.is_auto() {
        auto_margin
    } else {
        FlexMainLength::new(style.margin.bottom)
    }
}

pub(in crate::layout::flex) fn main_auto_margin_count(
    style: &ComputedStyle,
    physical_direction: FlexDirection,
) -> usize {
    if physical_direction.is_row_axis() {
        style.box_values.margin.left.is_auto() as usize
            + style.box_values.margin.right.is_auto() as usize
    } else {
        style.box_values.margin.top.is_auto() as usize
            + style.box_values.margin.bottom.is_auto() as usize
    }
}

pub(in crate::layout::flex) fn justify_content_offsets(
    justify_content: JustifyContent,
    physical_direction: FlexDirection,
    free_space: FlexMainLength,
    item_count: usize,
) -> FlexMainJustificationOffsets {
    let keyword = justify_content_fallback_keyword(justify_content, free_space, item_count);
    let reversed = matches!(
        physical_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let first = match keyword {
        ContentAlignmentKeyword::Normal
        | ContentAlignmentKeyword::Stretch
        | ContentAlignmentKeyword::Start => FlexMainLength::new(0.0),
        ContentAlignmentKeyword::FlexStart => {
            if reversed {
                free_space
            } else {
                FlexMainLength::new(0.0)
            }
        }
        ContentAlignmentKeyword::End => free_space,
        ContentAlignmentKeyword::FlexEnd => {
            if reversed {
                FlexMainLength::new(0.0)
            } else {
                free_space
            }
        }
        ContentAlignmentKeyword::Left => FlexMainLength::new(0.0),
        ContentAlignmentKeyword::Right => free_space,
        ContentAlignmentKeyword::Center => free_space.half(),
        ContentAlignmentKeyword::SpaceBetween => FlexMainLength::new(0.0),
        ContentAlignmentKeyword::SpaceAround => {
            if !free_space.is_negative() {
                free_space
                    .divide(std::num::NonZeroUsize::new(item_count).expect("non-zero item count"))
                    .half()
            } else {
                free_space.half()
            }
        }
        ContentAlignmentKeyword::SpaceEvenly => {
            if !free_space.is_negative() {
                free_space.divide(
                    std::num::NonZeroUsize::new(item_count + 1)
                        .expect("item count plus one is non-zero"),
                )
            } else {
                free_space.half()
            }
        }
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            FlexMainLength::new(0.0)
        }
    };
    let positive_free_space = free_space.max(FlexMainLength::new(0.0));
    let between = match keyword {
        ContentAlignmentKeyword::SpaceBetween if item_count > 1 => positive_free_space.divide(
            std::num::NonZeroUsize::new(item_count - 1)
                .expect("more than one item leaves a non-zero divisor"),
        ),
        ContentAlignmentKeyword::SpaceAround if item_count > 0 => positive_free_space
            .divide(std::num::NonZeroUsize::new(item_count).expect("non-zero item count")),
        ContentAlignmentKeyword::SpaceEvenly => positive_free_space.divide(
            std::num::NonZeroUsize::new(item_count + 1).expect("item count plus one is non-zero"),
        ),
        _ => FlexMainLength::new(0.0),
    };
    FlexMainJustificationOffsets {
        initial: first,
        between,
    }
}

/// Main-axis offsets selected by CSS `justify-content` for one flex line.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexMainJustificationOffsets {
    pub(in crate::layout::flex) initial: FlexMainLength,
    pub(in crate::layout::flex) between: FlexMainLength,
}

pub(in crate::layout::flex) fn justify_content_fallback_keyword(
    justify_content: JustifyContent,
    free_space: FlexMainLength,
    item_count: usize,
) -> ContentAlignmentKeyword {
    let mut keyword = justify_content.keyword;
    let mut safe = justify_content.safety == AlignmentSafety::Safe;
    if item_count <= 1 || free_space.is_non_positive() {
        (keyword, safe) = match keyword {
            ContentAlignmentKeyword::Stretch | ContentAlignmentKeyword::SpaceBetween => {
                (ContentAlignmentKeyword::FlexStart, true)
            }
            ContentAlignmentKeyword::SpaceAround | ContentAlignmentKeyword::SpaceEvenly => {
                (ContentAlignmentKeyword::Center, true)
            }
            other => (other, safe),
        };
    }
    if free_space.is_non_positive() && safe {
        ContentAlignmentKeyword::Start
    } else {
        keyword
    }
}

/// Apply CSS Box Alignment baseline content-alignment to wrapped flex lines.
///
/// Taffy's `AlignContentKeyword` has no baseline keywords, so the adapter maps
/// `align-content: baseline` to start packing. CSS Align instead treats flex
/// lines as the alignment subjects and aligns their compatible baseline sets
/// when those sets are available. `FlexCrossOffset` is projected by the
/// physical flex direction, so this also covers a logical row in vertical
/// writing, whose cross axis is physically horizontal:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content>.
pub(in crate::layout::flex) fn apply_baseline_align_content_offsets(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    estimates: &[FlexItemEstimate],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
) -> bool {
    if lines.is_empty() || container_style.flex_wrap == FlexWrap::NoWrap {
        return false;
    }

    let baseline_set = match container_style.align_content.keyword {
        ContentAlignmentKeyword::Baseline => FlexBaselineSet::First,
        ContentAlignmentKeyword::LastBaseline => FlexBaselineSet::Last,
        _ => return false,
    };

    let line_baselines = lines
        .iter()
        .map(|line| {
            flex_line_content_baseline(
                line,
                items,
                estimates,
                children,
                container_style,
                baseline_set,
                physical_direction,
            )
        })
        .collect::<Vec<_>>();
    if line_baselines
        .iter()
        .filter(|baseline| baseline.is_some())
        .count()
        <= 1
    {
        apply_baseline_align_content_fallback_offset(
            items,
            lines,
            children,
            container_style,
            physical_direction,
            container_cross_size,
            baseline_set,
        );
        return true;
    }

    // CSS Flexbox defines line baseline sets for a row main axis. A logical
    // row can project to a physical column in vertical writing, so this must
    // use the authored logical flex direction rather than `physical_direction`.
    if !container_style.flex_direction.is_row_axis() {
        apply_baseline_align_content_fallback_offset(
            items,
            lines,
            children,
            container_style,
            physical_direction,
            container_cross_size,
            baseline_set,
        );
        return true;
    }

    let target_baseline = line_baselines
        .iter()
        .flatten()
        .copied()
        .reduce(FlexCrossOffset::max)
        .expect("more than one baseline participant has a baseline");

    for (line_index, baseline) in line_baselines.into_iter().enumerate() {
        let Some(baseline) = baseline else {
            continue;
        };
        let delta = target_baseline - baseline;
        if delta.abs() <= 0.01 {
            continue;
        }
        shift_flex_line_cross_axis(&mut lines[line_index], items, physical_direction, delta);
    }
    true
}

/// Applies content-alignment fallback when line baselines cannot be shared.
///
/// CSS Align defines first-baseline content alignment fallback as safe logical
/// start, and last-baseline content alignment fallback as safe logical end.
/// The fallback moves the flex-line group in the flex container cross axis:
/// <https://www.w3.org/TR/css-align-3/#baseline-align-content> and
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
pub(in crate::layout::flex) fn apply_baseline_align_content_fallback_offset(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    children: &[StyledChild<'_>],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
    baseline_set: FlexBaselineSet,
) {
    // A stretched single line occupies the whole cross-size, but baseline
    // content-alignment falls back by positioning the alignment subjects, not
    // by treating that stretch slot as an already-positioned subject.
    let Some((group_start, group_end)) =
        flex_line_alignment_subject_cross_bounds(lines, items, children, physical_direction)
    else {
        return;
    };
    let group_size = (group_end - group_start).non_negative_size();
    let target_side = if group_size > container_cross_size {
        baseline_content_alignment_fallback_side(container_style, FlexBaselineSet::First)
    } else {
        baseline_content_alignment_fallback_side(container_style, baseline_set)
    };
    align_flex_line_group_cross_side_from_bounds(
        lines,
        items,
        physical_direction,
        target_side,
        container_cross_size,
        group_start,
        group_end,
    );
}

/// Baseline content-alignment fallbacks use logical start/end. `wrap-reverse`
/// reverses flex-line packing, but does not reverse the fallback defined by
/// CSS Box Alignment.
fn baseline_content_alignment_fallback_side(
    container_style: &ComputedStyle,
    baseline_set: FlexBaselineSet,
) -> PhysicalSide {
    let logical_start = if container_style.flex_wrap.reverses_cross_axis() {
        flex_cross_end_side(container_style)
    } else {
        flex_cross_start_side(container_style)
    };
    match baseline_set {
        FlexBaselineSet::First => logical_start,
        FlexBaselineSet::Last => logical_start.opposite(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_cross_constraint_keeps_an_indefinite_height_content_based() {
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(80.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            // A numeric fragmentainer limit is not a CSS definite height.
            height: Some(PhysicalContentHeight::new(content_box_pt(120.0))),
            height_basis: PercentageBasis::indefinite(),
        };

        assert_eq!(
            FlexLineCrossConstraint::from_container(
                &ComputedStyle::initial(),
                available,
                FlexDirection::Row,
                FlexCrossSize::new(120.0),
            ),
            FlexLineCrossConstraint::ContentBased
        );
    }

    #[test]
    fn line_cross_constraint_uses_explicit_single_line_height() {
        let available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(80.0)),
            width_basis: PercentageBasis::definite_from(
                content_box_pt(80.0),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: Some(PhysicalContentHeight::new(content_box_pt(120.0))),
            height_basis: PercentageBasis::indefinite(),
        };
        let mut style = ComputedStyle::initial();
        style.box_values.height.replace_with_used(
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(60.0),
            ),
        );

        assert_eq!(
            FlexLineCrossConstraint::from_container(
                &style,
                available,
                FlexDirection::Row,
                FlexCrossSize::new(60.0),
            ),
            FlexLineCrossConstraint::DefiniteInnerSize(FlexCrossSize::new(60.0))
        );
    }

    fn test_line(
        item_indices: Vec<usize>,
        cross_start: FlexCrossOffset,
        cross_end: FlexCrossOffset,
    ) -> FlexLineLayout {
        FlexLineLayout {
            source_start: item_indices.iter().cloned().min().unwrap_or(0),
            source_end: item_indices
                .iter()
                .cloned()
                .max()
                .map(|index| index + 1)
                .unwrap_or(0),
            item_indices,
            main_start: FlexMainOffset::new(0.0),
            main_end: FlexMainOffset::new(0.0),
            cross_start,
            cross_end,
            first_baseline: None,
            last_baseline: None,
            collapsed_struts: Vec::new(),
        }
    }

    fn test_child() -> StyledChild<'static> {
        StyledChild {
            kind: FormattingContextChildKind::AnonymousContent { children: vec![] },
            style: ComputedStyle::initial(),
        }
    }

    #[test]
    fn balance_context_uses_cross_gap_when_adding_requested_lines() {
        let mut lines = vec![test_line(
            vec![0, 1],
            FlexCrossOffset::new(0.0),
            FlexCrossOffset::new(10.0),
        )];
        let mut items = vec![
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(0.0, 0.0),
                ContainerSize::new(20.0, 10.0),
            )),
            FlexItemLayout::new(ContainerRect::new(
                ContainerPoint::new(20.0, 0.0),
                ContainerSize::new(20.0, 10.0),
            )),
        ];
        let children = vec![test_child(), test_child()];

        assert!(rebalance_flex_line_membership(
            &mut lines,
            &mut items,
            &children,
            FlexBalanceContext {
                physical_direction: FlexDirection::Row,
                requested_line_count: Some(2),
                hypothetical_main_sizes: None,
                main_gap: FlexMainSize::new(0.0),
                cross_gap: FlexCrossSize::new(15.0),
                available_main_size: FlexMainSize::new(100.0),
            },
        ));

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].cross_start, FlexCrossOffset::new(25.0));
        assert_eq!(lines[1].cross_end, FlexCrossOffset::new(35.0));
    }

    #[test]
    fn canonical_wrap_topology_uses_hypothetical_outer_main_sizes() {
        let lines = collect_flex_line_topology(
            4,
            FlexWrap::Wrap,
            &[
                FlexMainSize::new(60.0),
                FlexMainSize::new(45.0),
                FlexMainSize::new(20.0),
                FlexMainSize::new(25.0),
            ],
            Some(FlexMainSize::new(100.0)),
            FlexMainSize::new(5.0),
        );

        assert_eq!(
            lines,
            vec![
                FlexLineTopology {
                    item_indices: vec![0],
                    source_start: 0,
                    source_end: 1,
                },
                FlexLineTopology {
                    item_indices: vec![1, 2, 3],
                    source_start: 1,
                    source_end: 4,
                },
            ]
        );
    }

    #[test]
    fn canonical_wrap_topology_keeps_an_oversized_first_item_on_its_own_line() {
        let lines = collect_flex_line_topology(
            3,
            FlexWrap::WrapReverse,
            &[
                FlexMainSize::new(140.0),
                FlexMainSize::new(0.0),
                FlexMainSize::new(30.0),
            ],
            Some(FlexMainSize::new(100.0)),
            FlexMainSize::new(0.0),
        );

        assert_eq!(lines[0].item_indices, vec![0]);
        assert_eq!(lines[1].item_indices, vec![1, 2]);
    }

    #[test]
    fn balance_outer_main_sizes_keep_negative_margin_clamp() {
        let outer_main_sizes = [FlexMainSize::new(20.0 - 30.0), FlexMainSize::new(30.0)];

        assert_eq!(outer_main_sizes[0], FlexMainSize::new(0.0));
        assert_eq!(
            balanced_flex_line_count(
                &outer_main_sizes,
                FlexMainSize::new(0.0),
                FlexMainSize::new(30.0),
            ),
            1
        );
    }

    #[test]
    fn balance_partitions_overflowing_main_sizes_at_available_boundary() {
        let item_indices = [0, 1, 2];
        let outer_main_sizes = [
            FlexMainSize::new(60.0),
            FlexMainSize::new(60.0),
            FlexMainSize::new(40.0),
        ];

        assert_eq!(
            balanced_flex_line_partitions(
                &item_indices,
                &outer_main_sizes,
                2,
                FlexMainSize::new(0.0),
                FlexMainSize::new(100.0),
            ),
            Some(vec![vec![0], vec![1, 2]])
        );
    }
}
