use super::*;

/// Page-local geometry supplied when flex gap decorations are emitted for one
/// fragmented container slice. It keeps the content span, physical content
/// height, and paint clip from being recombined as unrelated scalars.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexGapDecorationFragmentContext {
    pub(in crate::layout::flex) page_index: usize,
    pub(in crate::layout::flex) content_inline_span: PageInlineSpan,
    pub(in crate::layout::flex) content_height: PhysicalContentHeight,
    pub(in crate::layout::flex) fragment_bounds: PaintClip,
    pub(in crate::layout::flex) has_forced_item_breaks: bool,
}
pub(in crate::layout::flex) fn flex_gap_decoration_primitives_for_page(
    flex_layout: &FlexLayout,
    style: &ComputedStyle,
    context: FlexGapDecorationFragmentContext,
) -> Vec<PaintPrimitive> {
    let Some(fragment_block_bounds) = flex_page_fragment_block_range(
        &flex_layout.fragment_plan,
        context.page_index,
        !matches!(
            style.align_content.keyword,
            ContentAlignmentKeyword::Normal
                | ContentAlignmentKeyword::Start
                | ContentAlignmentKeyword::End
                | ContentAlignmentKeyword::FlexStart
                | ContentAlignmentKeyword::FlexEnd
                | ContentAlignmentKeyword::Left
                | ContentAlignmentKeyword::Right
                | ContentAlignmentKeyword::Center
                | ContentAlignmentKeyword::Baseline
                | ContentAlignmentKeyword::LastBaseline
        ),
    ) else {
        return Vec::new();
    };
    let block_start = fragment_block_bounds.start().points();
    let block_end = fragment_block_bounds.end().points();
    let fragment_height = (block_end - block_start).max(0.0);
    if fragment_height <= 0.01 {
        return Vec::new();
    }

    let mut gutters = flex_gap_decoration_gutters(
        flex_layout,
        style,
        PhysicalContentWidth::new(content_box_pt(context.content_inline_span.width())),
        context.content_height,
    );
    let fragments_at_item_main_axis_boundaries =
        FlexFragmentationBoundaryProjection::for_style(style)
            == FlexFragmentationBoundaryProjection::ItemMainAxis;
    let has_later_page_fragment = flex_layout
        .fragment_plan
        .fragments
        .iter()
        .any(|fragment| fragment.page_index > context.page_index);
    let suppresses_cross_axis_gutter_at_outgoing_break = fragments_at_item_main_axis_boundaries
        && has_later_page_fragment
        && block_end < context.content_height.points() - 0.01;
    let suppressed_cross_axis_gutters = if suppresses_cross_axis_gutter_at_outgoing_break {
        gutters.columns.clone()
    } else {
        Vec::new()
    };
    gutters.columns = gutters
        .columns
        .into_iter()
        .filter_map(|mut gutter| {
            let Some(segment) = gutter.segment_range else {
                // In a physical-Y main-axis fragment, cross-axis gutters run
                // through the source main-axis extent. If that extent reaches
                // the outgoing fragmentation break, the gutter disappears at
                // the break and this fragment paints no decoration for it.
                // The following fragment recomputes its re-established
                // gutter from the surviving item intervals.
                // <https://drafts.csswg.org/css-gaps-1/#fragmentation>
                return (!suppresses_cross_axis_gutter_at_outgoing_break).then_some(gutter);
            };
            // A physical cross-axis gutter that reaches an outgoing
            // item-main-axis fragmentation break has no gap there, so its
            // decoration is suppressed with that gap. The continuation
            // re-establishes the gutter from its own surviving item interval;
            // clipping the original source segment on both pages would paint
            // a decoration through a fragmentation break.
            // <https://drafts.csswg.org/css-gaps-1/#fragmentation>
            if fragments_at_item_main_axis_boundaries && segment.end > block_end + 0.01 {
                return None;
            }
            let start = segment.start.max(block_start);
            let end = segment.end.min(block_end);
            if end <= start + 0.01 {
                return None;
            }
            gutter.segment_range = Some(GapAxisSpan::new(start - block_start, end - block_start));
            Some(gutter)
        })
        .collect();
    if !suppressed_cross_axis_gutters.is_empty() {
        // Once the cross-axis gutter disappears at this outgoing break, its
        // perpendicular main-axis decorations meet through the former
        // junction. Preserve each line-owned rule and let neighboring
        // segments meet at the collapsed gutter's center instead of leaving
        // a decoration-sized hole.
        for gutter in &mut gutters.rows {
            let Some(mut segment) = gutter.segment_range else {
                continue;
            };
            for cross_gutter in &suppressed_cross_axis_gutters {
                let center = (cross_gutter.span.start + cross_gutter.span.end) * 0.5;
                if (segment.end - cross_gutter.span.start).abs() <= 0.01 {
                    segment.end = center;
                }
                if (segment.start - cross_gutter.span.end).abs() <= 0.01 {
                    segment.start = center;
                }
            }
            gutter.segment_range = Some(segment);
        }
    }
    gutters.rows = if context.has_forced_item_breaks {
        // Forced breaks between flex items/lines replace the intervening row
        // gutter with a fragmentainer boundary. No fragment owns that gutter,
        // so it contributes no row-rule segment on either side.
        Vec::new()
    } else {
        flex_fragment_gap_gutters(&gutters.rows, fragment_block_bounds)
    };
    // Visibility is defined by adjacency in the unfragmented flex layout.
    // Retain neighboring items across the fragment boundary when deciding
    // whether a page-local segment is `between` items; restricting metadata
    // to ink already replayed on this page incorrectly hides the rule in the
    // gap immediately before the next fragment.
    // https://drafts.csswg.org/css-gaps-1/#gap-rule-visibility
    let items = flex_layout
        .items
        .iter()
        .map(|item| {
            GapDecorationItem::from_rect(GapDecorationRect::new(
                GapDecorationPoint::new(item.x().points(), item.y().points() - block_start),
                GapDecorationSize::new(item.width().points(), item.height().points()),
            ))
        })
        .collect::<Vec<_>>();

    flex_gap_decoration_primitives_with_gutters(
        style,
        GapDecorationContainer::new(
            context.content_inline_span.left_x(),
            context.fragment_bounds.y() + context.fragment_bounds.height(),
            context.content_inline_span.width(),
            fragment_height,
        ),
        &items,
        &gutters,
    )
}

