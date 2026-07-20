use super::*;

#[derive(Debug, Clone)]
pub(in crate::layout) enum PageNameScope {
    Element,
    Inline { previous_page_name: Option<String> },
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineSplitBlockPaintScope {
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) checkpoint: PaintCheckpoint,
    pub(in crate::layout) positioned_layer_start: usize,
    pub(in crate::layout) source_order: usize,
}
