use super::*;

/// Return whether an inline fragment needs glyph or decoration paint.
///
/// CSS Color defines alpha as part of the used color. Fully transparent text
/// still participates in layout and can have backgrounds, but emits no visible
/// glyph paint unless an explicit visible text-decoration color is present:
/// <https://www.w3.org/TR/css-color-4/#alpha-value> and
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
pub(in crate::layout) fn inline_fragment_has_visible_text_paint(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    fragment.style().color.is_visible()
        || (fragment.style().text_decoration.has_visible_line()
            && fragment
                .style()
                .text_decoration
                .color
                .unwrap_or(fragment.style().color)
                .is_visible())
}

pub(in crate::layout) fn justifiable_fragment_space_count<F: InlineFragmentAccess>(
    fragments: &[F],
) -> usize {
    let mut end = fragments.len();
    while end > 0 && inline_fragment_is_pre_wrap_hanging_space(&fragments[end - 1]) {
        end -= 1;
    }
    fragments[..end]
        .iter()
        .filter(|fragment| inline_fragment_is_inter_word_justification_space(*fragment))
        .map(|fragment| fragment.text().chars().count())
        .sum()
}

pub(in crate::layout) fn split_mixed_line_into_inter_character_units(
    items: &[InlineLineItem],
) -> Vec<InlineLineItem> {
    items
        .iter()
        .flat_map(|item| match item {
            InlineLineItem::Fragment(fragment) => {
                split_fragment_into_inter_character_units(fragment)
                    .into_iter()
                    .map(InlineLineItem::Fragment)
                    .collect()
            }
            InlineLineItem::Atom(atom) => vec![InlineLineItem::Atom(atom.clone())],
            InlineLineItem::Float(_) => Vec::new(),
        })
        .collect()
}

pub(in crate::layout) fn split_fragment_into_inter_character_units(
    fragment: &InlineFragment,
) -> Vec<InlineFragment> {
    if fragment.generated_leader() {
        return vec![fragment.clone()];
    }
    let ranges = typographic_unit_ranges(fragment.text());
    if ranges.len() <= 1 {
        return vec![fragment.clone()];
    }
    ranges
        .into_iter()
        .filter_map(|range| {
            let text = &fragment.text()[range];
            (!text.is_empty()).then(|| {
                InlineFragment::new(
                    text,
                    fragment.style().clone(),
                    fragment.baseline_shift,
                    fragment.link_target().map(ToOwned::to_owned),
                    false,
                    fragment.source(),
                    fragment.generated_leader(),
                    fragment.hanging_edges(),
                    fragment.ancestor_inline_decorations().to_vec(),
                )
                .with_visual_offset(fragment.visual_offset())
            })
        })
        .collect()
}

/// CSS Text justification policy selected for one materialized inline line.
///
/// CSS Text defines justification in terms of text-justification opportunities
/// after white-space processing and line breaking. Keeping those opportunities
/// in a line-level plan lets mixed, generated, page-margin, and fragmented
/// painting share the same expansion decisions:
/// <https://www.w3.org/TR/css-text-3/#text-justify-property>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct InlineJustificationPlan {
    pub(in crate::layout) mode: InlineJustificationMode,
    pub(in crate::layout) opportunities: Vec<JustificationOpportunity>,
    pub(in crate::layout) item_expansion_counts: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineJustificationMode {
    None,
    InterWord,
    InterCharacter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct JustificationOpportunity {
    pub(in crate::layout) after_item_index: usize,
    pub(in crate::layout) kind: JustificationOpportunityKind,
}

/// CSS Text justification opportunity kind recorded for diagnostics/tests.
///
/// Suppressed and blocking entries are retained so tests can prove that
/// cursive/control gaps and opaque atom boundaries were considered by policy
/// rather than silently omitted by the painter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum JustificationOpportunityKind {
    WordSeparator,
    TypographicUnitGap,
    SuppressedScriptOrControlGap,
    OpaqueAtomBoundary,
}

impl InlineJustificationPlan {
    pub(in crate::layout) fn for_line(
        items: &[InlineLineItem],
        text_justify: TextJustify,
        should_justify: bool,
    ) -> Self {
        let mode = match (should_justify, text_justify) {
            (false, _) | (_, TextJustify::None) => InlineJustificationMode::None,
            (true, TextJustify::InterCharacter) => InlineJustificationMode::InterCharacter,
            (true, TextJustify::Auto | TextJustify::InterWord) => {
                InlineJustificationMode::InterWord
            }
        };
        let mut plan = Self {
            mode,
            opportunities: Vec::new(),
            item_expansion_counts: vec![0; items.len()],
        };
        match mode {
            InlineJustificationMode::None => {}
            InlineJustificationMode::InterWord => plan.collect_inter_word_opportunities(items),
            InlineJustificationMode::InterCharacter => {
                plan.collect_inter_character_opportunities(items)
            }
        }
        plan
    }

    pub(in crate::layout) fn justifies_inter_word(&self) -> bool {
        self.mode == InlineJustificationMode::InterWord
    }

    pub(in crate::layout) fn justifies_inter_character(&self) -> bool {
        self.mode == InlineJustificationMode::InterCharacter
    }

    pub(in crate::layout) fn expansion_opportunity_count(&self) -> usize {
        self.item_expansion_counts.iter().sum()
    }

    pub(in crate::layout) fn extra_space_width(
        &self,
        line_width: f32,
        available_width: f32,
    ) -> f32 {
        let gaps = self.expansion_opportunity_count();
        if gaps > 0 && line_width < available_width {
            (available_width - line_width) / gaps as f32
        } else {
            0.0
        }
    }

