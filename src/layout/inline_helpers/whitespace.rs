use super::*;

pub(in crate::layout) fn char_boundary_slice(
    text: &str,
    range: std::ops::Range<usize>,
) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let start = previous_char_boundary(text, range.start.min(text.len()));
    let end = next_char_boundary(text, range.end.min(text.len()));
    (start < end).then(|| text[start..end].to_string())
}

pub(in crate::layout) fn previous_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

pub(in crate::layout) fn next_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub(in crate::layout) fn inline_item_is_collapsible_space<T>(item: &T) -> bool
where
    T: AsRef<InlineItem> + ?Sized,
{
    matches!(
        item.as_ref(),
        InlineItem::Word(word)
            if word.style.white_space.collapses_spaces()
                && word.text.chars().all(is_css_collapsible_whitespace)
    )
}

pub(in crate::layout) fn trim_inline_item_edges<T>(items: &mut Vec<T>)
where
    T: AsRef<InlineItem>,
{
    let first_kept = items
        .iter()
        .position(|item| !inline_item_is_collapsible_space(item));
    match first_kept {
        Some(0) => {}
        Some(index) => {
            items.drain(..index);
        }
        None => {
            items.clear();
            return;
        }
    }
    trim_trailing_inline_spaces(items);
}

pub(in crate::layout) fn trim_trailing_inline_spaces<T>(items: &mut Vec<T>)
where
    T: AsRef<InlineItem>,
{
    while items.last().is_some_and(inline_item_is_collapsible_space) {
        items.pop();
    }
}

pub(in crate::layout) fn inline_line_item_is_collapsible_space(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Fragment(fragment)
            if inline_fragment_is_collapsible_space(fragment)
    )
}

pub(in crate::layout) fn inline_fragment_is_collapsible_space(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    fragment.style().white_space.collapses_spaces()
        && fragment.text().chars().all(is_css_collapsible_whitespace)
}

/// Return whether a line item is a `pre-wrap` space run that can hang.
///
/// CSS Text phase II makes preserved spaces at the end of a soft-wrapped
/// `pre-wrap` line hang, while `break-spaces` explicitly keeps such spaces
/// from hanging:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(in crate::layout) fn inline_line_item_is_pre_wrap_hanging_space(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Fragment(fragment)
            if inline_fragment_is_pre_wrap_hanging_space(fragment)
    )
}

/// Return whether a fragment is a preserved `pre-wrap` edge-space run.
///
/// CSS Text phase II lets preserved spaces at the end of a soft-wrapped
/// `pre-wrap` line hang. Keeping this predicate at the fragment level lets
/// line construction and justification agree on which trailing space advances
/// are outside the formatted line measure:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(in crate::layout) fn inline_fragment_is_pre_wrap_hanging_space(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    fragment.style().white_space == WhiteSpace::PreWrap
        && fragment.text().chars().all(is_css_preserved_document_space)
}

pub(in crate::layout) fn trailing_hanging_space_separator_width_for_line_items<T>(
    line: &[T],
    font_system: &mut FontSystem,
) -> f32
where
    T: AsRef<InlineLineItem>,
{
    let mut width = 0.0;
    let mut follows_hanging_separator = false;
    for item in line.iter().rev() {
        let fragment = match item.as_ref() {
            // Regular inline box edges are decoration ownership markers, not
            // textual line content. CSS Text Phase II finds a trailing
            // space-separator sequence through nested inline boxes.
            // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
            InlineLineItem::Atom(atom)
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                ) =>
            {
                continue;
            }
            InlineLineItem::Fragment(fragment) => fragment,
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => break,
        };
        if fragment.text().is_empty() {
            continue;
        }
        for character in fragment.text().chars().rev() {
            // CSS Text Phase II first removes a collapsible document-space
            // suffix. That removal exposes a preceding other-space separator
            // sequence as the visual line edge, so it must not stop this
            // reverse scan or be charged to the hanging sequence itself.
            // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
            if fragment.style().white_space.collapses_spaces()
                && is_css_collapsible_whitespace(character)
                && !follows_hanging_separator
            {
                continue;
            }
            if character_is_css_other_space_separator(character)
                && fragment
                    .style()
                    .white_space
                    .hangs_trailing_space_separators()
            {
                width += font_system.measure_text(&character.to_string(), fragment.style());
                follows_hanging_separator = true;
                continue;
            }
            // A selected legacy line edge is one whitespace sequence, not a
            // sequence of independently trimmed runs. An interleaved document
            // space belongs to the unconditional hanging sequence once a
            // following other space separator has selected that sequence.
            // `pre-wrap` has a distinct conditional hanging effect, so it
            // accounts for its preserved document-space advances separately.
            // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
            if fragment.style().white_space == WhiteSpace::PreWrap
                && fragment
                    .style()
                    .white_space
                    .hangs_trailing_space_separators()
                && is_css_preserved_document_space(character)
            {
                continue;
            }
            if fragment.style().white_space != WhiteSpace::PreWrap
                && fragment
                    .style()
                    .white_space
                    .hangs_trailing_space_separators()
                && follows_hanging_separator
                && is_css_preserved_document_space(character)
            {
                width += font_system.measure_text(&character.to_string(), fragment.style());
                continue;
            }
            return width;
        }
    }
    width
}