pub(in crate::layout::flex) fn flex_gap_decoration_items(
    flex_layout: &FlexLayout,
) -> Vec<GapDecorationItem> {
    flex_layout
        .items
        .iter()
        .map(|item| {
            GapDecorationItem::from_rect(GapDecorationRect::new(
                GapDecorationPoint::new(item.x().points(), item.y().points()),
                GapDecorationSize::new(item.width().points(), item.height().points()),
            ))
        })
        .collect()
}

pub(in crate::layout::flex) fn flex_fragment_gap_gutters(
    gutters: &[GapDecorationGutter],
    fragment_block_bounds: FlexFragmentBlockBounds,
) -> Vec<GapDecorationGutter> {
    // Gap decoration is a scalar paint adapter; keep source-range endpoints
    // typed until this projection.
    let block_start = fragment_block_bounds.start().points();
    let block_end = fragment_block_bounds.end().points();
    gutters
        .iter()
        .filter_map(|gutter| {
            // A collapsed, non-break gutter still owns a rule-list slot and
            // its decoration may overflow the zero-width gap. Conversely, a
            // gutter straddling a fragmentainer boundary is suppressed rather
            // than clipped into two independent decorations.
            // <https://drafts.csswg.org/css-gaps-1/#gap-decoration-segments>
            let fully_in_fragment =
                gutter.span.start >= block_start - 0.01 && gutter.span.end <= block_end + 0.01;
            fully_in_fragment.then_some(GapDecorationGutter {
                span: GapAxisSpan::new(
                    gutter.span.start - block_start,
                    gutter.span.end - block_start,
                ),
                ..*gutter
            })
        })
        .collect()
}

pub(in crate::layout::flex) fn flex_item_line_range(
    flex_layout: &FlexLayout,
    item_index: usize,
) -> (usize, usize) {
    flex_layout
        .lines
        .iter()
        .enumerate()
        .find(|(_, line)| line.item_indices.contains(&item_index))
        .map(|(line_index, _)| (line_index, line_index + 1))
        .unwrap_or((0, 0))
}

