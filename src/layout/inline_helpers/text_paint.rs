use super::*;

/// Return whether an inline fragment needs glyph or decoration paint.
///
/// CSS CssColor defines alpha as part of the used color. Fully transparent text
/// still participates in layout and can have backgrounds, but an explicit
/// visible text decoration or text shadow still needs the glyph outline as a
/// paint source:
/// <https://www.w3.org/TR/css-color-4/#alpha-value> and
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
pub(in crate::layout) fn inline_fragment_has_visible_text_paint(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    fragment.style().color.is_visible()
        || fragment
            .style()
            .text_decoration_origins
            .effective_layers()
            .any(|layer| {
                layer.decoration.has_visible_line()
                    && layer
                        .decoration
                        .color
                        .unwrap_or(fragment.style().color)
                        .is_visible()
            })
        || fragment.style().text_shadow.iter().any(|shadow| {
            !shadow.inset && shadow.color.resolve(fragment.style().color).is_visible()
        })
}

/// Return whether adjacent inline text fragments can share one painted line.
///
/// CSS Inline Layout creates line boxes from adjacent inline boxes, while PDF
/// text emission can keep distinct font runs inside one text object when the
/// shared line-level paint state is compatible:
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::layout) fn can_paint_inline_fragments_together(
    left: &(impl InlineFragmentAccess + ?Sized),
    right: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    let shares_first_letter_group = left
        .first_letter_pseudo_group_id()
        .zip(right.first_letter_pseudo_group_id())
        .is_some_and(|(left, right)| left == right);
    (left.mergeable() && right.mergeable() || shares_first_letter_group)
        && inline_text_sources_are_paint_compatible(left.source(), right.source())
        && (left.baseline_shift() - right.baseline_shift()).abs() < 0.01
        && left.visual_offset() == right.visual_offset()
        && left.link_target() == right.link_target()
        && (left.style().font_size - right.style().font_size).abs() < 0.01
        && left.style().vertical_align == right.style().vertical_align
        && left.style().color == right.style().color
        // Palette selection is paint state. Adjacent palette-only inline
        // boxes may share shaping context, but must not collapse into one
        // rendered text run because a COLR glyph reads the palette at paint.
        // <https://drafts.csswg.org/css-fonts-4/#font-palette-prop>
        && left.style().font_palette == right.style().font_palette
        && left.style().visibility == right.style().visibility
        // Opacity establishes a stacking context. Adjacent text cannot share
        // one prepared paint group across that compositing boundary even when
        // their shaping state is otherwise identical.
        // <https://www.w3.org/TR/css-color-4/#transparency>
        && left.style().opacity == right.style().opacity
        && left.style().text_decoration == right.style().text_decoration
        && inline_ancestor_decorations_have_same_text_paint_effect(
            left.ancestor_inline_decorations(),
            right.ancestor_inline_decorations(),
        )
}

pub(in crate::layout) fn inline_text_sources_are_paint_compatible(
    left: InlineTextSource,
    right: InlineTextSource,
) -> bool {
    match (left, right) {
        (InlineTextSource::FootnoteCall(left), InlineTextSource::FootnoteCall(right)) => {
            left == right
        }
        (InlineTextSource::FootnoteCall(_), _) | (_, InlineTextSource::FootnoteCall(_)) => false,
        (InlineTextSource::BidiControl, InlineTextSource::BidiControl) => true,
        (InlineTextSource::BidiControl, _) | (_, InlineTextSource::BidiControl) => false,
        (InlineTextSource::BlockEllipsis, InlineTextSource::BlockEllipsis) => true,
        (InlineTextSource::BlockEllipsis, _) | (_, InlineTextSource::BlockEllipsis) => false,
        // A transformed separator remains a distinct layout source, but it
        // belongs to the same text paint group as compatible neighboring
        // text. The group-level ActualText mapping restores its authored
        // U+200B (or omits generated `<wbr>`) without splitting the glyph
        // stream at the replacement character.
        // <https://drafts.csswg.org/css-text-4/#word-space-transform>
        (InlineTextSource::WordSpaceTransform(_), InlineTextSource::RunIn)
        | (InlineTextSource::RunIn, InlineTextSource::WordSpaceTransform(_)) => false,
        (InlineTextSource::WordSpaceTransform(_), _)
        | (_, InlineTextSource::WordSpaceTransform(_)) => true,
        (InlineTextSource::Marker, InlineTextSource::Marker) => true,
        (InlineTextSource::Marker, _) | (_, InlineTextSource::Marker) => false,
        (InlineTextSource::RunIn, InlineTextSource::RunIn) => true,
        (InlineTextSource::RunIn, _) | (_, InlineTextSource::RunIn) => false,
        (
            InlineTextSource::Normal | InlineTextSource::Generated | InlineTextSource::GeneratedWbr,
            _,
        ) => true,
    }
}
