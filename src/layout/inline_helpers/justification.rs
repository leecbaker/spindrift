use super::*;
pub(in crate::layout) fn justifiable_fragment_space_count<F: InlineFragmentAccess>(
    fragments: &[F],
) -> usize {
    let mut end = fragments.len();
    while end > 0 && inline_fragment_is_pre_wrap_hanging_space(&fragments[end - 1]) {
        end -= 1;
    }
    fragments[..end]
        .iter()
        .map(inline_fragment_inter_word_justification_space_count)
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
/// Suppressed entries are retained so tests can prove that cursive/control
/// gaps were considered by policy rather than silently omitted by the painter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum JustificationOpportunityKind {
    WordSeparator,
    TypographicUnitGap,
    SuppressedScriptOrControlGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InterCharacterJustificationUnit<'a> {
    after_item_index: usize,
    kind: InterCharacterJustificationUnitKind<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterCharacterJustificationUnitKind<'a> {
    Text(&'a str),
    AtomicInlineRun,
}

impl InlineJustificationPlan {
    pub(in crate::layout) fn for_line(
        items: &[InlineLineItem],
        text_justify: TextJustify,
        should_justify: bool,
    ) -> Self {
        // A preserved tab is resolved against the logical inline cursor. CSS
        // Text requires those tab stops to retain that geometry; distributing
        // preceding document spaces for justification would move every later
        // stop. Keep the selected line un-justified until justification has
        // source-range metadata for text after a tab.
        // <https://drafts.csswg.org/css-text-4/#text-align-property> and
        // <https://drafts.csswg.org/css-text-3/#tab-size-property>
        let contains_preserved_tab = items.iter().any(|item| {
            matches!(item, InlineLineItem::Fragment(fragment) if fragment.text().contains('\t'))
        });
        let mode = match (should_justify, text_justify, contains_preserved_tab) {
            (_, _, true) => InlineJustificationMode::None,
            (false, _, false) | (_, TextJustify::None, false) => InlineJustificationMode::None,
            (true, TextJustify::InterCharacter, false) => InlineJustificationMode::InterCharacter,
            (true, TextJustify::Auto | TextJustify::InterWord, false) => {
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
            .cloned()
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
            // `text-justify` inherits, but an inline descendant can suppress
            // expansion for just its own word separators while the parent
            // justified line continues to distribute its remaining eligible
            // opportunities.
            // <https://drafts.csswg.org/css-text-3/#text-justify-property>
            if matches!(fragment.style().text_justify, TextJustify::None) {
                continue;
            }
            let separator_count = inline_fragment_inter_word_justification_space_count(fragment);
            if separator_count == 0 {
                continue;
            }
            self.item_expansion_counts[index] = separator_count;
            self.opportunities
                .extend((0..separator_count).map(|_| JustificationOpportunity {
                    after_item_index: index,
                    kind: JustificationOpportunityKind::WordSeparator,
                }));
        }
    }

    pub(in crate::layout) fn collect_inter_character_opportunities(
        &mut self,
        items: &[InlineLineItem],
    ) {
        let units = inter_character_justification_units(items);
        for pair in units.windows(2) {
            let unit = pair[0];
            let next = pair[1];
            let allowed = inter_character_gap_allowed_between_units(unit, next);
            self.opportunities.push(JustificationOpportunity {
                after_item_index: unit.after_item_index,
                kind: if allowed {
                    JustificationOpportunityKind::TypographicUnitGap
                } else {
                    JustificationOpportunityKind::SuppressedScriptOrControlGap
                },
            });
            if allowed {
                self.item_expansion_counts[unit.after_item_index] += 1;
            }
        }
    }
}

fn inter_character_justification_units(
    items: &[InlineLineItem],
) -> Vec<InterCharacterJustificationUnit<'_>> {
    let mut units = Vec::new();
    let mut previous_visible_unit_was_atomic = false;
    for (index, item) in items.iter().enumerate() {
        match item {
            InlineLineItem::Fragment(fragment) => {
                previous_visible_unit_was_atomic = false;
                if inline_fragment_is_inter_character_unit(fragment) {
                    units.push(InterCharacterJustificationUnit {
                        after_item_index: index,
                        kind: InterCharacterJustificationUnitKind::Text(fragment.text()),
                    });
                }
            }
            InlineLineItem::Atom(atom) if inline_atom_is_inter_character_unit(atom) => {
                let can_extend_atomic_run = previous_visible_unit_was_atomic
                    && matches!(
                        units.last(),
                        Some(InterCharacterJustificationUnit {
                            kind: InterCharacterJustificationUnitKind::AtomicInlineRun,
                            ..
                        })
                    );
                if can_extend_atomic_run {
                    let Some(InterCharacterJustificationUnit {
                        after_item_index,
                        kind: InterCharacterJustificationUnitKind::AtomicInlineRun,
                    }) = units.last_mut()
                    else {
                        unreachable!("atomic run check must match last unit");
                    };
                    *after_item_index = index;
                } else {
                    units.push(InterCharacterJustificationUnit {
                        after_item_index: index,
                        kind: InterCharacterJustificationUnitKind::AtomicInlineRun,
                    });
                }
                previous_visible_unit_was_atomic = true;
            }
            InlineLineItem::Atom(atom) if inline_atom_is_inter_character_transparent(atom) => {}
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                previous_visible_unit_was_atomic = false;
            }
        }
    }
    units
}

