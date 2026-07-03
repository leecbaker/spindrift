use super::*;

#[derive(Debug, Clone)]
pub(in crate::layout) struct PageNameScope {
    pub(in crate::layout) end_page_name: Option<String>,
    pub(in crate::layout) start_page_count: usize,
    pub(in crate::layout) start_page_has_content: bool,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineSplitBlockPaintScope {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) checkpoint: PaintCheckpoint,
    pub(in crate::layout) positioned_layer_start: usize,
    pub(in crate::layout) source_order: usize,
}
