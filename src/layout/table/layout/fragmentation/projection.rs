//! Source-to-destination grid projection and structural paint projection.

use super::*;
use crate::layout::block::suppress_fragmented_box_edges;
use crate::layout::paint_ops::FragmentedDecorationSlice;

/// One committed source-row slice exposed by a table fragment.
///
/// The source offset is deliberately retained in table-grid block coordinates;
/// it is not a page coordinate and must never be combined directly with a
/// destination fragmentainer origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout::table) struct TableGridSourceRowSlice {
    pub(in crate::layout::table) row_index: usize,
    pub(in crate::layout::table) block_start: TableGridBlockOffset,
    pub(in crate::layout::table) block_size: TableGridLength,
    /// The matching destination fragmentainer block offset. This is recorded
    /// at row-commit time rather than reconstructed during structural paint.
    pub(in crate::layout::table) destination_block_start: TableGridBlockOffset,
}

/// The complete mapping from one retained table grid to one committed
/// destination fragmentainer.
///
/// Source row slices belong to the unfragmented table grid, while destination
/// slices are packed at the fragmentainer's logical block start. Keeping both
/// placements and the durable source slices in one value prevents a caller
/// from accidentally using a source-table offset as a physical destination
/// origin.
/// <https://drafts.csswg.org/css-tables-3/#table-fragmentation>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableGridFragmentProjection {
    source_frame: TableSourceGridFrame,
    destination_frame: TableDestinationCellGridFrame,
    source_row_slices: Vec<TableGridSourceRowSlice>,
}

