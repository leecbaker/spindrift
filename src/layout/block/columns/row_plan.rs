use super::*;

/// The normal-flow anonymous columns committed by temporary multicol layout.
///
/// This excludes an unused terminal fragmentainer, synthetic visual-overflow
/// slices, and fragmentainers reached only by a parallel float or positioned
/// flow. None of those creates a normal-flow row, source break opportunity,
/// or gap-rule slot.
/// <https://drafts.csswg.org/css-break-3/#parallel-flows>
#[derive(Debug, Clone, Copy)]
pub(super) struct CommittedMulticolColumns(NonZeroUsize);

impl CommittedMulticolColumns {
    pub(super) fn from_source_subject_count(count: usize) -> Self {
        Self(NonZeroUsize::new(count.max(1)).expect("source subject count is nonzero"))
    }

    pub(super) fn from_temporary_column_pages(
        temporary_page_count: usize,
        trailing_column_was_never_entered: bool,
    ) -> Self {
        let count = temporary_page_count
            .saturating_sub(usize::from(trailing_column_was_never_entered))
            .max(1);
        Self(NonZeroUsize::new(count).expect("committed multicol column count is nonzero"))
    }

    pub(super) fn count(self) -> usize {
        self.0.get()
    }
}

/// The two independently committed fragment ranges of a multicolumn set.
///
/// The normal-flow range owns rows and CSS Fragmentation break topology. The
/// physical range additionally retains columns reached by parallel floats and
/// absolutely positioned descendants, whose paint and containing-block
/// projections must survive the speculative layout rollback without granting
/// them normal-flow break ownership.
/// <https://drafts.csswg.org/css-break-3/#parallel-flows>
/// <https://drafts.csswg.org/css-multicol-2/#multi-column-model>
#[derive(Debug, Clone, Copy)]
pub(super) struct CommittedMulticolFragmentPlan {
    in_flow_columns: CommittedMulticolColumns,
    physical_column_count: NonZeroUsize,
}

impl CommittedMulticolFragmentPlan {
    pub(super) fn new(
        in_flow_columns: CommittedMulticolColumns,
        physical_column_count: usize,
    ) -> Self {
        Self {
            in_flow_columns,
            physical_column_count: NonZeroUsize::new(physical_column_count.max(1))
                .expect("committed multicol physical range is non-empty"),
        }
    }

    pub(super) fn in_flow_columns(self) -> CommittedMulticolColumns {
        self.in_flow_columns
    }

    pub(super) fn physical_column_count(self) -> usize {
        self.physical_column_count.get()
    }
}

/// The wrapping partition and canonical gap-rule topology of one committed
/// column sequence.
#[derive(Debug, Clone, Copy)]
pub(super) struct CommittedMulticolRows {
    columns: CommittedMulticolColumns,
    columns_per_row: NonZeroUsize,
    count: NonZeroUsize,
}

impl CommittedMulticolRows {
    pub(super) fn new(columns: CommittedMulticolColumns, columns_per_row: NonZeroUsize) -> Self {
        let count = columns.count().div_ceil(columns_per_row.get()).max(1);
        Self {
            columns,
            columns_per_row,
            count: NonZeroUsize::new(count).expect("committed multicol row count is nonzero"),
        }
    }

    pub(super) fn row_count(self) -> usize {
        self.count.get()
    }

    pub(super) fn plan(
        self,
        row: MulticolumnRowIndex,
        row_gap: Option<f32>,
        reverse_rule_order: bool,
    ) -> Option<CommittedMulticolRowPlan> {
        let row_index = row.get();
        (row_index < self.row_count()).then(|| {
            let row_start = row_index * self.columns_per_row.get();
            let occupied_columns = self
                .columns
                .count()
                .saturating_sub(row_start)
                .min(self.columns_per_row.get())
                .max(1);
            let boundary = if row_index + 1 == self.row_count() {
                MulticolRowBoundary::Final
            } else if let Some(gap) = row_gap {
                let sequence = NonZeroUsize::new(self.row_count().saturating_sub(1))
                    .expect("a non-final committed row has a rule sequence");
                let rule_index = if reverse_rule_order {
                    sequence.get().saturating_sub(1).saturating_sub(row_index)
                } else {
                    row_index
                };
                let slot = GapRuleSlot::new(GapRuleIndex::new(rule_index), sequence)
                    .expect("committed row rule index is inside its sequence");
                MulticolRowBoundary::FollowedBy { gap, slot }
            } else {
                MulticolRowBoundary::Final
            };
            CommittedMulticolRowPlan {
                row,
                occupied_columns,
                boundary,
            }
        })
    }
}

