use super::*;

#[derive(Clone)]
struct TextSpacingCharacter {
    item_index: usize,
    range: std::ops::Range<usize>,
    class: Option<crate::text::TextSpacingPunctuationClass>,
    policy: TextSpacingTrim,
}

/// Return the text-spacing characters that participate in selected-line edge
/// selection.
///
/// CSS-generated UAX #9 controls and zero-advance inline-scope boundaries do
/// not create text edges. The remaining visible text is one selected inline
/// line even when it crosses an automatic `::marker` or an authored isolate;
/// a pseudo-element is not a CSS Text edge of its own. Keep the controls in
/// the materialized item stream for bidi, while making them transparent to
/// punctuation spacing:
/// <https://drafts.csswg.org/css-text-4/#text-spacing-trim-property> and
/// <https://drafts.csswg.org/css-lists-3/#list-style-position-property>.
fn selected_line_text_spacing_characters(
    items: &[MeasuredInlineItem],
) -> Vec<TextSpacingCharacter> {
    let mut characters = Vec::new();
    for (item_index, item) in items.iter().enumerate() {
        let InlineLineItem::Fragment(fragment) = &item.item else {
            continue;
        };
        let vertical = matches!(
            fragment.style().text_layout_policy(),
            crate::css::TextLayoutPolicy::Vertical(_)
        );
        for (start, character) in fragment.text().char_indices() {
            if crate::text::character_is_bidi_format_control(character) {
                continue;
            }
            characters.push(TextSpacingCharacter {
                item_index,
                range: start..start + character.len_utf8(),
                class: crate::text::text_spacing_punctuation_class(
                    character,
                    fragment.style().language.as_deref(),
                    vertical,
                ),
                policy: fragment.style().text_spacing_trim.resolved(),
            });
        }
    }
    characters
}

/// Whether selected source can produce a `text-spacing-trim` adjustment.
///
/// This deliberately mirrors the target eligibility in
/// `apply_materialized_text_spacing_trim` without collecting per-character
/// provenance. Ordinary text is by far the common case, and it does not need
/// the allocated character list used to resolve selected-line adjacency.
pub(super) fn materialized_items_may_use_text_spacing_trim(items: &[MeasuredInlineItem]) -> bool {
    items.iter().any(|item| {
        let InlineLineItem::Fragment(fragment) = &item.item else {
            return false;
        };
        if fragment.style().text_spacing_trim.resolved() == TextSpacingTrim::SpaceAll {
            return false;
        }
        let vertical = matches!(
            fragment.style().text_layout_policy(),
            crate::css::TextLayoutPolicy::Vertical(_)
        );
        fragment.text().chars().any(|character| {
            !crate::text::character_is_bidi_format_control(character)
                && crate::text::text_spacing_punctuation_class(
                    character,
                    fragment.style().language.as_deref(),
                    vertical,
                )
                .is_some()
        })
    })
}

