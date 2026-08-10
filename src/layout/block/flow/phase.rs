use super::*;

pub(in crate::layout) struct BlockFlowChildrenPhaseInput<'a, 'boxes> {
    pub(in crate::layout) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout) element: &'a Element,
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) stylesheets: &'a Stylesheets<'a>,
    pub(in crate::layout) child_boxes: Option<&'a [box_tree::FormattingBox<'boxes>]>,
    pub(in crate::layout) can_collapse_start_margin: bool,
    pub(in crate::layout) can_collapse_end_margin: bool,
    pub(in crate::layout) applied_start_margin: LayoutLength,
    /// Whether a cleared parent's applied start margin already includes its
    /// first adjoining in-flow descendant's start-margin contribution.
    pub(in crate::layout) clearance_consumed_adjoining_start_margin: bool,
    pub(in crate::layout) starts_at_page_top: bool,
    pub(in crate::layout) laid_out_column_children: bool,
    pub(in crate::layout) use_box_inline_items: bool,
    /// Whether the target block already incorporated its normalized run-in
    /// prelude and inline children into one line-item sequence.
    pub(in crate::layout) run_in_inline_items_laid_out: bool,
    pub(in crate::layout) use_ordered_mixed_flow: bool,
    /// Whether a preceding direct inline run establishes the source side of
    /// the first class-A child boundary.
    pub(in crate::layout) has_preceding_inline_flow_content: bool,
    /// A direct inline sequence already selected an automatic clamp point or
    /// captured local discard break. Later in-flow block source is beyond the
    /// same cutoff and must not enter ordinary traversal.
    pub(in crate::layout) preceding_inline_local_cutoff: bool,
    /// Maximum number of local multicol regions which this traversal may
    /// enter for `continue: discard`. This is not a page/column fragmentainer
    /// limit; it only decides when later source is omitted.
    pub(in crate::layout) discard_region_limit: Option<super::children::state::DiscardRegionLimit>,
    /// A finite absolute/font-relative automatic clamp constraint resolved at
    /// this owning block. Descendant traversal carries its remaining portion
    /// as a layout-only controller; percentage constraints resolve later.
    pub(in crate::layout) direct_automatic_block_size_constraint:
        Option<crate::units::ContentBoxLength>,
    pub(in crate::layout) definite_content_height: Option<f32>,
    pub(in crate::layout) descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct BlockFlowChildrenPhaseOutcome {
    pub(in crate::layout) pending_end_margin_collapse: Option<BlockEndMarginCollapse>,
    pub(in crate::layout) collapsed_start_margin_offset: LayoutLength,
    /// Geometry of the rendered legend selected by an HTML fieldset child
    /// traversal. It crosses this phase boundary unchanged so only the
    /// fieldset's own decoration pass can consume it.
    pub(in crate::layout) rendered_legend: Option<super::children::state::RenderedLegendGeometry>,
    /// Slots exported by nested block formatting contexts. Direct inline runs
    /// are retained by the enclosing block's capture instead.
    pub(in crate::layout) descendant_clamp_line_slots: usize,
    /// A descendant captured a local automatic/discard continuation cutoff.
    pub(in crate::layout) has_local_continuation_cutoff: bool,
    /// Retained direct-child source before the first local discard break.
    /// This is intentionally a source endpoint, never a page/column index.
    pub(in crate::layout) discard_source_prefix:
        Option<super::children::state::DiscardSourcePrefix>,
}