/// A committed row plus its paintable boundary assignment.
#[derive(Debug, Clone, Copy)]
pub(super) struct CommittedMulticolRowPlan {
    pub(super) row: MulticolumnRowIndex,
    pub(super) occupied_columns: usize,
    pub(super) boundary: MulticolRowBoundary,
}

/// One source-ordered participant in the multicol container's block flow.
///
/// Spanners are represented explicitly instead of being folded into a scalar
/// consumed height, so a container-wide planner can retain their row boundary
/// and paint ownership when column sets are interleaved with them.
/// <https://drafts.csswg.org/css-multicol-2/#multi-column-model>
#[derive(Debug, Clone, Copy)]
pub(super) enum CommittedMulticolFlowEntry {
    ColumnRow {
        plan: CommittedMulticolRowPlan,
        source_offset: MulticolRowBlockOffset,
        nominal_block_size: UsedMulticolColumnBlockSize,
        flow_consumed_block_size: UsedMulticolColumnBlockSize,
    },
    #[allow(
        dead_code,
        reason = "spanner commit is migrated after column-row placement"
    )]
    Spanner {
        source_offset: MulticolRowBlockOffset,
        used_block_size: LogicalBlockContentSize,
    },
}

/// The source-ordered committed row geometry of a multicol formatting context.
#[derive(Debug, Clone)]
pub(super) struct CommittedMulticolFlow {
    entries: Vec<CommittedMulticolFlowEntry>,
    block_extent: LayoutLength,
}

impl CommittedMulticolFlow {
    pub(super) fn from_column_rows(
        rows: CommittedMulticolRows,
        used_block_size: UsedMulticolColumnBlockSize,
        initial_row_used_block_size: Option<UsedMulticolColumnBlockSize>,
        final_row_used_block_size: UsedMulticolColumnBlockSize,
        row_gap: Option<f32>,
        reverse_rule_order: bool,
    ) -> Self {
        let gap = row_gap.unwrap_or(0.0).max(0.0);
        let mut entries = Vec::with_capacity(rows.row_count());
        let mut source_offset = layout_pt(0.0);
        for row_index in 0..rows.row_count() {
            let row = MulticolumnRowIndex::new(row_index);
            let plan = rows
                .plan(row, row_gap, reverse_rule_order)
                .expect("a committed row index is inside its row sequence");
            let nominal_block_size = if row_index == 0 {
                initial_row_used_block_size.unwrap_or(used_block_size)
            } else {
                used_block_size
            };
            let flow_consumed_block_size = if row_index + 1 == rows.row_count() {
                final_row_used_block_size
            } else {
                nominal_block_size
            };
            entries.push(CommittedMulticolFlowEntry::ColumnRow {
                plan,
                source_offset: MulticolRowBlockOffset::new(source_offset),
                nominal_block_size,
                flow_consumed_block_size,
            });
            if row_index + 1 == rows.row_count() {
                source_offset += layout_pt(flow_consumed_block_size.points());
            } else {
                source_offset += layout_pt(nominal_block_size.points());
                source_offset += layout_pt(gap);
            }
        }
        Self {
            entries,
            block_extent: source_offset,
        }
    }

    pub(super) fn block_extent(&self) -> LayoutLength {
        self.block_extent
    }