/// Apply `text-spacing-trim` to one candidate's selected source items.
///
/// The graph keeps its source runs full-width. This function runs only after a
/// candidate has chosen its line range, splits the used fragments at affected
/// typographic units, and reshapes those units with the appropriate OpenType
/// alternate. Thus a narrower opening bracket changes the candidate measure
/// before line selection commits, without mutating source text, extraction, or
/// graph break positions:
/// <https://drafts.csswg.org/css-text-4/#text-spacing-trim-property>.
pub(super) fn apply_materialized_text_spacing_trim(
    items: &mut Vec<MeasuredInlineItem>,
    font_system: &mut FontSystem,
    is_initial_line: bool,
    _available_width: Option<f32>,
) {
    use crate::text::TextSpacingPunctuationClass::{
        Closing, IdeographicSpace, MiddleDot, NarrowClosing, NarrowOpening, Opening,
    };

    if !materialized_items_may_use_text_spacing_trim(items) {
        return;
    }
    let characters = selected_line_text_spacing_characters(items);
    debug_assert!(!characters.is_empty());

    let mut targets = Vec::<(usize, std::ops::Range<usize>)>::new();
    let mut add_target = |character: TextSpacingCharacter| {
        if matches!(character.class, Some(Opening | Closing | MiddleDot))
            && !targets.iter().any(|(item_index, range)| {
                *item_index == character.item_index && *range == character.range
            })
        {
            targets.push((character.item_index, character.range));
        }
    };

    for character in &characters {
        if character.policy == TextSpacingTrim::TrimAll && character.class.is_some() {
            add_target(character.clone());
        }
    }
    // CSS Text's start and end are the physical edges of this selected
    // formatted line. Generated-marker provenance is retained for marker
    // semantics and paint, but it is not a separate text-spacing boundary.
    let first_line_edge_character = characters.first();
    let last_line_edge_character = characters.last();
    if let Some(first) = first_line_edge_character {
        let trims_start = matches!(
            first.policy,
            TextSpacingTrim::TrimStart | TextSpacingTrim::TrimBoth
        ) || (first.policy == TextSpacingTrim::SpaceFirst && !is_initial_line);
        if first.class == Some(Opening) && trims_start {
            add_target(first.clone());
        }
    }
    if let Some(last) = last_line_edge_character
        && last.class == Some(Closing)
        && matches!(
            last.policy,
            TextSpacingTrim::Normal
                | TextSpacingTrim::TrimStart
                | TextSpacingTrim::SpaceFirst
                | TextSpacingTrim::TrimBoth
        )
    {
        add_target(last.clone());
    }
    for pair in characters.windows(2) {
        let [previous, current] = pair else { continue };
        if current.policy == TextSpacingTrim::SpaceAll {
            continue;
        }
        if current.class == Some(Opening)
            && matches!(
                previous.class,
                Some(Opening | MiddleDot | Closing | IdeographicSpace | NarrowOpening)
            )
        {
            add_target(current.clone());
        }
        if previous.policy != TextSpacingTrim::SpaceAll
            && previous.class == Some(Closing)
            && matches!(
                current.class,
                Some(Closing | MiddleDot | IdeographicSpace | NarrowClosing)
            )
        {
            add_target(previous.clone());
        }
    }
    if targets.is_empty() {
        return;
    }

    let mut output = Vec::with_capacity(items.len() + targets.len());
    for (item_index, item) in std::mem::take(items).into_iter().enumerate() {
        let InlineLineItem::Fragment(fragment) = &item.item else {
            output.push(item);
            continue;
        };
        let mut ranges = targets
            .iter()
            .filter(|(target_index, _)| *target_index == item_index)
            .map(|(_, range)| range.clone())
            .collect::<Vec<_>>();
        if ranges.is_empty() {
            output.push(item);
            continue;
        }
        ranges.sort_by_key(|range| range.start);
        let mut cursor = 0;
        for range in ranges {
            if cursor < range.start {
                push_text_spacing_fragment(
                    &mut output,
                    fragment,
                    &fragment.text()[cursor..range.start],
                    false,
                    font_system,
                );
            }
            push_text_spacing_fragment(
                &mut output,
                fragment,
                &fragment.text()[range.clone()],
                true,
                font_system,
            );
            cursor = range.end;
        }
        if cursor < fragment.text().len() {
            push_text_spacing_fragment(
                &mut output,
                fragment,
                &fragment.text()[cursor..],
                false,
                font_system,
            );
        }
    }
    *items = output;
}

pub(super) fn push_text_spacing_fragment(
    output: &mut Vec<MeasuredInlineItem>,
    source: &InlineFragment,
    text: &str,
    trimmed: bool,
    font_system: &mut FontSystem,
) {
    if text.is_empty() {
        return;
    }
    let mut fragment = source.clone();
    fragment.set_text(Rc::from(text));
    if trimmed {
        let tag = if matches!(
            fragment.style().text_layout_policy(),
            crate::css::TextLayoutPolicy::Vertical(_)
        ) {
            *b"vhal"
        } else {
            *b"halt"
        };
        let style = fragment.style_mut();
        if let Some(setting) = style
            .font_feature_settings
            .0
            .iter_mut()
            .find(|setting| setting.tag == tag)
        {
            setting.value = 1;
        } else {
            style
                .font_feature_settings
                .0
                .push(crate::css::FontFeatureSetting::new(tag, 1));
            style
                .font_feature_settings
                .0
                .sort_by_key(|setting| setting.tag);
        }
    }
    let shaped = font_system.shape_untracked_inline_line_with_style_identity(
        fragment.text(),
        &fragment.data.style,
        fragment.style().line_height,
    );
    let width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    output.push(MeasuredInlineItem::new(
        InlineLineItem::Fragment(fragment),
        width,
        shaped.map(Rc::new),
    ));
}