    pub(in crate::layout) fn expansion_count_after_item(&self, item_index: usize) -> usize {
        self.item_expansion_counts
            .get(item_index)
            .copied()
            .unwrap_or(0)
    }

    pub(in crate::layout) fn collect_inter_word_opportunities(&mut self, items: &[InlineLineItem]) {
        let mut end = items.len();
        while end > 0 && inline_line_item_is_pre_wrap_hanging_space(&items[end - 1]) {
            end -= 1;
        }
        for (index, item) in items[..end].iter().enumerate() {
            let InlineLineItem::Fragment(fragment) = item else {
                continue;
            };
            if !inline_fragment_is_inter_word_justification_space(fragment) {
                continue;
            }
            let count = fragment.text().chars().count();
            self.item_expansion_counts[index] = count;
            self.opportunities
                .extend((0..count).map(|_| JustificationOpportunity {
                    after_item_index: index,
                    kind: JustificationOpportunityKind::WordSeparator,
                }));
        }
    }

    pub(in crate::layout) fn collect_inter_character_opportunities(
        &mut self,
        items: &[InlineLineItem],
    ) {
        for index in 0..items.len().saturating_sub(1) {
            let item = &items[index];
            let next = &items[index + 1];
            if matches!(item, InlineLineItem::Float(_)) || matches!(next, InlineLineItem::Float(_))
            {
                continue;
            }
            if matches!(item, InlineLineItem::Atom(_)) || matches!(next, InlineLineItem::Atom(_)) {
                self.opportunities.push(JustificationOpportunity {
                    after_item_index: index,
                    kind: JustificationOpportunityKind::OpaqueAtomBoundary,
                });
                continue;
            }
            let (InlineLineItem::Fragment(fragment), InlineLineItem::Fragment(next_fragment)) =
                (item, next)
            else {
                continue;
            };
            if !inline_fragment_is_inter_character_unit(fragment)
                || !inline_fragment_is_inter_character_unit(next_fragment)
            {
                continue;
            }
            let allowed =
                inter_character_gap_allowed_between_text(fragment.text(), next_fragment.text());
            self.opportunities.push(JustificationOpportunity {
                after_item_index: index,
                kind: if allowed {
                    JustificationOpportunityKind::TypographicUnitGap
                } else {
                    JustificationOpportunityKind::SuppressedScriptOrControlGap
                },
            });
            if allowed {
                self.item_expansion_counts[index] = 1;
            }
        }
    }
}

pub(in crate::layout) fn inline_fragment_is_inter_character_unit(
    fragment: &InlineFragment,
) -> bool {
    !fragment.generated_leader() && typographic_unit_count(fragment.text()) > 0
}

pub(in crate::layout) fn apply_first_line_pseudos_to_line_items(
    items: &mut Vec<InlineLineItem>,
    block_style: &ComputedStyle,
) {
    if let Some(first_line_style) = block_style.first_line_style.as_deref() {
        for item in items.iter_mut() {
            if let InlineLineItem::Fragment(fragment) = item {
                *fragment.style_mut() = first_line_style.clone();
            }
        }
    }
    if let Some(first_letter_style) = block_style.first_letter_style.as_deref() {
        apply_first_letter_pseudo_to_line_items(items, first_letter_style);
    }
}

pub(in crate::layout) fn apply_first_letter_pseudo_to_line_items(
    items: &mut Vec<InlineLineItem>,
    first_letter_style: &ComputedStyle,
) {
    for index in 0..items.len() {
        let InlineLineItem::Fragment(fragment) = &items[index] else {
            continue;
        };
        let Some(range) = first_letter_byte_range(fragment.text()) else {
            continue;
        };
        let pieces = split_fragment_for_first_letter(fragment, range, first_letter_style)
            .into_iter()
            .map(InlineLineItem::Fragment)
            .collect::<Vec<_>>();
        items.splice(index..=index, pieces);
        break;
    }
}

pub(in crate::layout) fn split_fragment_for_first_letter(
    fragment: &InlineFragment,
    range: std::ops::Range<usize>,
    first_letter_style: &ComputedStyle,
) -> Vec<InlineFragment> {
    let mut pieces = Vec::new();
    if range.start > 0 {
        let mut before = fragment.clone();
        before.set_text(fragment.text()[..range.start].to_string());
        pieces.push(before);
    }
    let mut letter = fragment.clone();
    letter.set_text(fragment.text()[range.clone()].to_string());
    *letter.style_mut() = first_letter_style.clone();
    letter.set_mergeable(false);
    pieces.push(letter);
    if range.end < fragment.text().len() {
        let mut after = fragment.clone();
        after.set_text(fragment.text()[range.end..].to_string());
        pieces.push(after);
    }
    pieces
}

pub(in crate::layout) fn first_letter_byte_range(text: &str) -> Option<std::ops::Range<usize>> {
    let mut start = None;
    let mut end = None;
    let mut saw_letter = false;
    for (index, character) in text.char_indices() {
        if start.is_none() && character.is_whitespace() {
            continue;
        }
        let is_punctuation = character_is_unicode_punctuation(character);
        if !saw_letter {
            if is_punctuation {
                start.get_or_insert(index);
                end = Some(index + character.len_utf8());
                continue;
            }
            if character_is_unicode_alphanumeric(character) {
                start.get_or_insert(index);
                end = Some(index + character.len_utf8());
                saw_letter = true;
                continue;
            }
            if start.is_some() {
                return None;
            }
            continue;
        }
        if is_punctuation {
            end = Some(index + character.len_utf8());
        } else {
            break;
        }
    }
    saw_letter.then(|| start.unwrap_or(0)..end.unwrap_or(0))
}
