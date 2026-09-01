use super::*;
use crate::text::{character_is_autospace_ideograph, character_is_inter_character_control};
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

/// Split only the typographic units that `text-justify: auto` can expand.
///
/// The `auto` policy shares inter-word expansion with `inter-word`, but it
/// additionally distributes only between adjacent ideographic units.  It must
/// not turn ordinary Latin text into per-character paint groups merely because
/// the line is justified: those groups have no eligible `auto` boundary and
/// can otherwise perturb shaping, extraction, and line-summary geometry.
///
/// <https://drafts.csswg.org/css-text-3/#text-justify-property>
pub(in crate::layout) fn split_mixed_line_into_auto_justification_units(
    items: &[InlineLineItem],
) -> Vec<InlineLineItem> {
    items
        .iter()
        .flat_map(|item| match item {
            InlineLineItem::Fragment(fragment) => {
                split_fragment_into_auto_justification_units(fragment)
                    .into_iter()
                    .map(InlineLineItem::Fragment)
                    .collect()
            }
            InlineLineItem::Atom(atom) => vec![InlineLineItem::Atom(atom.clone())],
            InlineLineItem::Float(_) => Vec::new(),
        })
        .collect()
}

fn split_fragment_into_auto_justification_units(fragment: &InlineFragment) -> Vec<InlineFragment> {
    if fragment.generated_leader() {
        return vec![fragment.clone()];
    }
    let ranges = CursiveProtectedUnitRanges::new(fragment.text());
    let contains_ideograph = ranges.iter().any(|range| {
        fragment.text()[range.clone()]
            .chars()
            .any(character_is_autospace_ideograph)
    });
    if !contains_ideograph {
        return vec![fragment.clone()];
    }
    if ranges.len() <= 1 {
        return vec![auto_justification_fragment_slice(
            fragment,
            0..fragment.text().len(),
            false,
        )];
    }

    let mut fragments = Vec::with_capacity(ranges.len());
    let mut ordinary_start = None;
    for range in ranges {
        let unit = &fragment.text()[range.clone()];
        if unit.chars().any(character_is_autospace_ideograph) {
            if let Some(start) = ordinary_start.take() {
                fragments.push(auto_justification_fragment_slice(
                    fragment,
                    start..range.start,
                    fragment.mergeable(),
                ));
            }
            // The explicit unit boundary carries a possible auto
            // justification gap.  Retain it as a paint boundary even on a
            // line whose used extra space happens to be zero, matching the
            // inter-character materialization route.
            fragments.push(auto_justification_fragment_slice(fragment, range, false));
        } else {
            ordinary_start.get_or_insert(range.start);
        }
    }
    if let Some(start) = ordinary_start {
        fragments.push(auto_justification_fragment_slice(
            fragment,
            start..fragment.text().len(),
            fragment.mergeable(),
        ));
    }
    fragments
}

fn auto_justification_fragment_slice(
    fragment: &InlineFragment,
    range: std::ops::Range<usize>,
    mergeable: bool,
) -> InlineFragment {
    let mut hanging_edges = fragment.hanging_edges();
    hanging_edges.blocks_start &= range.start == 0;
    hanging_edges.blocks_end &= range.end == fragment.text().len();
    InlineFragment::new(
        &fragment.text()[range],
        fragment.style().clone(),
        fragment.baseline_shift,
        fragment.link_target().map(ToOwned::to_owned),
        mergeable,
        fragment.source(),
        false,
        hanging_edges,
        fragment.ancestor_inline_decorations().to_vec(),
    )
    .with_visual_offset(fragment.visual_offset())
}