impl TableGridFragmentProjection {
    pub(in crate::layout::table) fn new(
        source_placement: TableGridPlacement,
        destination_frame: TableDestinationCellGridFrame,
    ) -> Self {
        Self {
            source_frame: TableSourceGridFrame::new(source_placement),
            destination_frame,
            source_row_slices: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(in crate::layout::table) fn fixture(
        source_placement: TableGridPlacement,
        destination: TableGridPlacement,
    ) -> Self {
        Self::new(
            source_placement,
            TableDestinationCellGridFrame::fixture(destination),
        )
    }

    pub(in crate::layout::table) fn source_placement(&self) -> TableGridPlacement {
        self.source_frame.grid()
    }

    pub(in crate::layout::table) fn destination_placement(&self) -> TableGridPlacement {
        self.destination_frame.grid()
    }

    pub(in crate::layout::table) fn record_source_row_slice(
        &mut self,
        row: TableRowBounds,
        decision: TableRowFragmentDecision,
    ) {
        let block_start = TableGridBlockOffset::new(TableGridLength::new(
            row.start + decision.row_offset.max(0.0),
        ));
        let block_size = TableGridLength::new(decision.row_height.max(0.0));
        if block_size.get() > 0.0 {
            let destination_block_start = self
                .source_row_slices
                .last()
                .map(|previous| {
                    let source_gap = (block_start.length().get()
                        - (previous.block_start.length().get() + previous.block_size.get()))
                    .max(0.0);
                    TableGridBlockOffset::new(TableGridLength::new(
                        previous.destination_block_start.length().get()
                            + previous.block_size.get()
                            + source_gap,
                    ))
                })
                .unwrap_or_else(|| TableGridBlockOffset::new(TableGridLength::new(0.0)));
            self.source_row_slices.push(TableGridSourceRowSlice {
                row_index: decision.row_index,
                block_start,
                block_size,
                destination_block_start,
            });
        }
    }

    pub(in crate::layout::table) fn source_row_slices(&self) -> &[TableGridSourceRowSlice] {
        &self.source_row_slices
    }

    /// Look up a committed source slice by its source row identity.
    ///
    /// Collapsed rows intentionally have no visible source slice, so the
    /// slice vector is not index-aligned with a fragment plan's row list.
    /// Structural paint must therefore use the durable source-row identity
    /// rather than its local plan position.
    pub(in crate::layout::table) fn source_row_slice(
        &self,
        row_index: usize,
    ) -> Option<&TableGridSourceRowSlice> {
        self.source_row_slices
            .iter()
            .find(|slice| slice.row_index == row_index)
    }

    /// Project one source-grid slice into this fragmentainer exactly once.
    pub(in crate::layout::table) fn project_slice(
        &self,
        source_slice: TableGridRect,
        destination_slice: TableGridRect,
        source_inline_edge: TableGridLength,
    ) -> TableStructuralPaintProjection {
        TableStructuralPaintProjection::from_grid_slices(
            self.source_placement(),
            self.destination_placement(),
            source_slice,
            destination_slice,
            source_inline_edge,
        )
    }
}

/// Projection of immutable source-grid geometry into one committed table
/// fragment viewport.
///
/// Table tracks retain their unfragmented logical positions while each table
/// body fragment exposes only the row pieces recorded in its
/// [`TableFragmentPlan`]. Keeping those concepts together prevents callers
/// from accidentally treating a fragment-local page origin as a source-grid
/// offset.
/// <https://drafts.csswg.org/css-tables-3/#table-fragmentation>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableGridFragmentViewport {
    projection: TableGridFragmentProjection,
    destination_frame: TableFragmentainerFrame,
    root_background_source_placement: TableGridPlacement,
    wrapper_timeline: TableWrapperFragmentTimeline,
    source_row_bounds: Vec<TableRowBounds>,
}

/// The CSS table-root background view of one fragmented table body.
///
/// CSS Tables paints the table root from its grid, separated-border edge
/// spacing, padding, and border, but deliberately excludes captions.  The
/// source area therefore remains the complete root box for
/// `box-decoration-break: slice`, while every committed row piece supplies a
/// distinct destination clip in its fragmentainer.
/// <https://drafts.csswg.org/css-tables-3/#table-root>
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
#[derive(Debug, Clone)]
pub(in crate::layout::table) struct TableWrapperDecorationViewport {
    fragments: Vec<TableWrapperDecorationSlice>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableWrapperDecorationSlice {
    destination_clip_border_area: PaintBackgroundArea,
    decoration: FragmentedDecorationSlice,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRootLogicalInsets {
    inline_start: TableGridLength,
    inline_end: TableGridLength,
    block_start: TableGridLength,
    block_end: TableGridLength,
}

impl TableRootLogicalInsets {
    pub(in crate::layout::table) fn block_start(self) -> TableGridLength {
        self.block_start
    }
}

/// One structural table-paint slice projected from the unfragmented logical
/// grid into a committed fragmentainer.
///
/// Source and destination row rectangles intentionally share a table-grid
/// type but never a placement. The source retains the row offset used for
/// `box-decoration-break: slice`; the destination is packed at the physical
/// fragmentainer grid origin. Keeping them together prevents a source offset
/// from shifting the destination table origin.
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableStructuralPaintProjection {
    source_clip: PaintRect,
    destination_clip: PaintRect,
    pub(in crate::layout::table) source_to_destination: PaintTranslation,
}

impl TableStructuralPaintProjection {
    pub(in crate::layout::table) fn from_grid_slices(
        source_placement: TableGridPlacement,
        destination_placement: TableGridPlacement,
        source_slice: TableGridRect,
        destination_slice: TableGridRect,
        _source_inline_edge: TableGridLength,
    ) -> Self {
        // `TableGridPlacement::page_top_rect_for` is the writing-mode
        // boundary. Both rectangles are consequently already physical page
        // geometry; applying a logical-to-page transform here would rotate
        // vertical backgrounds a second time.
        let source_clip = source_placement
            .page_top_rect_for(source_slice)
            .paint_rect();
        let destination_clip = destination_placement
            .page_top_rect_for(destination_slice)
            .paint_rect();
        Self {
            source_clip,
            destination_clip,
            source_to_destination: PaintTranslation::new(
                destination_clip.origin.x - source_clip.origin.x,
                destination_clip.origin.y - source_clip.origin.y,
            ),
        }
    }

    pub(in crate::layout::table) fn source_clip(self) -> PaintRect {
        self.source_clip
    }

    pub(in crate::layout::table) fn destination_clip(self) -> PaintRect {
        self.destination_clip
    }
}

/// Select the table structural layer whose originating cells should expose a
/// background. The selected layer's positioning area remains separate from
/// the cell projections produced below.
///
/// CSS 2.2 paints row, column, row-group, and column-group backgrounds through
/// the complete areas of cells originating in those structures. A cell that
/// merely overlaps a column does not expose that column's background. The
/// same origin rule is used by CSS Tables 3's cell-background algorithm:
/// <https://www.w3.org/TR/CSS2/tables.html#table-layers>;
/// <https://drafts.csswg.org/css-tables-3/#drawing-cell-backgrounds>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) enum TableStructuralOrigin {
    Rows { start: usize, end: usize },
    Columns { start: usize, end: usize },
}

impl TableStructuralOrigin {
    pub(in crate::layout::table) fn contains(self, row: usize, column: usize) -> bool {
        match self {
            Self::Rows { start, end } => (start..end).contains(&row),
            Self::Columns { start, end } => (start..end).contains(&column),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableStructuralVisibleCellRun {
    last_local_row: usize,
    source_start: f32,
    source_end: f32,
    destination_start: f32,
    destination_end: f32,
    last_source_row: usize,
}

impl TableStructuralVisibleCellRun {
    pub(in crate::layout::table) fn new(
        local_row: usize,
        source_row: usize,
        source_start: f32,
        source_end: f32,
        destination_start: f32,
        destination_end: f32,
    ) -> Self {
        Self {
            last_local_row: local_row,
            source_start,
            source_end,
            destination_start,
            destination_end,
            last_source_row: source_row,
        }
    }

    pub(in crate::layout::table) fn extend(
        &mut self,
        local_row: usize,
        source_row: usize,
        source_end: f32,
        destination_end: f32,
    ) {
        self.last_local_row = local_row;
        self.last_source_row = source_row;
        self.source_end = source_end;
        self.destination_end = destination_end;
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::layout::table) fn table_structural_originating_cell_projections(
    projection: &TableGridFragmentProjection,
    row_bounds: &[TableRowBounds],
    column_plan: &TableColumnPlan,
    table_grid: &TableGrid,
    fragment_rows: &[usize],
    row_heights: &[f32],
    row_offsets: &[f32],
    origin: TableStructuralOrigin,
    source_inline_edge: TableGridLength,
) -> Vec<TableStructuralPaintProjection> {
    let mut projections = Vec::new();
    for (origin_row, cells) in table_grid.rows.iter().enumerate() {
        for cell in cells {
            if !origin.contains(origin_row, cell.column) {
                continue;
            }
            let cell_end_row = origin_row
                .saturating_add(cell.rowspan.max(1))
                .min(row_bounds.len());
            let Some(cell_start) = row_bounds.get(origin_row).copied() else {
                continue;
            };
            let Some(cell_end) = cell_end_row
                .checked_sub(1)
                .and_then(|index| row_bounds.get(index))
                .copied()
            else {
                continue;
            };
            let cell_block_start = cell_start.start;
            let cell_block_end = cell_end.start + cell_end.size;
            let cell_inline = column_plan.inline_bounds_for_span(cell.column, cell.colspan);
            let mut visible_run = None;

            let mut commit_run = |run: Option<TableStructuralVisibleCellRun>| {
                let Some(run) = run else {
                    return;
                };
                if run.source_end <= run.source_start
                    || run.destination_end <= run.destination_start
                {
                    return;
                }
                let source_rect = TableGridRect::new(
                    TableGridPoint::from_lengths(
                        cell_inline.start,
                        TableGridLength::new(run.source_start),
                    ),
                    TableGridSize::from_lengths(
                        cell_inline.size,
                        TableGridLength::new(run.source_end - run.source_start),
                    ),
                );
                let destination_rect = TableGridRect::new(
                    TableGridPoint::from_lengths(
                        cell_inline.start,
                        TableGridLength::new(run.destination_start),
                    ),
                    TableGridSize::from_lengths(
                        cell_inline.size,
                        TableGridLength::new(run.destination_end - run.destination_start),
                    ),
                );
                projections.push(projection.project_slice(
                    source_rect,
                    destination_rect,
                    source_inline_edge,
                ));
            };

            for (local_row, source_row) in fragment_rows.iter().copied().enumerate() {
                if source_row < origin_row || source_row >= cell_end_row {
                    commit_run(visible_run.take());
                    continue;
                }
                let Some(&visible_size) = row_heights.get(local_row) else {
                    commit_run(visible_run.take());
                    continue;
                };
                if visible_size <= 0.0 {
                    commit_run(visible_run.take());
                    continue;
                }
                // A committed fragment records source-to-destination row
                // slices as it is laid out.  Structural painting is also
                // useful before that commitment (for example for an
                // unfragmented table and the geometry-level callers), where
                // the row bounds plus the visible row offset are the
                // authoritative grid coordinates.
                let (source_start, destination_start) =
                    if let Some(slice) = projection.source_row_slice(source_row) {
                        (
                            slice.block_start.length().get(),
                            slice.destination_block_start.length().get(),
                        )
                    } else {
                        let Some(row) = row_bounds.get(source_row) else {
                            commit_run(visible_run.take());
                            continue;
                        };
                        let visible_offset = row_offsets.get(local_row).copied().unwrap_or(0.0);
                        let start = row.start + visible_offset.max(0.0);
                        (start, start)
                    };
                let source_start = source_start.max(cell_block_start);
                let source_end = (source_start + visible_size).min(cell_block_end);
                if source_end <= source_start {
                    commit_run(visible_run.take());
                    continue;
                }
                let destination_end = destination_start + visible_size;
                if let Some(run) = &mut visible_run
                    && run.last_local_row + 1 == local_row
                    && run.last_source_row + 1 == source_row
                {
                    run.extend(local_row, source_row, source_end, destination_end);
                } else {
                    commit_run(visible_run.take());
                    visible_run = Some(TableStructuralVisibleCellRun::new(
                        local_row,
                        source_row,
                        source_start,
                        source_end,
                        destination_start,
                        destination_end,
                    ));
                }
            }
            commit_run(visible_run.take());
        }
    }
    projections
}

impl TableWrapperDecorationViewport {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout::table) fn new(
        projection: &TableGridFragmentProjection,
        fragmentainer_placement: TableFragmentainerPlacement,
        destination_page_index: usize,
        root_source_placement: TableGridPlacement,
        wrapper_timeline: TableWrapperFragmentTimeline,
        style: &ComputedStyle,
        table_width: UsedTableWidth,
        block_edge_spacing: f32,
    ) -> Self {
        let source_placement = root_source_placement;
        let insets = table_root_background_logical_insets(
            source_placement,
            style,
            table_width,
            block_edge_spacing,
        );
        let grid_inline = source_placement.logical_inline_grid_extent();
        let grid_block = source_placement.logical_block_grid_extent();
        let root_rect = TableGridRect::new(
            TableGridPoint::from_lengths(-insets.inline_start, -insets.block_start),
            TableGridSize::from_lengths(
                grid_inline + insets.inline_start + insets.inline_end,
                grid_block + insets.block_start + insets.block_end,
            ),
        );
        // Root decoration replay consumes the wrapper-root source frame,
        // never a grid-content source offset with the full border-box span.
        let root_source_frame = wrapper_timeline.root_source_frame(root_rect);
        debug_assert!(root_source_frame.local_block_start().points() >= 0.0);
        debug_assert!(
            (root_source_frame.block_span().get() - root_rect.size.height).abs() <= f32::EPSILON
        );
        let root_rect = root_source_frame.root_rect();
        // `root_rect` already includes both wrapper block insets. Its source
        // geometry is the complete unfragmented border box used for
        // `box-decoration-break: slice`; adding the trailing inset again
        // shifts the positioning area and changes a repeating gradient's
        // phase in every continuation fragment.
        let source_positioning_rect = TableGridRect::new(
            root_rect.origin,
            TableGridSize::from_lengths(
                TableGridLength::new(root_rect.size.width),
                TableGridLength::new(root_rect.size.height),
            ),
        );
        let source_positioning_border_area = PaintBackgroundArea::from_paint_rect(
            source_placement
                .page_top_rect_for(source_positioning_rect)
                .paint_rect(),
        );
        let mut fragments = Vec::new();
        // Structural paint is emitted while each row piece is committed. The
        // current timeline entry is therefore exactly this paint call's
        // visible grid intersection; replaying all earlier entries would
        // paint their root backgrounds again for every subsequent row.
        for slice in
            wrapper_timeline.grid_body_slices_for(fragmentainer_placement, destination_page_index)
        {
            let row_height = slice.source.size().get();
            debug_assert!(row_height > 0.0);
            let block_start = slice
                .grid_source_start
                .expect("grid-body timeline entries retain their grid source interval")
                .length();
            let destination_placement = projection.destination_placement();
            let block_end = block_start + TableGridLength::new(row_height);
            let before = if block_start.get() <= 0.0 {
                insets.block_start
            } else {
                TableGridLength::new(0.0)
            };
            let after = if block_end >= grid_block {
                insets.block_end
            } else {
                TableGridLength::new(0.0)
            };
            let rect = TableGridRect::new(
                TableGridPoint::from_lengths(-insets.inline_start, block_start - before),
                TableGridSize::from_lengths(
                    grid_inline + insets.inline_start + insets.inline_end,
                    TableGridLength::new(row_height) + before + after,
                ),
            );
            let destination_block_start = slice.destination_grid_start.length().get();
            let destination_rect = TableGridRect::new(
                TableGridPoint::from_lengths(
                    -insets.inline_start,
                    TableGridLength::new(destination_block_start) - before,
                ),
                TableGridSize::from_lengths(
                    grid_inline + insets.inline_start + insets.inline_end,
                    TableGridLength::new(row_height) + before + after,
                ),
            );
            let projection = TableStructuralPaintProjection::from_grid_slices(
                source_placement,
                destination_placement,
                rect,
                destination_rect,
                TableGridLength::new(0.0),
            );
            let owns_block_start = block_start.get() <= 0.01;
            let owns_block_end = block_end.get() >= grid_block.get() - 0.01;
            let destination_clip_border_area =
                PaintBackgroundArea::from_paint_rect(projection.destination_clip());
            fragments.push(TableWrapperDecorationSlice {
                destination_clip_border_area: PaintBackgroundArea::from_paint_rect(
                    projection.destination_clip(),
                ),
                decoration: FragmentedDecorationSlice::new(
                    source_positioning_border_area.paint_rect(),
                    destination_clip_border_area.paint_rect(),
                    table_grid_source_progress_translation(
                        source_placement.writing_mode(),
                        TableGridBlockOffset::new(block_start),
                    ),
                    owns_block_start,
                    owns_block_end,
                ),
            });
        }
        if fragments.is_empty() && !wrapper_timeline.has_grid_body_slices() {
            let projection = TableStructuralPaintProjection::from_grid_slices(
                source_placement,
                projection.destination_placement(),
                root_rect,
                root_rect,
                TableGridLength::new(0.0),
            );
            let destination_clip_border_area =
                PaintBackgroundArea::from_paint_rect(projection.destination_clip());
            fragments.push(TableWrapperDecorationSlice {
                destination_clip_border_area: PaintBackgroundArea::from_paint_rect(
                    projection.destination_clip(),
                ),
                decoration: FragmentedDecorationSlice::new(
                    source_positioning_border_area.paint_rect(),
                    destination_clip_border_area.paint_rect(),
                    projection.source_to_destination,
                    true,
                    true,
                ),
            });
        }
        Self { fragments }
    }

    pub(in crate::layout::table) fn image_primitives(
        &self,
        style: &ComputedStyle,
        base_url: Option<&url::Url>,
        root_url: Option<&url::Url>,
        resource_cache: &ResourceCache,
    ) -> Vec<PaintPrimitive> {
        self.fragments
            .iter()
            .flat_map(|fragment| {
                let mut fragment_style = style.clone();
                suppress_fragmented_box_edges(
                    &mut fragment_style,
                    fragment.decoration.owns_block_start(),
                    fragment.decoration.owns_block_end(),
                );
                // Resolve the CSS background in the destination fragment's
                // physical coordinate system.  For `slice`, the shared
                // decoration contract translates the one unbroken source
                // positioning area; the destination area remains solely the
                // paint/clip geometry.  Resolving in source coordinates and
                // translating the primitive afterwards double-counts the
                // table fragmentainer translation for gradients and patterns.
                // <https://www.w3.org/TR/css-break-3/#break-decoration>
                // <https://www.w3.org/TR/css-backgrounds-3/#background-position>
                let positioning_border_area = PaintBackgroundArea::from_paint_rect(
                    fragment
                        .decoration
                        .positioning_border_rect(style.box_decoration_break),
                );
                fragmented_table_root_background_image_primitives(
                    positioning_border_area,
                    fragment.destination_clip_border_area,
                    &fragment_style,
                    base_url,
                    root_url,
                    resource_cache,
                )
            })
            .collect()
    }

    /// Paint the table-root color through the same projected clips as its
    /// background images.
    ///
    /// A fragmented table root has one source border area and one destination
    /// clip per visible row piece.  Resolving the color against the old
    /// fragment-local wrapper rectangle made the color layer disagree with the
    /// image layer, especially after a vertical writing-mode projection.
    pub(in crate::layout::table) fn color_primitives(
        &self,
        style: &ComputedStyle,
        table_width: UsedTableWidth,
    ) -> Vec<PaintPrimitive> {
        let Some(fill) = style.background.background_color.visible_color(style.color) else {
            return Vec::new();
        };
        self.fragments
            .iter()
            .filter_map(|fragment| {
                let destination = fragment.destination_clip_border_area.paint_rect();
                let clip = background_rect_clip_area_for_box(
                    destination,
                    style,
                    table_width.border_widths,
                    style.background.background_clip,
                    None,
                );
                (clip.size.width > 0.0 && clip.size.height > 0.0)
                    .then(|| PaintPrimitive::Rect(RenderedRect::from_paint_rect(clip, Some(fill))))
            })
            .collect()
    }
}

/// Map a table-grid source interval into the table-local replay canvas.
///
/// The enclosing fragmentation context later maps completed temporary parent
/// fragments to columns/pages.  A table-root decoration must therefore carry
/// only the immutable grid-source progress here: using the difference between
/// temporary-page origins leaks the parent replay translation into the table
/// background phase, and makes captions affect `box-decoration-break: slice`.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
pub(in crate::layout::table) fn table_grid_source_progress_translation(
    writing_mode: WritingMode,
    source_block_start: TableGridBlockOffset,
) -> PaintTranslation {
    let progress = source_block_start.length().get();
    match writing_mode {
        WritingMode::HorizontalTb => PaintTranslation::new(0.0, progress),
        WritingMode::VerticalLr | WritingMode::SidewaysLr => PaintTranslation::new(progress, 0.0),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => PaintTranslation::new(-progress, 0.0),
    }
}

pub(in crate::layout::table) fn table_root_background_logical_insets(
    placement: TableGridPlacement,
    style: &ComputedStyle,
    table_width: UsedTableWidth,
    block_edge_spacing: f32,
) -> TableRootLogicalInsets {
    let axes = WritingModeAxes::new(placement.writing_mode(), style.used_direction());
    let edge = |edges: css::Edges, side| match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    };
    let inset = |side| {
        TableGridLength::new(
            edge(table_width.border_widths, side) + edge(table_width.padding, side),
        )
    };
    TableRootLogicalInsets {
        inline_start: inset(axes.physical_side(LogicalSide::InlineStart)),
        inline_end: inset(axes.physical_side(LogicalSide::InlineEnd)),
        block_start: inset(axes.physical_side(LogicalSide::BlockStart))
            + TableGridLength::new(block_edge_spacing),
        block_end: inset(axes.physical_side(LogicalSide::BlockEnd))
            + TableGridLength::new(block_edge_spacing),
    }
}

impl TableGridFragmentViewport {
    pub(in crate::layout::table) fn new(
        source_placement: TableGridPlacement,
        destination_frame: TableFragmentainerFrame,
        root_background_source_placement: TableGridPlacement,
        wrapper_timeline: TableWrapperFragmentTimeline,
        source_row_bounds: Vec<TableRowBounds>,
    ) -> Self {
        Self {
            projection: TableGridFragmentProjection::new(
                source_placement,
                destination_frame.cell_grid_frame(),
            ),
            destination_frame,
            root_background_source_placement,
            wrapper_timeline,
            source_row_bounds,
        }
    }

    /// The unfragmented logical grid used to resolve structural background
    /// positioning. Its origin is deliberately independent of any destination
    /// page or column, as required by `box-decoration-break: slice`.
    pub(in crate::layout::table) fn destination_placement(&self) -> TableGridPlacement {
        self.projection.destination_placement()
    }

    pub(in crate::layout::table) fn fragmentainer_placement(&self) -> TableFragmentainerPlacement {
        self.destination_frame.placement()
    }

    pub(in crate::layout::table) fn destination_frame(&self) -> TableFragmentainerFrame {
        self.destination_frame
    }

    /// The retained unfragmented grid used to resolve `slice` backgrounds and
    /// borders before projecting a row piece into this fragmentainer.
    pub(in crate::layout::table) fn source_placement(&self) -> TableGridPlacement {
        self.projection.source_placement()
    }

    /// The stable grid-local source placement used only by table-root
    /// backgrounds. Captions are wrapper siblings and cannot influence this
    /// CSS background positioning area.
    pub(in crate::layout::table) fn root_background_source_placement(&self) -> TableGridPlacement {
        self.root_background_source_placement
    }

    pub(in crate::layout::table) fn wrapper_timeline(&self) -> TableWrapperFragmentTimeline {
        self.wrapper_timeline.clone()
    }

    pub(in crate::layout::table) fn projection(&self) -> &TableGridFragmentProjection {
        &self.projection
    }

    pub(in crate::layout::table) fn row_bounds(&self) -> &[TableRowBounds] {
        &self.source_row_bounds
    }

    pub(in crate::layout::table) fn record_source_row_slice(
        &mut self,
        decision: TableRowFragmentDecision,
        destination_page_index: usize,
    ) {
        let Some(row) = self.source_row_bounds.get(decision.row_index).copied() else {
            return;
        };
        self.projection.record_source_row_slice(row, decision);
        if let Some(slice) = self.projection.source_row_slices().last().copied() {
            self.wrapper_timeline.record_grid_body_slice(
                self.destination_frame.placement(),
                destination_page_index,
                slice.block_start,
                slice.block_size,
                slice.destination_block_start,
            );
        }
    }

    /// Return the next packed destination block offset without projecting a
    /// physical page-Y coordinate back into the table grid. This is required
    /// for vertical tables, whose block axis is physical X.
    pub(in crate::layout::table) fn next_destination_block_start(
        &self,
        decision: TableRowFragmentDecision,
    ) -> Option<TableGridBlockOffset> {
        let row = self.source_row_bounds.get(decision.row_index)?;
        let block_start = TableGridBlockOffset::new(TableGridLength::new(
            row.start + decision.row_offset.max(0.0),
        ));
        Some(
            self.projection
                .source_row_slices
                .last()
                .map(|previous| {
                    let source_gap = (block_start.length().get()
                        - (previous.block_start.length().get() + previous.block_size.get()))
                    .max(0.0);
                    TableGridBlockOffset::new(TableGridLength::new(
                        previous.destination_block_start.length().get()
                            + previous.block_size.get()
                            + source_gap,
                    ))
                })
                .unwrap_or_else(|| TableGridBlockOffset::new(TableGridLength::new(0.0))),
        )
    }

    pub(in crate::layout::table) fn source_row_slices(&self) -> &[TableGridSourceRowSlice] {
        self.projection.source_row_slices()
    }
}
pub(in crate::layout::table) fn collapsed_cell_decoration_style(
    style: &ComputedStyle,
    collapsed: bool,
) -> ComputedStyle {
    let mut decoration_style = style.clone();
    if !collapsed {
        return decoration_style;
    }

    if decoration_style.background.background_clip == css::BackgroundBox::Border {
        decoration_style.background.background_clip = css::BackgroundBox::Padding;
    }
    for layer in &mut decoration_style.background.background_layers {
        if layer.clip == css::BackgroundBox::Border {
            layer.clip = css::BackgroundBox::Padding;
        }
    }
    decoration_style
}

/// Whether source-logical column order runs opposite the final page paint
/// order.  Structural column backgrounds are a single table painting layer;
/// using this at that layer keeps adjacent opaque spans from taking ownership
/// of each other's fractional device-pixel edge after a writing-mode
/// projection.
///
/// <https://drafts.csswg.org/css-tables-3/#drawing-backgrounds>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
pub(in crate::layout::table) fn table_columns_paint_in_reverse_page_order(
    style: &ComputedStyle,
) -> bool {
    matches!(
        WritingModeAxes::new(style.writing_mode, style.used_direction())
            .physical_side(LogicalSide::InlineStart),
        PhysicalSide::Right | PhysicalSide::Bottom
    )
}

/// Whether a column-group interval contains an explicit `col` layer that
/// must remain above the group background in CSS table paint order.
pub(in crate::layout::table) fn table_column_group_has_explicit_columns(
    columns: &[TableColumn<'_>],
    start_column: usize,
    end_column: usize,
    column_count: usize,
) -> bool {
    let mut column_index = 0;
    for column in columns {
        if column_index >= column_count {
            break;
        }
        let span = column.span.min(column_count - column_index).max(1);
        let column_end = column_index + span;
        let overlaps_group = column_index < end_column && column_end > start_column;
        let is_group_placeholder = column
            .group
            .as_ref()
            .is_some_and(|group| group.signature == column.signature);
        if overlaps_group && !is_group_placeholder {
            return true;
        }
        column_index = column_end;
    }
    false
}