    pub(super) fn row_plan(&self, row: MulticolumnRowIndex) -> Option<CommittedMulticolRowPlan> {
        self.entries.iter().find_map(|entry| match entry {
            CommittedMulticolFlowEntry::ColumnRow { plan, .. } if plan.row == row => Some(*plan),
            CommittedMulticolFlowEntry::ColumnRow { .. }
            | CommittedMulticolFlowEntry::Spanner { .. } => None,
        })
    }

    pub(super) fn row_geometry(
        &self,
        row: MulticolumnRowIndex,
    ) -> Option<(MulticolRowBlockOffset, UsedMulticolColumnBlockSize)> {
        self.entries.iter().find_map(|entry| match entry {
            CommittedMulticolFlowEntry::ColumnRow {
                plan,
                source_offset,
                nominal_block_size,
                ..
            } if plan.row == row => Some((*source_offset, *nominal_block_size)),
            CommittedMulticolFlowEntry::ColumnRow { .. }
            | CommittedMulticolFlowEntry::Spanner { .. } => None,
        })
    }

    pub(super) fn row_flow_consumed_block_size(
        &self,
        row: MulticolumnRowIndex,
    ) -> Option<UsedMulticolColumnBlockSize> {
        self.entries.iter().find_map(|entry| match entry {
            CommittedMulticolFlowEntry::ColumnRow {
                plan,
                flow_consumed_block_size,
                ..
            } if plan.row == row => Some(*flow_consumed_block_size),
            CommittedMulticolFlowEntry::ColumnRow { .. }
            | CommittedMulticolFlowEntry::Spanner { .. } => None,
        })
    }

    #[cfg(test)]
    fn entries(&self) -> &[CommittedMulticolFlowEntry] {
        &self.entries
    }
}

/// The used content-box block size of an anonymous multicolumn column.
///
/// Unlike an algorithmic fragmentation capacity, this is authored CSS
/// geometry and is therefore allowed to be exactly zero. Keeping it distinct
/// prevents the positive capacity needed for forward progress from leaking
/// into row placement, clipping, gaps, or containing-block geometry.
/// <https://drafts.csswg.org/css-multicol-2/#ch>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct UsedMulticolColumnBlockSize(LogicalBlockContentSize);

impl UsedMulticolColumnBlockSize {
    pub(super) fn new(value: LogicalBlockContentSize) -> Self {
        debug_assert!(
            value.points() >= 0.0,
            "a used CSS block size is non-negative"
        );
        Self(LogicalBlockContentSize::new(content_box_pt(
            value.points().max(0.0),
        )))
    }

    pub(super) fn from_points(value: f32) -> Self {
        Self::new(LogicalBlockContentSize::new(content_box_pt(value.max(0.0))))
    }

    pub(super) fn points(self) -> f32 {
        self.0.points()
    }

    pub(super) fn logical_size(self) -> LogicalBlockContentSize {
        self.0
    }
}

/// Strictly-positive capacity used only by the fragmentation progress loop.
///
/// CSS permits `column-height: 0`, while CSS Fragmentation requires layout to
/// make progress. The one-CSS-pixel fallback belongs in this type alone and
/// must never be used as the anonymous column's CSS geometry.
/// <https://drafts.csswg.org/css-multicol-2/#ch>
/// <https://www.w3.org/TR/css-break-3/#breaking-rules>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FragmentationProgressCapacity(LayoutLength);

impl FragmentationProgressCapacity {
    pub(super) fn for_used_column(size: UsedMulticolColumnBlockSize) -> Self {
        let capacity = layout_pt(size.logical_size().points().max(css::CSS_PX_TO_PT));
        debug_assert!(capacity.points() > 0.0);
        Self(capacity)
    }

    pub(super) fn layout_length(self) -> LayoutLength {
        self.0
    }
}

/// A source-local logical block displacement within a multicol container.
///
/// This is deliberately not interchangeable with a physical page-top
/// position. Writing-mode projection happens only when a committed row is
/// replayed into its destination fragmentainer.
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MulticolRowBlockOffset(LayoutLength);

