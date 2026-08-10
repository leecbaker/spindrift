use super::*;
/// Return the inline-end hanging width for `hanging-punctuation: last`.
///
/// CSS Text says a closing bracket or quote at the end of the last formatted
/// line can hang, and non-zero inline-axis padding or border between the glyph
/// and the line edge prevents hanging:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(in crate::layout) fn last_hanging_punctuation_width(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
) -> f32 {
    hanging_punctuation_widths(font_system, fragments, block_style, false, true, false).end
}

/// Return start/end hanging punctuation advances for one line.
///
/// CSS Text excludes at most one hangable glyph at each line edge from line
/// measurement. `first` affects only the first formatted line, `last` only the
/// last formatted line, `force-end` affects every line end, and `allow-end`
/// conditionally hangs only when the line would otherwise overflow:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(in crate::layout) fn hanging_punctuation_widths(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
    is_first_line: bool,
    is_last_line: bool,
    line_overflows: bool,
) -> HangingPunctuationWidths {
    HangingPunctuationWidths {
        start: first_hanging_punctuation_width(font_system, fragments, block_style, is_first_line),
        end: end_hanging_punctuation_width(
            font_system,
            fragments,
            block_style,
            is_last_line,
            line_overflows,
        ),
    }
}

pub(in crate::layout) fn first_hanging_punctuation_width(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
    is_first_line: bool,
) -> f32 {
    let fragment = fragments
        .iter()
        .find(|fragment| !trim_css_collapsible_whitespace(fragment.text()).is_empty());
    first_hanging_punctuation_width_for_fragment(font_system, fragment, block_style, is_first_line)
}

pub(in crate::layout) fn first_hanging_punctuation_width_for_fragment(
    font_system: &mut FontSystem,
    fragment: Option<&InlineFragment>,
    block_style: &ComputedStyle,
    is_first_line: bool,
) -> f32 {
    if !block_style.hanging_punctuation.first || !is_first_line {
        return 0.0;
    }
    let Some(fragment) = fragment else {
        return 0.0;
    };
    let Some(character) = trim_start_css_collapsible_whitespace(fragment.text())
        .chars()
        .next()
    else {
        return 0.0;
    };
    if !character_is_first_hangable_punctuation(character) {
        return 0.0;
    }
    if fragment.hanging_edges().blocks_start {
        return 0.0;
    }
    font_system.measure_text(&character.to_string(), fragment.style())
}

pub(in crate::layout) fn end_hanging_punctuation_width(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
    is_last_line: bool,
    line_overflows: bool,
) -> f32 {
    let fragment = fragments
        .iter()
        .rev()
        .find(|fragment| !trim_css_collapsible_whitespace(fragment.text()).is_empty());
    end_hanging_punctuation_width_for_fragment(
        font_system,
        fragment,
        block_style,
        is_last_line,
        line_overflows,
    )
    .points()
}

pub(in crate::layout) fn end_hanging_punctuation_width_for_fragment(
    font_system: &mut FontSystem,
    fragment: Option<&InlineFragment>,
    block_style: &ComputedStyle,
    is_last_line: bool,
    line_overflows: bool,
) -> LayoutLength {
    let Some(fragment) = fragment else {
        return layout_pt(0.0);
    };
    let Some(character) = trim_end_css_collapsible_whitespace(fragment.text())
        .chars()
        .next_back()
    else {
        return layout_pt(0.0);
    };
    let hangs_by_last = block_style.hanging_punctuation.last && is_last_line;
    let hangs_by_force_end =
        block_style.hanging_punctuation.force_end && character_is_hangable_stop_or_comma(character);
    let hangs_by_allow_end = block_style.hanging_punctuation.allow_end
        && line_overflows
        && character_is_hangable_stop_or_comma(character);
    if !(hangs_by_last && character_is_last_hangable_punctuation(character)
        || hangs_by_force_end
        || hangs_by_allow_end)
    {
        return layout_pt(0.0);
    }
    if fragment.hanging_edges().blocks_end {
        return layout_pt(0.0);
    }
    intrinsic::hanging_punctuation_character_width(font_system, character, fragment.style())
}