pub(in crate::layout::flex) fn flex_item_block_bounds(
    item: &FlexItemLayout,
    use_fragmentation_height: bool,
) -> FlexFragmentBlockBounds {
    let height = if use_fragmentation_height {
        FlexFragmentBlockSize::new(item.fragmentation_height().points())
    } else {
        FlexFragmentBlockSize::new(item.height().points())
    };
    FlexFragmentBlockBounds::from_start_and_size(
        FlexFragmentBlockOffset::new(item.y().points()),
        height,
    )
}

pub(in crate::layout::flex) fn flex_gap_decoration_gutters(
    flex_layout: &FlexLayout,
    style: &ComputedStyle,
    content_width: PhysicalContentWidth,
    content_height: PhysicalContentHeight,
) -> GapDecorationGutters {
    let axes = FlexAxes::for_style(style);
    let PhysicalFlexGaps {
        horizontal: physical_gap_width,
        vertical: physical_gap_height,
    } = physical_flex_gaps(style);
    let used_physical_gap_width = used_flex_gap(
        physical_gap_width,
        PercentageBasis::definite(content_width.content_box_length()),
    );
    let used_physical_gap_height = used_flex_gap(
        physical_gap_height,
        PercentageBasis::definite(content_height.content_box_length()),
    );
    let main_gap = if axes.is_main_row_axis() {
        flex_main_gap_size(used_physical_gap_width)
    } else {
        flex_main_gap_size(used_physical_gap_height)
    };
    let cross_gap = if axes.is_main_row_axis() {
        flex_cross_gap_size(used_physical_gap_height)
    } else {
        flex_cross_gap_size(used_physical_gap_width)
    };
    let mut cross_gutters = flex_cross_axis_gap_gutters(
        flex_layout,
        axes,
        cross_gap,
        matches!(
            style.align_content.keyword,
            ContentAlignmentKeyword::SpaceBetween
                | ContentAlignmentKeyword::SpaceAround
                | ContentAlignmentKeyword::SpaceEvenly
        ),
    );
    let mut main_gutters = flex_main_axis_gap_gutters(
        flex_layout,
        axes,
        main_gap,
        matches!(
            style.justify_content.keyword,
            ContentAlignmentKeyword::SpaceBetween
                | ContentAlignmentKeyword::SpaceAround
                | ContentAlignmentKeyword::SpaceEvenly
        ),
    );
    // Both builders return CSS flex order: main-axis gaps advance within each
    // line, then through lines from cross-start to cross-end; cross-axis gaps
    // advance through adjacent lines in that same direction. Preserve those
    // indices while projecting them to physical column/row bands.
    assign_flex_gap_rule_indices(&mut main_gutters, false);
    assign_flex_gap_rule_indices(&mut cross_gutters, false);
    if axes.is_main_row_axis() {
        GapDecorationGutters {
            columns: main_gutters,
            rows: cross_gutters,
        }
    } else {
        GapDecorationGutters {
            columns: cross_gutters,
            rows: main_gutters,
        }
    }
}

fn assign_flex_gap_rule_indices(gutters: &mut [GapDecorationGutter], reverse: bool) {
    let count = gutters.len();
    for (physical_index, gutter) in gutters.iter_mut().enumerate() {
        // The CSS Gaps sequence advances through each actual flex gap, not
        // each unique physical coordinate. Aligned gaps in different wrapped
        // lines therefore consume distinct rule-list entries.
        // <https://drafts.csswg.org/css-gaps-1/#assigning>
        gutter.rule_index = Some(if reverse {
            count.saturating_sub(1).saturating_sub(physical_index)
        } else {
            physical_index
        });
    }
}

