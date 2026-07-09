mod graph;
mod items;
mod mixed;
mod paint_mixed;
mod paragraph;

pub(in crate::layout) use graph::{
    InlineIntrinsicContribution, InlineIntrinsicMeasurement, InlineMeasuredParagraph,
    build_inline_opportunity_graph,
};
pub(in crate::layout) use items::{
    InlineLayoutOutcome, InlineLineRecord, InlineLineSequence, MulticolumnInlinePaintGeometry,
    inline_line_fragment_is_phantom,
};

#[cfg(test)]
pub(in crate::layout) use graph::MeasuredInlineItem;
#[cfg(test)]
pub(in crate::layout) use graph::{
    InlineBreakKind, InlineGraphPosition, InlineGraphRange, InlineLineEdgeEffectKind,
    InlineLineFragment,
};
#[cfg(test)]
pub(in crate::layout) use mixed::{
    RangedMeasuredMixedInlineLineItem, split_mixed_inline_visual_ranges_at_transparent_inline_edges,
};