pub(in crate::layout) fn last_hanging_punctuation_width_for_line_items<T>(
    font_system: &mut FontSystem,
    items: &[T],
    block_style: &ComputedStyle,
) -> LayoutLength
where
    T: AsRef<InlineLineItem>,
{
    end_hanging_punctuation_width_for_line_items(font_system, items, block_style, true, false)
}

/// Return the inline-end hanging punctuation width for mixed inline items.
///
/// CSS Text applies the same hanging punctuation eligibility to inline text
/// even when that text is split across inline boxes and atomic inline items:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property> and
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::layout) fn end_hanging_punctuation_width_for_line_items<T>(
    font_system: &mut FontSystem,
    items: &[T],
    block_style: &ComputedStyle,
    is_last_line: bool,
    line_overflows: bool,
) -> LayoutLength
where
    T: AsRef<InlineLineItem>,
{
    let mut fragment = None;
    for item in items.iter().rev() {
        match item.as_ref() {
            InlineLineItem::Fragment(candidate)
                if !trim_css_collapsible_whitespace(candidate.text()).is_empty() =>
            {
                fragment = Some(candidate);
                break;
            }
            InlineLineItem::Atom(atom) if atom.content().is_box_edge() => break,
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {}
        }
    }
    end_hanging_punctuation_width_for_fragment(
        font_system,
        fragment,
        block_style,
        is_last_line,
        line_overflows,
    )
}

pub(in crate::layout) fn hanging_punctuation_widths_for_line_items<T>(
    font_system: &mut FontSystem,
    items: &[T],
    block_style: &ComputedStyle,
    is_first_line: bool,
    is_last_line: bool,
    line_overflows: bool,
) -> HangingPunctuationWidths
where
    T: AsRef<InlineLineItem>,
{
    let first_fragment = items.iter().find_map(|item| match item.as_ref() {
        InlineLineItem::Fragment(fragment)
            if !trim_css_collapsible_whitespace(fragment.text()).is_empty() =>
        {
            Some(fragment)
        }
        InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
    });
    let last_fragment = items.iter().rev().find_map(|item| match item.as_ref() {
        InlineLineItem::Fragment(fragment)
            if !trim_css_collapsible_whitespace(fragment.text()).is_empty() =>
        {
            Some(fragment)
        }
        InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
    });
    HangingPunctuationWidths {
        start: first_hanging_punctuation_width_for_fragment(
            font_system,
            first_fragment,
            block_style,
            is_first_line,
        ),
        end: end_hanging_punctuation_width_for_fragment(
            font_system,
            last_fragment,
            block_style,
            is_last_line,
            line_overflows,
        )
        .points(),
    }
}

pub(in crate::layout) fn last_hanging_punctuation_width_for_inline_items(
    font_system: &mut FontSystem,
    items: &[InlineItem],
    block_style: &ComputedStyle,
) -> f32 {
    if !block_style.hanging_punctuation.last {
        return 0.0;
    }
    let mut word = None;
    for item in items.iter().rev() {
        match item {
            InlineItem::Word(candidate)
                if !trim_css_collapsible_whitespace(&candidate.text).is_empty() =>
            {
                word = Some(candidate);
                break;
            }
            InlineItem::Atom(atom) if atom.content().is_box_edge() => break,
            InlineItem::Word(_)
            | InlineItem::Atom(_)
            | InlineItem::Float(_)
            | InlineItem::Break(_)
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => {}
        }
    }
    let Some(word) = word else {
        return 0.0;
    };
    last_hanging_punctuation_width(
        font_system,
        std::slice::from_ref(&InlineFragment::new_shared_style(
            transform_text(&word.text, &word.style),
            Rc::clone(&word.style),
            word.baseline_shift,
            word.link_target.clone(),
            word.mergeable,
            word.source,
            false,
            word.hanging_edges,
            Rc::clone(&word.ancestor_inline_decorations),
        )),
        block_style,
    )
}

/// Return whether paint-time line alignment should exclude hanging punctuation.
///
/// CSS Text excludes hanging punctuation from line measurement. For
/// shrink-to-fit boxes with `width: auto`, intrinsic sizing has already
/// resolved that exclusion into the used inline size, so subtracting it again
/// during alignment double-applies the adjustment:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic>.
pub(in crate::layout) fn line_box_uses_hanging_punctuation_alignment(
    style: &ComputedStyle,
) -> bool {
    !style.box_values.width.is_auto()
}
