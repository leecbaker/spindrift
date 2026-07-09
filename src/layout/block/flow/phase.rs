use super::*;

pub(in crate::layout) struct BlockFlowChildrenPhaseInput<'a, 'boxes> {
    pub(in crate::layout) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout) element: &'a Element,
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) stylesheets: &'a [Stylesheet],
    pub(in crate::layout) child_boxes: Option<&'a [box_tree::FormattingBox<'boxes>]>,
    pub(in crate::layout) can_collapse_start_margin: bool,
    pub(in crate::layout) can_collapse_end_margin: bool,
    pub(in crate::layout) applied_start_margin: LayoutLength,
    pub(in crate::layout) starts_at_page_top: bool,
    pub(in crate::layout) laid_out_column_children: bool,
    pub(in crate::layout) use_box_inline_items: bool,
    pub(in crate::layout) use_ordered_mixed_flow: bool,
    pub(in crate::layout) definite_content_height: Option<f32>,
    pub(in crate::layout) descendant_percentage_height_basis: Option<BlockSizePercentageBasis>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct BlockFlowChildrenPhaseOutcome {
    pub(in crate::layout) pending_end_margin_collapse: Option<BlockEndMarginCollapse>,
    pub(in crate::layout) collapsed_start_margin_offset: LayoutLength,
    /// Slots exported by nested block formatting contexts. Direct inline runs
    /// are retained by the enclosing block's capture instead.
    pub(in crate::layout) descendant_clamp_line_slots: usize,
}