fn inter_character_gap_allowed_between_units(
    unit: InterCharacterJustificationUnit<'_>,
    next: InterCharacterJustificationUnit<'_>,
) -> bool {
    match (unit.kind, next.kind) {
        (
            InterCharacterJustificationUnitKind::Text(text),
            InterCharacterJustificationUnitKind::Text(next_text),
        ) => inter_character_gap_allowed_between_text(text, next_text),
        (
            InterCharacterJustificationUnitKind::Text(_)
            | InterCharacterJustificationUnitKind::AtomicInlineRun,
            InterCharacterJustificationUnitKind::Text(_)
            | InterCharacterJustificationUnitKind::AtomicInlineRun,
        ) => true,
    }
}

pub(in crate::layout) fn inline_fragment_is_inter_character_unit(
    fragment: &InlineFragment,
) -> bool {
    !fragment.generated_leader() && !fragment.text().is_empty()
}

pub(in crate::layout) fn inline_atom_is_inter_character_unit(atom: &InlineAtom) -> bool {
    matches!(
        atom.content(),
        InlineAtomContent::Canvas
            | InlineAtomContent::Iframe(_)
            | InlineAtomContent::Image(_)
            | InlineAtomContent::Gradient { .. }
            | InlineAtomContent::Svg { .. }
            | InlineAtomContent::InlineBox { .. }
            | InlineAtomContent::TextCombineUpright { .. }
            | InlineAtomContent::InlineFragment { .. }
    )
}

pub(in crate::layout) fn inline_atom_is_inter_character_transparent(atom: &InlineAtom) -> bool {
    matches!(
        atom.content(),
        InlineAtomContent::InlineEdge(_) | InlineAtomContent::StaticPositionPlaceholder
    )
}

/// Count the inter-word justification opportunities within an inline fragment.
///
/// A line fragment can contain unbreakable word separators (for example
/// U+00A0 NO-BREAK SPACE) in the middle of otherwise ordinary text. CSS Text
/// still allows those separators to expand under `text-justify: inter-word`;
/// restricting opportunities to fragments consisting only of spaces loses
/// final-line `text-align-last: justify` whenever line construction retains a
/// no-break sequence as one source fragment:
/// <https://www.w3.org/TR/css-text-3/#valdef-text-justify-inter-word>.
pub(in crate::layout) fn inline_fragment_inter_word_justification_space_count(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> usize {
    if fragment.generated_leader() {
        0
    } else {
        fragment
            .text()
            .chars()
            .filter(|character| character_is_css_word_separator(*character))
            .count()
    }
}
