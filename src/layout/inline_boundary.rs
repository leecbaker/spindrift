use super::*;

/// The CSS Text role of a collected inline stream item at a non-text boundary.
///
/// CSS Text whitespace processing and line construction both need to know
/// whether a boundary is transparent to adjacent text, terminates text
/// context, or creates a forced line boundary:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineBoundaryRole {
    Text,
    TransparentTextBoundary,
    OpaqueAtomic,
    IndependentFormattingContext,
    Float,
    ForcedBreak,
    PageScopeStart,
    PageScopeEnd,
}

impl InlineBoundaryRole {
    pub(in crate::layout) fn is_transparent_to_whitespace(self) -> bool {
        matches!(
            self,
            Self::TransparentTextBoundary | Self::PageScopeStart | Self::PageScopeEnd
        )
    }

    pub(in crate::layout) fn resets_text_context(self) -> bool {
        matches!(
            self,
            Self::OpaqueAtomic | Self::IndependentFormattingContext | Self::Float
        )
    }

    pub(in crate::layout) fn is_page_scope(self) -> bool {
        matches!(self, Self::PageScopeStart | Self::PageScopeEnd)
    }
}

pub(in crate::layout) fn inline_item_boundary_role(item: &InlineItem) -> InlineBoundaryRole {
    match item {
        InlineItem::Word(word) if word.text.chars().all(character_is_bidi_format_control) => {
            InlineBoundaryRole::TransparentTextBoundary
        }
        InlineItem::Word(_) => InlineBoundaryRole::Text,
        InlineItem::Atom(atom) => inline_atom_boundary_role(atom.content()),
        InlineItem::Float(_) => InlineBoundaryRole::Float,
        InlineItem::Break(_) => InlineBoundaryRole::ForcedBreak,
        InlineItem::PageScopeStart(_) => InlineBoundaryRole::PageScopeStart,
        InlineItem::PageScopeEnd => InlineBoundaryRole::PageScopeEnd,
    }
}

pub(in crate::layout) fn inline_atom_boundary_role(
    content: &InlineAtomContent,
) -> InlineBoundaryRole {
    match content {
        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)) => {
            InlineBoundaryRole::TransparentTextBoundary
        }
        InlineAtomContent::InlineBox { .. } | InlineAtomContent::InlineFragment(_) => {
            InlineBoundaryRole::IndependentFormattingContext
        }
        InlineAtomContent::Canvas
        | InlineAtomContent::Image(_)
        | InlineAtomContent::Svg { .. }
        | InlineAtomContent::StaticPositionPlaceholder
        | InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace)
        | InlineAtomContent::Leader(_) => InlineBoundaryRole::OpaqueAtomic,
    }
}
