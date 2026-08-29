use super::*;

/// Selects the one child-source traversal that owns a block's direct
/// descendants.
///
/// A float is out of normal flow, but a direct float still has to be emitted
/// by its parent's block-child traversal when no in-flow inline content
/// precedes it. Keeping that case distinct from an inline sequence prevents a
/// floated box's *own* descendant text from manufacturing an anonymous line
/// box in its parent.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum ChildTraversalMode {
    /// Normalized inline source owns the direct descendants, including floats
    /// that occur beside actual parent inline content.
    InlineSequence,
    /// Direct floats are the only non-phantom child source and are emitted by
    /// ordinary block-child traversal at the parent's current block position.
    DirectFloatChildren,
    /// Raw DOM source must preserve the interleaving of inline and block
    /// children.
    OrderedMixed,
    /// Ordinary block-child traversal owns the descendants.
    BlockChildren,
}

pub(in crate::layout) struct BlockFlowChildrenPhaseInput<'a, 'boxes> {
    pub(in crate::layout) fragmentainer_kind: FragmentainerKind,
    pub(in crate::layout) element: &'a Element,
    pub(in crate::layout) style: &'a ComputedStyle,
    pub(in crate::layout) stylesheets: &'a Stylesheets<'a>,
    pub(in crate::layout) child_boxes: Option<&'a [box_tree::FormattingBox<'boxes>]>,
    pub(in crate::layout) can_collapse_start_margin: bool,
    pub(in crate::layout) can_collapse_end_margin: bool,
    /// CSS2's used block-start margin arrangement for this block. Clearance
    /// keeps the adjusted margin separate from adjoining parent/child sets.
    pub(in crate::layout) start_margin_arrangement: BlockStartMarginArrangement,
    pub(in crate::layout) starts_at_page_top: bool,
    pub(in crate::layout) laid_out_column_children: bool,
    pub(in crate::layout) traversal_mode: ChildTraversalMode,
    /// Whether the target block already incorporated its normalized run-in
    /// prelude and inline children into one line-item sequence.
    pub(in crate::layout) run_in_inline_items_laid_out: bool,
    /// Whether a preceding direct inline run establishes the source side of
    /// the first class-A child boundary.
    pub(in crate::layout) has_preceding_inline_flow_content: bool,
    /// A direct inline sequence already selected an automatic clamp point or
    /// captured local discard break. Later in-flow block source is beyond the
    /// same cutoff and must not enter ordinary traversal.
    pub(in crate::layout) preceding_inline_local_cutoff: bool,
    /// Used block-axis contribution of the direct inline source that precedes
    /// this block-child traversal.  Automatic clamp selection is expressed in
    /// the owning content-box coordinate system, so this must be debited
    /// before the first block child establishes a possible clamp point.
    ///
    /// <https://drafts.csswg.org/css-overflow-4/#line-clamp-containers>
    pub(in crate::layout) preceding_inline_clamp_block_advance: crate::units::ContentBoxLength,
    /// Maximum number of local multicol regions which this traversal may
    /// enter for `continue: discard`. This is not a page/column fragmentainer
    /// limit; it only decides when later source is omitted.
    pub(in crate::layout) discard_region_limit: Option<super::children::state::DiscardRegionLimit>,
    /// A finite absolute/font-relative automatic clamp constraint resolved at
    /// this owning block. Descendant traversal carries its remaining portion
    /// as a layout-only controller; percentage constraints resolve later.
    pub(in crate::layout) direct_automatic_block_size_constraint:
        Option<crate::units::ContentBoxLength>,
    pub(in crate::layout) descendant_percentage_height_context: DescendantBlockPercentageContext,
}

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct BlockFlowChildrenPhaseOutcome {
    pub(in crate::layout) pending_end_margin_collapse: Option<BlockEndMarginCollapse>,
    pub(in crate::layout) collapsed_start_margin_offset: LayoutLength,
    /// Dynamic separation of the traversed adjoining-margin set by actual
    /// CSS2 clearance in a normal-flow descendant.
    pub(in crate::layout) adjoining_margin_set_boundary: BlockMarginCollapseBoundary,
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
