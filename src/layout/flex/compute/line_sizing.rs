use super::*;

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
            if let Some(container_cross_size) = cross_constraint.single_line_slot_size() {
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
    cross_axis_layout: FlexLineCrossAxisLayout,
) {
    refresh_flex_line_cross_bounds(
        lines,
        items,
        children,
        container_style,
        physical_direction,
        cross_axis_layout.constraint,
    );
    resolve_flex_line_cross_sizes(
        lines,
        items,
        estimates,
        children,
        container_style,
        physical_direction,
        cross_axis_layout,
    );
    if let Some(container_cross_size) = cross_axis_layout.constraint.single_line_slot_size() {
        stretch_wrapped_flex_lines_to_container_cross_size(
            lines,
            items,
            container_style,
            physical_direction,
            container_cross_size,
            cross_axis_layout.gap,
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
pub(in crate::layout::flex) fn stretch_wrapped_flex_lines_to_container_cross_size(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
    cross_gap: FlexCrossSize,
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
    let axes = PhysicalFlexDirection::new(physical_direction);
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

/// Pack final flex-line slots after every post-measurement size refinement.
///
/// Taffy correctly packs the initial line boxes, but Spindrift subsequently
/// rebuilds their cross sizes from the Flexbox line-sizing inputs. That
/// reconstruction deliberately makes adjacent slots so later stretch and
/// fragmentation measurements have stable inputs. Repack those final slots
/// before source coordinates are captured for fragment replay; otherwise
/// positional `align-content` values such as `center` retain offsets computed
/// for obsolete line sizes:
/// <https://www.w3.org/TR/css-flexbox-1/#align-content-property>.
pub(in crate::layout::flex) fn repack_final_flex_line_offsets(
    items: &mut [FlexItemLayout],
    lines: &mut [FlexLineLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    container_cross_size: FlexCrossSize,
    line_cross_gap: FlexCrossSize,
) -> bool {
    if lines.is_empty()
        || container_style.flex_wrap == FlexWrap::NoWrap
        // `normal` resolves to `stretch` for flex line packing. Its line
        // slots have already been placed by the stretch phase, including the
        // overflowing `flex-start` fallback; rebuilding them here would
        // overwrite that fallback and move a wrap-reverse line to the wrong
        // edge.
        || matches!(
            container_style.align_content.keyword,
            ContentAlignmentKeyword::Normal | ContentAlignmentKeyword::Stretch
        )
    {
        return false;
    }

    let alignment = container_style.align_content;
    if matches!(
        alignment.keyword,
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline
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
    // `space-around` and `space-evenly` fall back to center. Positional
    // alignment uses the same final free space, rather than Taffy's stale
    // pre-remeasurement slots.
    // <https://www.w3.org/TR/css-align-3/#distribution-flex>
    let (initial, distributed_between) = match alignment.keyword {
        ContentAlignmentKeyword::Normal
        | ContentAlignmentKeyword::Start
        | ContentAlignmentKeyword::FlexStart
        | ContentAlignmentKeyword::Left
        | ContentAlignmentKeyword::Stretch => {
            (FlexCrossLength::new(0.0), FlexCrossLength::new(0.0))
        }
        ContentAlignmentKeyword::End
        | ContentAlignmentKeyword::FlexEnd
        | ContentAlignmentKeyword::Right => (free_space, FlexCrossLength::new(0.0)),
        ContentAlignmentKeyword::Center => (free_space.half(), FlexCrossLength::new(0.0)),
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
        ContentAlignmentKeyword::Baseline | ContentAlignmentKeyword::LastBaseline => {
            unreachable!("content-baseline alignment returned above")
        }
    };

    let safe_overflow_fallback =
        free_space.is_negative() && alignment.safety == AlignmentSafety::Safe;
    let (initial, distributed_between) = if safe_overflow_fallback {
        (FlexCrossLength::new(0.0), FlexCrossLength::new(0.0))
    } else {
        (initial, distributed_between)
    };

    let cross_origin = FlexCrossOffset::new(0.0);
    // `FlexLineLayout` records are collected in Taffy's physical traversal
    // order. CSS `wrap-reverse` reverses that physical order, but does not
    // change the order-modified identity of a logical flex line. Pack by the
    // durable cross-start rank rather than by a source index, because source
    // order is unrelated to the `order`-modified flex sequence.
    let mut logical_order = (0..lines.len()).collect::<Vec<_>>();
    logical_order.sort_by_key(|&index| lines[index].logical_cross_start_rank);
    // `safe` overflow falls back to logical `start`, whereas ordinary
    // `flex-start` follows the wrap-reversed flex cross start.  The values
    // coincide without `wrap-reverse`, but using the latter after a safe
    // fallback moves an overflowing line back onto the unsafe edge.
    // <https://drafts.csswg.org/css-align-3/#overflow-values>
    // <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property>
    let packing_start_side = final_line_packing_start_side(container_style, safe_overflow_fallback);
    if packing_start_side.is_start_edge() {
        let mut next_start = cross_origin + initial;
        for line_index in logical_order {
            let line = &mut lines[line_index];
            let delta = next_start - line.cross_start;
            shift_flex_line_cross_axis(line, items, physical_direction, delta);
            next_start = line.cross_end + line_cross_gap + distributed_between;
        }
    } else {
        let mut next_end = cross_origin + container_cross_size - initial;
        for line_index in logical_order {
            let line = &mut lines[line_index];
            let target_start = next_end - line.cross_size();
            let delta = target_start - line.cross_start;
            shift_flex_line_cross_axis(line, items, physical_direction, delta);
            next_end = line.cross_start - line_cross_gap - distributed_between;
        }
    }
    true
}

/// Select the physical edge from which final flex-line slots are repacked.
///
/// `flex-start` follows the cross axis after `wrap-reverse`, but the safe
/// overflow fallback prescribed by CSS Align is logical `start` and therefore
/// uses the unreversed cross axis.
/// <https://drafts.csswg.org/css-align-3/#overflow-values>
/// <https://www.w3.org/TR/css-flexbox-1/#flex-wrap-property>
pub(in crate::layout::flex) fn final_line_packing_start_side(
    container_style: &ComputedStyle,
    safe_overflow_fallback: bool,
) -> PhysicalSide {
    if safe_overflow_fallback {
        flex_unreversed_cross_start_side(container_style)
    } else {
        flex_cross_start_side(container_style)
    }
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
    cross_axis_layout: FlexLineCrossAxisLayout,
) {
    let cross_constraint = cross_axis_layout.constraint;
    if lines.is_empty() {
        return;
    }
    if let FlexLineCrossConstraint::BalancedLineSlot(reserved_slot) = cross_constraint {
        // The reserved balanced slot constrains measurement and line
        // formation, but it is not a maximum. A specified cross size
        // (including one resolved from the container percentage basis) can
        // overflow that slot. Physical columns re-pack their final used line
        // widths from the balanced membership, while rows retain their
        // declared measurement slots for cross-axis placement.
        // <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>
        let resolved_sizes = lines
            .iter()
            .map(|line| {
                line.item_indices
                    .iter()
                    .map(|&index| {
                        item_outer_cross_size(
                            &items[index],
                            &children[index].style,
                            physical_direction,
                        )
                    })
                    .fold(reserved_slot, FlexCrossSize::max)
                    .max(line.largest_collapsed_strut())
            })
            .collect::<Vec<_>>();
        if physical_direction.is_row_axis() {
            // A definite row container retains the slots allocated from its
            // declared count. Overflowing a slot changes the line's used
            // cross size but not the next reserved start edge; otherwise a
            // percentage height would be measured against the container and
            // then also consume that overflow a second time during packing.
            for (line, resolved_size) in lines.iter_mut().zip(resolved_sizes) {
                line.cross_end = line.cross_start + resolved_size;
            }
        } else {
            apply_resolved_flex_line_cross_sizes(
                lines,
                items,
                container_style,
                physical_direction,
                cross_axis_layout.gap,
                resolved_sizes,
            );
        }
        return;
    }
    // A single-line flex container uses its inner cross size only when that
    // size is definite. An automatic row instead derives its line from the
    // remeasured hypothetical item contributions below.
    // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
    if container_style.flex_wrap == FlexWrap::NoWrap
        && cross_constraint.single_line_slot_size().is_some()
    {
        return;
    }

    // A wrapped physical column with a definite cross constraint has already
    // resolved each automatic cross size as fit-content before Taffy formed
    // its lines.  Replacing that used contribution with the estimate's
    // max-content metric here loses the constraint during final replay: a
    // 100px-wide column can incorrectly become a 333px-wide item after its
    // otherwise-correct Taffy allocation.  The current margin-box cross
    // extents are the final used contributions for this line-sizing step;
    // later line-slot remeasurement still handles stretch and explicit
    // intrinsic constraints.
    // <https://www.w3.org/TR/css-flexbox-1/#algo-cross-line>
    // <https://www.w3.org/TR/css-sizing-3/#fit-content-sizing>
    if physical_direction.is_column_axis()
        && container_style.flex_wrap.wraps()
        && cross_constraint.single_line_slot_size().is_some()
    {
        let resolved_sizes = lines
            .iter()
            .map(|line| {
                line.item_indices
                    .iter()
                    .map(|&index| {
                        item_outer_cross_size(
                            &items[index],
                            &children[index].style,
                            physical_direction,
                        )
                    })
                    .fold(line.largest_collapsed_strut(), FlexCrossSize::max)
            })
            .collect::<Vec<_>>();
        apply_resolved_flex_line_cross_sizes(
            lines,
            items,
            container_style,
            physical_direction,
            cross_axis_layout.gap,
            resolved_sizes,
        );
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
    apply_resolved_flex_line_cross_sizes(
        lines,
        items,
        container_style,
        physical_direction,
        cross_axis_layout.gap,
        resolved_sizes,
    );
}

/// Place already-resolved line cross sizes in physical line order.
fn apply_resolved_flex_line_cross_sizes(
    lines: &mut [FlexLineLayout],
    items: &mut [FlexItemLayout],
    container_style: &ComputedStyle,
    physical_direction: FlexDirection,
    cross_gap: FlexCrossSize,
    resolved_sizes: Vec<FlexCrossSize>,
) {
    let axes = PhysicalFlexDirection::new(physical_direction);
    let mut physical_order = (0..lines.len()).collect::<Vec<_>>();
    physical_order.sort_by(|&left, &right| {
        lines[left]
            .cross_start
            .partial_cmp(&lines[right].cross_start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let cross_start_side = flex_baseline_alignment_side(container_style, FlexBaselineSet::First);
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
