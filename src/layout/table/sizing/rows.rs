//! Table row-height planning and distribution.

use super::*;
/// CSS Tables 3 row-height plan for first-pass minimums, reference sizes, and
/// final distributed row sizes.
///
/// Spec: <https://drafts.csswg.org/css-tables-3/#row-layout> and
/// <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) enum TableHeightDistributionTarget {
    /// No definite table content-box height constrains row distribution.
    Intrinsic,
    /// A resolved table content-box height constrains row distribution.
    Definite(ContentBoxLength),
}

impl TableHeightDistributionTarget {
    /// The definite content-box height needed by legacy sizing interfaces.
    pub(in crate::layout::table) fn definite_content_height(self) -> Option<ContentBoxLength> {
        match self {
            Self::Intrinsic => None,
            Self::Definite(height) => Some(height),
        }
    }
}

/// Cache representation of [`TableHeightDistributionTarget`].
///
/// This is the only boundary that reduces a semantic content-box length to
/// its scalar bit representation for hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::layout) enum TableHeightDistributionTargetKey {
    Intrinsic,
    Definite(u32),
}

impl From<TableHeightDistributionTarget> for TableHeightDistributionTargetKey {
    fn from(target: TableHeightDistributionTarget) -> Self {
        match target {
            TableHeightDistributionTarget::Intrinsic => Self::Intrinsic,
            TableHeightDistributionTarget::Definite(height) => {
                Self::Definite(height.points().to_bits())
            }
        }
    }
}

#[cfg(test)]
mod table_height_distribution_target_tests {
    use super::*;

    #[test]
    fn cache_key_distinguishes_intrinsic_and_definite_distribution_targets() {
        assert_ne!(
            TableHeightDistributionTargetKey::from(TableHeightDistributionTarget::Intrinsic),
            TableHeightDistributionTargetKey::from(TableHeightDistributionTarget::Definite(
                content_box_pt(75.0),
            )),
        );
    }
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct TableHeightPlan {
    pub(in crate::layout::table) rows: Vec<TableRowHeightPlan>,
    /// Resolved constraint for distributing the table grid's rows.
    ///
    /// This is distinct from the resulting intrinsic grid height: percentage
    /// descendants become definite only for the `Definite` variant.
    pub(in crate::layout::table) target: TableHeightDistributionTarget,
}

/// Per-row state used by `TableHeightPlan`.
///
/// `base` is the ROWMIN-style first-pass size, `reference` includes
/// explicit/percentage row, row-group, and cell constraints, and `final_height`
/// is the size after the CSS Tables 3 distribution algorithm.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout::table) struct TableRowHeightPlan {
    pub(in crate::layout::table) base: f32,
    /// The row's pre-`visibility: collapse` intrinsic block contribution.
    /// Spanning-cell descendants are laid out against these source tracks
    /// before the collapsed tracks are removed from visible painting.
    pub(in crate::layout::table) source_height: f32,
    pub(in crate::layout::table) reference: f32,
    pub(in crate::layout::table) final_height: f32,
    pub(in crate::layout::table) auto: bool,
    pub(in crate::layout::table) collapsed: bool,
}
pub(in crate::layout::table) fn table_plan_height(
    row: &TableRowHeightPlan,
    target: TableHeightTarget,
) -> f32 {
    match target {
        TableHeightTarget::Base => row.base,
        TableHeightTarget::Reference => row.reference,
    }
}

pub(in crate::layout::table) fn table_plan_height_mut(
    row: &mut TableRowHeightPlan,
    target: TableHeightTarget,
) -> &mut f32 {
    match target {
        TableHeightTarget::Base => &mut row.base,
        TableHeightTarget::Reference => &mut row.reference,
    }
}

impl TableHeightPlan {
    pub(in crate::layout::table) fn final_row_heights(&self) -> Vec<f32> {
        self.rows.iter().map(|row| row.final_height).collect()
    }

    pub(in crate::layout::table) fn row_occupancy(&self) -> Vec<bool> {
        self.rows.iter().map(|row| !row.collapsed).collect()
    }