/// The cross-axis band occupied by a finalized flex line for gap decoration.
///
/// Ordinary lines retain the cross-size slot resolved by Quire's final flex
/// layout.  Item rectangles cannot stand in for that slot: `align-content:
/// stretch` grows the line even when its fixed-size items do not stretch.
/// Reconstructing a band from items is reserved for the exceptional stale-line
/// replay shape documented by [`FinalizedFlexGapLine`].
enum FinalizedFlexGapLineBand {
    Allocated {
        start: FlexCrossOffset,
        end: FlexCrossOffset,
    },
    ReconstructedFromItems {
        start: FlexCrossOffset,
        end: FlexCrossOffset,
    },
}

impl FinalizedFlexGapLineBand {
    fn start(&self) -> FlexCrossOffset {
        match self {
            Self::Allocated { start, .. } | Self::ReconstructedFromItems { start, .. } => *start,
        }
    }

    fn end(&self) -> FlexCrossOffset {
        match self {
            Self::Allocated { end, .. } | Self::ReconstructedFromItems { end, .. } => *end,
        }
    }
}

/// One final flex line reconstructed for gap-decoration topology.
///
/// Taffy's line-membership record is normally authoritative.  During an
/// indefinite cross-size replay it can, however, retain two wrapped items in
/// one record even after their finalized rectangles occupy disjoint cross
/// bands.  Decorations must follow finalized flex geometry, so split that
/// exceptional overlapping-main/disjoint-cross arrangement before producing
/// actual gaps.  Ordinary aligned items stay in their recorded line.
struct FinalizedFlexGapLine<'a> {
    items: Vec<&'a FlexItemLayout>,
    cross_band: FinalizedFlexGapLineBand,
}

