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
        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
        | InlineAtomContent::InlineEdge(InlineEdgeRole::MetricsOnlyStrut)
        | InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(_))
        | InlineAtomContent::StaticPositionPlaceholder(_) => {
            // Out-of-flow boxes retain a zero-size placeholder for static
            // positioning, but CSS Text processes the surrounding source as
            // one text sequence. The placeholder must not create a text
            // context reset or a soft-wrap boundary.
            // A text-autospace adjustment is likewise a non-text boundary
            // effect: it never supplies UAX #14's atomic-object input or
            // resets the source text context.
            // <https://drafts.csswg.org/css-text-3/#line-break-details> and
            // <https://drafts.csswg.org/css-text-4/#text-autospace-property>
            InlineBoundaryRole::TransparentTextBoundary
        }
        InlineAtomContent::InlineBox { .. }
        | InlineAtomContent::Ruby { .. }
        | InlineAtomContent::TextCombineUpright { .. }
        | InlineAtomContent::InlineFragment { .. } => {
            InlineBoundaryRole::IndependentFormattingContext
        }
        InlineAtomContent::Canvas
        | InlineAtomContent::Iframe(_)
        | InlineAtomContent::Image(_)
        | InlineAtomContent::Gradient { .. }
        | InlineAtomContent::Svg { .. }
        | InlineAtomContent::Leader(_) => InlineBoundaryRole::OpaqueAtomic,
    }
}
