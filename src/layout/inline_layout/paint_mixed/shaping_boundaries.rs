use super::*;

pub(in crate::layout) fn inline_atom_content_preserves_adjacent_space_summary(
    content: &InlineAtomContent,
) -> bool {
    matches!(
        content,
        InlineAtomContent::Canvas
            | InlineAtomContent::Iframe(_)
            | InlineAtomContent::Image(_)
            | InlineAtomContent::Gradient { .. }
            | InlineAtomContent::Svg { .. }
            | InlineAtomContent::InlineBox { .. }
            | InlineAtomContent::TextCombineUpright { .. }
            | InlineAtomContent::InlineFragment { .. }
            | InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
    )
}

pub(in crate::layout) fn pending_inline_fragments_are_collapsible_space(
    fragments: &[impl InlineFragmentAccess],
) -> bool {
    !fragments.is_empty()
        && fragments.iter().all(|fragment| {
            fragment.style().white_space.collapses_spaces()
                && fragment.text().chars().all(is_css_collapsible_whitespace)
        })
}

pub(in crate::layout) fn pending_inline_fragments_are_join_control_only(
    fragments: &[impl InlineFragmentAccess],
) -> bool {
    !fragments.is_empty()
        && fragments
            .iter()
            .all(|fragment| inline_fragment_is_join_control_only(fragment))
}

pub(in crate::layout) fn inline_fragment_can_append_collapsible_space(
    previous: &(impl InlineFragmentAccess + ?Sized),
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    inline_fragment_is_collapsible_space(fragment)
        && inline_text_sources_are_paint_compatible(previous.source(), fragment.source())
        && previous.link_target() == fragment.link_target()
        && (previous.style().font_size - fragment.style().font_size).abs() < 0.01
        && previous.style().vertical_align == fragment.style().vertical_align
        && previous.style().visibility == fragment.style().visibility
}

/// Return whether an inline box edge is transparent to a pending text shaping
/// group.
///
/// A plain inline element contributes zero-width start/end edges. CSS Text
/// still shapes across those edges, even when its child has a distinct paint
/// style. Used inline-axis decoration and bidi isolation are the exceptions:
/// they form an actual typographic boundary.
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>
pub(super) fn inline_atom_preserves_pending_text_shaping(atom: &InlineAtom) -> bool {
    matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
        if edge.advance == 0.0
            && edge.paint_extent == 0.0
            && !inline_box_edge_fragment_breaks_shaping(atom.style(), *edge)
            && !inline_box_bidi_isolation_breaks_shaping(atom.style()))
}