impl MulticolRowBlockOffset {
    #[allow(dead_code, reason = "used by the staged committed-row planner")]
    pub(super) fn new(value: LayoutLength) -> Self {
        debug_assert!(
            value.points() >= 0.0,
            "a row offset is source-local and non-negative"
        );
        Self(layout_pt(value.points().max(0.0)))
    }

    #[allow(dead_code, reason = "used by the staged committed-row planner")]
    pub(super) fn layout_length(self) -> LayoutLength {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_used_column_size_has_positive_progress_capacity() {
        let used = UsedMulticolColumnBlockSize::from_points(0.0);
        let progress = FragmentationProgressCapacity::for_used_column(used);

        assert_eq!(used.points(), 0.0);
        assert_eq!(progress.layout_length().points(), css::CSS_PX_TO_PT);
    }

    #[test]
    fn positive_used_column_size_is_its_progress_capacity() {
        let used = UsedMulticolColumnBlockSize::from_points(24.0);
        let progress = FragmentationProgressCapacity::for_used_column(used);

        assert_eq!(used.points(), 24.0);
        assert_eq!(progress.layout_length().points(), 24.0);
        assert_eq!(progress.layout_length(), layout_pt(24.0));
        assert_eq!(used.logical_size().points(), 24.0);
    }

    #[test]
    fn parallel_fragment_spans_do_not_create_in_flow_rows() {
        let plan = CommittedMulticolFragmentPlan::new(
            CommittedMulticolColumns::from_temporary_column_pages(1, false),
            3,
        );
        let rows =
            CommittedMulticolRows::new(plan.in_flow_columns(), NonZeroUsize::new(2).unwrap());

        assert_eq!(plan.physical_column_count(), 3);
        assert_eq!(rows.row_count(), 1);
    }

    #[test]
    fn committed_zero_height_rows_advance_only_by_row_gap() {
        let rows = CommittedMulticolRows::new(
            CommittedMulticolColumns::from_temporary_column_pages(6, false),
            NonZeroUsize::new(2).unwrap(),
        );
        let flow = CommittedMulticolFlow::from_column_rows(
            rows,
            UsedMulticolColumnBlockSize::from_points(0.0),
            None,
            UsedMulticolColumnBlockSize::from_points(0.0),
            Some(20.0),
            false,
        );

        assert_eq!(flow.block_extent(), layout_pt(40.0));
        assert_eq!(flow.entries().len(), 3);
        let offsets = flow
            .entries()
            .iter()
            .map(|entry| match entry {
                CommittedMulticolFlowEntry::ColumnRow { source_offset, .. } => {
                    source_offset.layout_length()
                }
                CommittedMulticolFlowEntry::Spanner { .. } => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            offsets,
            vec![layout_pt(0.0), layout_pt(20.0), layout_pt(40.0)]
        );
    }

    #[test]
    fn committed_flow_separates_nominal_and_consumed_final_extents() {
        let rows = CommittedMulticolRows::new(
            CommittedMulticolColumns::from_source_subject_count(6),
            NonZeroUsize::new(2).unwrap(),
        );
        let flow = CommittedMulticolFlow::from_column_rows(
            rows,
            UsedMulticolColumnBlockSize::from_points(20.0),
            Some(UsedMulticolColumnBlockSize::from_points(5.0)),
            UsedMulticolColumnBlockSize::from_points(10.0),
            Some(10.0),
            false,
        );

        assert_eq!(flow.block_extent(), layout_pt(55.0));
        assert_eq!(
            flow.row_geometry(MulticolumnRowIndex::new(0))
                .unwrap()
                .1
                .points(),
            5.0
        );
        assert_eq!(
            flow.row_geometry(MulticolumnRowIndex::new(2))
                .unwrap()
                .1
                .points(),
            20.0
        );
        assert_eq!(
            flow.row_flow_consumed_block_size(MulticolumnRowIndex::new(2))
                .unwrap()
                .points(),
            10.0
        );
    }
}
