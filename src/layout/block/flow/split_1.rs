use super::*;

mod block_layout;
mod children;
mod split_2;

pub(in crate::layout) struct BlockFlowChildrenPhaseInput<'a, 'boxes> {
    pub(in crate::layout) element: &'a Element,
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) stylesheets: &'a [Stylesheet],
    pub(in crate::layout) child_boxes: Option<&'a [box_tree::FormattingBox<'boxes>]>,
    pub(in crate::layout) can_collapse_start_margin: bool,
    pub(in crate::layout) can_collapse_end_margin: bool,
    pub(in crate::layout) applied_start_margin: f32,
    pub(in crate::layout) starts_at_page_top: bool,
    pub(in crate::layout) laid_out_column_children: bool,
    pub(in crate::layout) use_box_inline_items: bool,
    pub(in crate::layout) use_ordered_mixed_flow: bool,
    pub(in crate::layout) definite_content_height: Option<f32>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct BlockFlowChildrenPhaseOutcome {
    pub(in crate::layout) pending_end_margin_collapse: Option<BlockEndMarginCollapse>,
    pub(in crate::layout) collapsed_start_margin_offset: f32,
}