    pub(in crate::layout::table) fn source_row_heights(&self) -> Vec<f32> {
        self.rows.iter().map(|row| row.source_height).collect()
    }
}

pub(in crate::layout::table) fn table_content_height_from_plan(
    rows: &[TableRowHeightPlan],
    target: TableHeightTarget,
    table_metrics: TableMetrics,
) -> f32 {
    let heights = rows
        .iter()
        .map(|row| table_plan_height(row, target))
        .collect::<Vec<_>>();
    let occupancy = rows.iter().map(|row| !row.collapsed).collect::<Vec<_>>();
    table_content_height(&heights, &occupancy, table_metrics)
}

pub(in crate::layout::table) fn table_span_height_from_plan(
    rows: &[TableRowHeightPlan],
    row: usize,
    rowspan: usize,
    target: TableHeightTarget,
    table_metrics: TableMetrics,
) -> f32 {
    let heights = rows
        .iter()
        .map(|row| table_plan_height(row, target))
        .collect::<Vec<_>>();
    let occupancy = rows.iter().map(|row| !row.collapsed).collect::<Vec<_>>();
    table_row_span_height(&heights, &occupancy, row, rowspan, table_metrics)
}

pub(in crate::layout::table) fn distribute_table_span_constraint(
    rows: &mut [TableRowHeightPlan],
    row: usize,
    rowspan: usize,
    required_height: f32,
    table_metrics: TableMetrics,
    target: TableHeightTarget,
) {
    if row >= rows.len() {
        return;
    }
    let current_height = table_span_height_from_plan(rows, row, rowspan, target, table_metrics);
    let extra = required_height - current_height;
    if extra <= 0.01 {
        return;
    }

    let end = (row + rowspan.max(1)).min(rows.len());
    let auto_receivers = (row..end)
        .filter(|index| !rows[*index].collapsed && rows[*index].auto)
        .collect::<Vec<_>>();
    let receivers = if auto_receivers.is_empty() {
        (row..end)
            .filter(|index| !rows[*index].collapsed)
            .collect::<Vec<_>>()
    } else {
        auto_receivers
    };
    if receivers.is_empty() {
        return;
    }

    let share = extra / receivers.len() as f32;
    for index in receivers {
        *table_plan_height_mut(&mut rows[index], target) += share;
    }
}

pub(in crate::layout::table) fn distribute_table_height_extra(
    rows: &mut [TableRowHeightPlan],
    extra: f32,
    predicate: impl Fn(&TableRowHeightPlan) -> bool,
) -> f32 {
    if extra <= 0.01 {
        return 0.0;
    }
    let receivers = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| predicate(row).then_some(index))
        .collect::<Vec<_>>();
    if receivers.is_empty() {
        return 0.0;
    }

    let share = extra / receivers.len() as f32;
    for index in &receivers {
        rows[*index].final_height += share;
    }
    extra
}

/// Return contiguous row-group spans used by table height distribution.
///
/// CSS Tables 3 distributes extra table block size to row groups before rows;
/// anonymous rows without an explicit row-group wrapper still form contiguous
/// distribution groups for the anonymous table objects created by table fixup.
/// <https://drafts.csswg.org/css-tables-3/#height-distribution-algorithm>
pub(in crate::layout::table) fn table_height_distribution_groups(
    rows: &[TableRow<'_>],
) -> Vec<(usize, usize)> {
    let Some(first_row) = rows.first() else {
        return Vec::new();
    };

    let mut groups = Vec::new();
    let mut start = 0;
    let mut current_group = first_row.row_groups.last().map(|group| &group.signature);
    for (index, row) in rows.iter().enumerate().skip(1) {
        let group = row.row_groups.last().map(|group| &group.signature);
        if group != current_group {
            groups.push((start, index));
            start = index;
            current_group = group;
        }
    }
    groups.push((start, rows.len()));
    groups
}

#[derive(Clone, Copy)]
pub(in crate::layout::table) enum TableHeightTarget {
    Base,
    Reference,
}