pub(in crate::layout) fn split_fragment_into_inter_character_units(
    fragment: &InlineFragment,
) -> Vec<InlineFragment> {
    if fragment.generated_leader() {
        return vec![fragment.clone()];
    }
    let ranges = CursiveProtectedUnitRanges::new(fragment.text());
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
    /// CSS Text's default compromise: stretch word separators and eligible
    /// ideographic boundaries without treating every script as trackable.
    Auto,
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
            (true, TextJustify::InterWord, false) => InlineJustificationMode::InterWord,
            (true, TextJustify::Auto, false) => InlineJustificationMode::Auto,
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
            InlineJustificationMode::Auto => plan.collect_auto_opportunities(items),
        }
        plan
    }

    pub(in crate::layout) fn justifies_inter_word(&self) -> bool {
        matches!(
            self.mode,
            InlineJustificationMode::InterWord | InlineJustificationMode::Auto
        )
    }

    pub(in crate::layout) fn justifies_inter_character(&self) -> bool {
        matches!(
            self.mode,
            InlineJustificationMode::InterCharacter | InlineJustificationMode::Auto
        )
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

    /// Return the expansion count caused by an inter-unit boundary, as
    /// opposed to a word separator owned by the item itself.
    pub(in crate::layout) fn inter_character_expansion_count_after_item(
        &self,
        item_index: usize,
    ) -> usize {
        self.opportunities
            .iter()
            .filter(|opportunity| {
                opportunity.after_item_index == item_index
                    && opportunity.kind == JustificationOpportunityKind::TypographicUnitGap
            })
            .count()
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

    /// Collect the default `text-justify: auto` opportunity set.
    ///
    /// CSS Text permits a universal compromise that distributes through word
    /// separators and between CJK ideographic typographic units. Formatting
    /// controls remain part of bidi resolution but are transparent here: a
    /// control can neither own nor create a justification gap.
    /// <https://drafts.csswg.org/css-text-3/#text-justify-property>
    fn collect_auto_opportunities(&mut self, items: &[InlineLineItem]) {
        self.collect_inter_word_opportunities(items);
        let units = inter_character_justification_units(items);
        for pair in units.windows(2) {
            let unit = pair[0];
            let next = pair[1];
            let allowed = auto_justification_gap_allowed_between_units(unit, next);
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

fn auto_justification_gap_allowed_between_units(
    unit: InterCharacterJustificationUnit<'_>,
    next: InterCharacterJustificationUnit<'_>,
) -> bool {
    let (
        InterCharacterJustificationUnitKind::Text(text),
        InterCharacterJustificationUnitKind::Text(next_text),
    ) = (unit.kind, next.kind)
    else {
        return false;
    };
    text.chars()
        .rev()
        .find(|character| !character_is_inter_character_control(*character))
        .is_some_and(character_is_autospace_ideograph)
        && next_text
            .chars()
            .find(|character| !character_is_inter_character_control(*character))
            .is_some_and(character_is_autospace_ideograph)
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
            | InlineAtomContent::Ruby { .. }
    )
}

pub(in crate::layout) fn inline_atom_is_inter_character_transparent(atom: &InlineAtom) -> bool {
    matches!(
        atom.content(),
        InlineAtomContent::InlineEdge(_) | InlineAtomContent::StaticPositionPlaceholder(_)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn text_item(text: &str, style: ComputedStyle) -> InlineLineItem {
        InlineLineItem::Fragment(InlineFragment::new(
            text,
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        ))
    }

    #[test]
    fn auto_justification_uses_visible_cjk_boundaries_not_bidi_controls() {
        let mut style = ComputedStyle::initial();
        style.text_justify = TextJustify::Auto;
        // The LRI/PDI stay attached to their visible typographic unit. They
        // affect UAX #9, but they must never own a distributable gap.
        let items = ["東\u{2066}", "京", "都", "東", "京\u{2069}", "都"]
            .into_iter()
            .map(|text| text_item(text, style.clone()))
            .collect::<Vec<_>>();

        let plan = InlineJustificationPlan::for_line(&items, TextJustify::Auto, true);

        assert_eq!(plan.mode, InlineJustificationMode::Auto);
        assert_eq!(plan.item_expansion_counts, vec![1, 1, 1, 1, 1, 0]);
        assert!(plan.opportunities.iter().all(|opportunity| {
            opportunity.kind == JustificationOpportunityKind::TypographicUnitGap
        }));
    }

    #[test]
    fn auto_justification_keeps_latin_runs_intact_while_splitting_ideographs() {
        let style = ComputedStyle::initial();
        let fragment = InlineFragment::new(
            "Latin 東\u{2066}京都 text",
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        );

        let units = split_fragment_into_auto_justification_units(&fragment)
            .into_iter()
            .map(|unit| unit.text().to_owned())
            .collect::<Vec<_>>();

        assert_eq!(units, vec!["Latin ", "東\u{2066}", "京", "都", " text"]);
    }
}