fn finalized_flex_gap_lines<'a>(
    flex_layout: &'a FlexLayout,
    axes: FlexAxes,
) -> Vec<FinalizedFlexGapLine<'a>> {
    let mut resolved = Vec::new();
    for line in &flex_layout.lines {
        let mut items = line
            .item_indices
            .iter()
            .filter_map(|&index| flex_layout.items.get(index))
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.cross_start(axes)
                .partial_cmp(&right.cross_start(axes))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut line_groups = Vec::<Vec<&FlexItemLayout>>::new();
        for item in items {
            let item_cross_start = item.cross_start(axes).points();
            let item_main_start = item.main_start(axes).points();
            let item_main_end = (item.main_start(axes) + item.main_size(axes)).points();
            let split_group = line_groups.last().is_some_and(|group| {
                let group_cross_end = group
                    .iter()
                    .map(|candidate| {
                        (candidate.cross_start(axes) + candidate.cross_size(axes)).points()
                    })
                    .fold(f32::NEG_INFINITY, f32::max);
                item_cross_start > group_cross_end + GAP_RULE_EPSILON
                    && group.iter().any(|candidate| {
                        let start = candidate.main_start(axes).points();
                        let end = (candidate.main_start(axes) + candidate.main_size(axes)).points();
                        item_main_start < end - GAP_RULE_EPSILON
                            && item_main_end > start + GAP_RULE_EPSILON
                    })
            });
            if split_group || line_groups.is_empty() {
                line_groups.push(vec![item]);
            } else {
                line_groups
                    .last_mut()
                    .expect("a non-empty resolved line has a current group")
                    .push(item);
            }
        }
        if line_groups.len() == 1 {
            // `FlexLineLayout` owns the finalized flex line allocation. This
            // differs from the union of its item rectangles whenever the
            // line has free cross-axis space, such as `align-content:
            // stretch` with fixed cross-size items.
            // <https://www.w3.org/TR/css-flexbox-1/#algo-line-break>
            resolved.push(FinalizedFlexGapLine {
                items: line_groups
                    .pop()
                    .expect("a non-empty flex line has one item group"),
                cross_band: FinalizedFlexGapLineBand::Allocated {
                    start: line.cross_start,
                    end: line.cross_end,
                },
            });
            continue;
        }

        // A source line that resolves to multiple disjoint, overlapping-main
        // bands is stale replay metadata rather than one CSS flex line. No
        // allocated per-band slot exists in that exceptional shape, so derive
        // each replacement band from its finalized item rectangles.
        resolved.extend(line_groups.into_iter().map(|items| {
            let cross_start = items
                .iter()
                .map(|item| item.cross_start(axes))
                .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
                .expect("a reconstructed flex gap line has items");
            let cross_end = items
                .iter()
                .map(|item| item.cross_start(axes) + item.cross_size(axes))
                .max_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
                .expect("a reconstructed flex gap line has items");
            FinalizedFlexGapLine {
                items,
                cross_band: FinalizedFlexGapLineBand::ReconstructedFromItems {
                    start: cross_start,
                    end: cross_end,
                },
            }
        }));
    }
    resolved.sort_by(|left, right| {
        left.cross_band
            .start()
            .partial_cmp(&right.cross_band.start())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if axes.cross_start_side().is_end_edge() {
        resolved.reverse();
    }
    resolved
}

pub(in crate::layout::flex) fn flex_main_axis_gap_gutters(
    flex_layout: &FlexLayout,
    axes: FlexAxes,
    used_gap: FlexMainSize,
    has_distributed_gutters: bool,
) -> Vec<GapDecorationGutter> {
    let mut gutters = Vec::new();
    for line in finalized_flex_gap_lines(flex_layout, axes) {
        let mut line_items = line.items;
        line_items.sort_by(|a, b| {
            a.main_start(axes)
                .partial_cmp(&b.main_start(axes))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if axes.main_start_side().is_end_edge() {
            line_items.reverse();
        }
        // Main-axis rule segments occupy the flex line's allocated cross-size,
        // including space added by `align-content: stretch`; the item margin
        // boxes determine gutter centers but do not truncate the line segment.
        // <https://drafts.csswg.org/css-gaps-1/#flex-gaps>
        let line_cross_range = Some((line.cross_band.start(), line.cross_band.end()));
        for pair in line_items.windows(2) {
            // Taffy's finalized item positions already include the exact
            // distributed-alignment remainder. Reconstructing a CSS-pixel
            // correction here loses that information and moves the rule away
            // from the used gutter center.
            // <https://drafts.csswg.org/css-gaps-1/#flex-gaps>
            let first_start = pair[0].main_start(axes).points();
            let first_end = (pair[0].main_start(axes) + pair[0].main_size(axes)).points();
            let second_start = pair[1].main_start(axes).points();
            let second_end = (pair[1].main_start(axes) + pair[1].main_size(axes)).points();
            // `line_items` is in logical main-axis order, which may descend
            // in physical coordinates. The shared painter receives an
            // increasing physical span, so take the facing item edges rather
            // than assuming left-to-right/top-to-bottom progress.
            let start = first_end.min(second_end);
            let end = first_start.max(second_start);
            if let Some((segment_start, segment_end)) = line_cross_range {
                push_unique_flex_gap_gutter_with_segment(
                    &mut gutters,
                    GapAxisSpan::new(start, end),
                    if has_distributed_gutters {
                        FlexGapDecorationGutterWidth::FillAvailable
                    } else {
                        FlexGapDecorationGutterWidth::Fixed(layout_pt(used_gap.points()))
                    },
                    GapAxisSpan::new(segment_start.points(), segment_end.points()),
                );
            }
        }
    }
    // Each flex line owns distinct gaps, even when their physical centers
    // coincide. In particular, their rule-list assignment and patterned paint
    // phase continue across lines rather than being coalesced into one gap.
    // <https://drafts.csswg.org/css-gaps-1/#assigning>
    gutters
}

pub(in crate::layout::flex) fn flex_cross_axis_gap_gutters(
    flex_layout: &FlexLayout,
    axes: FlexAxes,
    used_gap: FlexCrossSize,
    has_distributed_gutters: bool,
) -> Vec<GapDecorationGutter> {
    // Cross-axis gutters are the spaces between resolved flex line boxes.
    // Line metadata excludes the authored gap while retaining distributed and
    // stretched line allocation, so these boundaries are the authoritative
    // used gutter edges.
    // <https://drafts.csswg.org/css-gaps-1/#flex-gaps>
    // Ordinary finalized lines retain their allocated cross bands. The
    // exceptional stale-membership replay case reconstructs a physical band
    // from its item group before it reaches this adapter.
    let line_bands = finalized_flex_gap_lines(flex_layout, axes)
        .into_iter()
        .map(|line| (line.cross_band.start(), line.cross_band.end()))
        .collect::<Vec<_>>();
    let mut gutters = Vec::new();
    for pair in line_bands.windows(2) {
        // Logical cross-axis progression may descend in physical coordinates
        // (`wrap-reverse`, RTL vertical flows, and sideways modes).  As for
        // main-axis portions above, derive the physical increasing span from
        // the two facing line edges instead of assuming the earlier logical
        // line is physically before the next one.
        let first_start = pair[0].0.points();
        let first_end = pair[0].1.points();
        let second_start = pair[1].0.points();
        let second_end = pair[1].1.points();
        push_unique_flex_gap_gutter(
            &mut gutters,
            GapAxisSpan::new(first_end.min(second_end), first_start.max(second_start)),
            if has_distributed_gutters {
                FlexGapDecorationGutterWidth::FillAvailable
            } else {
                FlexGapDecorationGutterWidth::Fixed(layout_pt(used_gap.points()))
            },
        );
    }
    gutters
}

/// The rule extent selected for a gap-decoration paint primitive.
///
/// Distributed alignment fills the resolved gutter, while an authored gap
/// retains its used layout extent. This is deliberately separate from either
/// Flex axis because the next boundary is the axis-neutral gap-decoration
/// painter.
#[derive(Debug, Clone, Copy)]
enum FlexGapDecorationGutterWidth {
    Fixed(LayoutLength),
    FillAvailable,
}

fn push_unique_flex_gap_gutter(
    gutters: &mut Vec<GapDecorationGutter>,
    span: GapAxisSpan,
    used_gap: FlexGapDecorationGutterWidth,
) {
    let start = span.start;
    let end = span.end;
    if end < start - 0.01 {
        return;
    }
    let available = end - start;
    let size = match used_gap {
        FlexGapDecorationGutterWidth::Fixed(width) => width.points().min(available).max(0.0),
        FlexGapDecorationGutterWidth::FillAvailable => available,
    };
    let start = start + (available - size) * 0.5;
    let end = start + size;
    gutters.push(GapDecorationGutter::new(start, end));
}

fn push_unique_flex_gap_gutter_with_segment(
    gutters: &mut Vec<GapDecorationGutter>,
    span: GapAxisSpan,
    used_gap: FlexGapDecorationGutterWidth,
    segment: GapAxisSpan,
) {
    let start = span.start;
    let end = span.end;
    let segment_start = segment.start;
    let segment_end = segment.end;
    if end < start - 0.01 || segment_end <= segment_start + 0.01 {
        return;
    }
    let available = end - start;
    // Distributed alignment increases the effective gutter between adjacent
    // items; the decoration is centered in that entire resolved gutter.
    // https://drafts.csswg.org/css-align-3/#gap-legacy
    let size = match used_gap {
        FlexGapDecorationGutterWidth::Fixed(width) => width.points().min(available).max(0.0),
        FlexGapDecorationGutterWidth::FillAvailable => available,
    };
    let start = start + (available - size) * 0.5;
    let end = start + size;
    gutters.push(GapDecorationGutter::with_segment_range(
        start,
        end,
        segment_start,
        segment_end,
    ));
}

/// Paints CSS Gap Decoration rules for resolved flex gutters.
///
/// Flex layout resolves gaps from line and item placement after wrapping and
/// alignment. Supplying that metadata avoids treating unrelated item bands as
/// synthetic gutters:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-lines> and
/// <https://drafts.csswg.org/css-gaps-1/#segments>.
pub(in crate::layout) fn flex_gap_decoration_primitives_with_gutters(
    style: &ComputedStyle,
    container: GapDecorationContainer,
    items: &[GapDecorationItem],
    gutters: &GapDecorationGutters,
) -> Vec<PaintPrimitive> {
    let column_gaps = gutters
        .columns
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    let row_gaps = gutters
        .rows
        .iter()
        .cloned()
        .map(GapBand::from)
        .collect::<Vec<_>>();
    gap_decoration_primitives_for_gaps(GapDecorationContext {
        style,
        container,
        column_gaps: &column_gaps,
        row_gaps: &row_gaps,
        items,
        container_kind: GapContainerKind::Flex,
    })
}
