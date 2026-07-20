use super::*;
use std::rc::Rc;

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
        || (fragment.style().text_decoration.clone().has_visible_line()
            && fragment
                .style()
                .text_decoration
                .color
                .unwrap_or(fragment.style().color)
                .is_visible())
        || fragment.style().text_shadow.iter().any(|shadow| {
            !shadow.inset && shadow.color.resolve(fragment.style().color).is_visible()
        })
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
    !fragment.generated_leader() && typographic_unit_count(fragment.text()) > 0
}

pub(in crate::layout) fn inline_atom_is_inter_character_unit(atom: &InlineAtom) -> bool {
    matches!(
        atom.content(),
        InlineAtomContent::Canvas
            | InlineAtomContent::Iframe(_)
            | InlineAtomContent::Image(_)
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

pub(in crate::layout) fn apply_first_line_pseudos_to_line_items(
    items: &mut Vec<InlineLineItem>,
    block_style: &ComputedStyle,
    apply_first_letter: bool,
) {
    if let Some(first_line_style) = block_style.first_line_style.as_deref() {
        for item in items.iter_mut() {
            if let InlineLineItem::Fragment(fragment) = item
                && apply_first_line_style_delta(fragment.style_mut(), block_style, first_line_style)
            {
                fragment.set_force_inline_background_paint(true);
            }
        }
    }
    if apply_first_letter
        && let Some(first_letter_style) = block_style.first_letter_style.as_deref()
    {
        apply_first_letter_pseudo_to_line_items(items, first_letter_style);
    }
}

/// Apply properties established by `::first-line` without discarding a nested
/// inline fragment's own cascade (such as `<strong>`'s font weight).
///
/// A typographic pseudo style inherits from the originating block, while an
/// inline fragment may inherit from a descendant of that block. Copy only a
/// property whose pseudo computed value differs from its originating value:
/// that difference represents the pseudo rule's cascaded effect.
/// <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
fn apply_first_line_style_delta(
    fragment_style: &mut ComputedStyle,
    originating_style: &ComputedStyle,
    first_line_style: &ComputedStyle,
) -> bool {
    let mut background_changed = false;
    if first_line_style.color != originating_style.color
        && fragment_style.color == originating_style.color
    {
        // `::first-line` supplies an inherited color to its descendant
        // fragments. A nested inline's specified color, including one whose
        // principal box was suppressed by `display: contents`, wins over that
        // inherited pseudo value.
        // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
        fragment_style.color = first_line_style.color;
    }
    if first_line_style.text_fill_color != originating_style.text_fill_color {
        fragment_style.text_fill_color = first_line_style.text_fill_color;
    }
    if first_line_style.background_color != originating_style.background_color {
        fragment_style.background_color = first_line_style.background_color;
        background_changed = true;
    }
    if first_line_style.background_color_is_current_color
        != originating_style.background_color_is_current_color
    {
        fragment_style.background_color_is_current_color =
            first_line_style.background_color_is_current_color;
        background_changed = true;
    }
    if first_line_style.font_weight != originating_style.font_weight {
        fragment_style.font_weight = first_line_style.font_weight;
    }
    if first_line_style.font_style != originating_style.font_style {
        fragment_style.font_style = first_line_style.font_style;
    }
    if first_line_style.font_width != originating_style.font_width {
        fragment_style.font_width = first_line_style.font_width;
    }
    if first_line_style.font_size != originating_style.font_size {
        fragment_style.font_size = first_line_style.font_size;
    }
    if first_line_style.line_height != originating_style.line_height {
        fragment_style.line_height = first_line_style.line_height;
    }
    if first_line_style.letter_spacing != originating_style.letter_spacing {
        fragment_style.letter_spacing = first_line_style.letter_spacing.clone();
    }
    if first_line_style.word_spacing != originating_style.word_spacing {
        fragment_style.word_spacing = first_line_style.word_spacing.clone();
    }
    if first_line_style.text_transform != originating_style.text_transform {
        fragment_style.text_transform = first_line_style.text_transform;
    }
    if first_line_style.vertical_align != originating_style.vertical_align {
        fragment_style.vertical_align = first_line_style.vertical_align.clone();
    }
    background_changed
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
        before.set_text(Rc::<str>::from(&fragment.text()[..range.start]));
        pieces.push(before);
    }
    let mut letter = fragment.clone();
    letter.set_text(Rc::<str>::from(&fragment.text()[range.clone()]));
    *letter.style_mut() = first_letter_style.clone();
    letter.set_mergeable(false);
    pieces.push(letter);
    if range.end < fragment.text().len() {
        let mut after = fragment.clone();
        after.set_text(Rc::<str>::from(&fragment.text()[range.end..]));
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
