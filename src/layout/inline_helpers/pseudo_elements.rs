use super::*;
pub(in crate::layout) fn apply_first_line_pseudos_to_line_items(
    items: &mut Vec<InlineLineItem>,
    block_style: &ComputedStyle,
    apply_first_letter: bool,
) {
    if let Some(first_line_style) = block_style.first_line_style.as_deref() {
        for item in items.iter_mut() {
            match item {
                InlineLineItem::Fragment(fragment) => {
                    // CSS Overflow inserts a block ellipsis *after* the
                    // clamp container's first formatted line is determined.
                    // It is an anonymous inline of the terminal root inline
                    // box, not source content participating in `::first-line`.
                    // <https://drafts.csswg.org/css-overflow-4/#block-ellipsis>
                    if matches!(fragment.source(), InlineTextSource::BlockEllipsis) {
                        continue;
                    }
                    if apply_first_line_style_delta(
                        fragment.style_mut(),
                        block_style,
                        first_line_style,
                    ) {
                        fragment.set_force_inline_background_paint(true);
                    }
                    fragment.apply_to_ancestor_inline_decoration_styles(|style| {
                        apply_first_line_style_delta(style, block_style, first_line_style);
                    });
                }
                InlineLineItem::Atom(atom) => {
                    if matches!(
                        atom.content(),
                        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                    ) {
                        if first_line_style.color != block_style.color
                            && atom.style().color == block_style.color
                        {
                            atom.set_current_color_override(first_line_style.color);
                        }
                        continue;
                    }
                    let content = Rc::make_mut(&mut atom.data);
                    let InlineAtomContent::Ruby {
                        base,
                        annotations,
                        annotation_sides,
                        ..
                    } = &mut content.content
                    else {
                        continue;
                    };
                    apply_first_line_style_to_ruby_level(base, block_style, first_line_style);
                    for (annotation, side) in annotations.iter_mut().zip(annotation_sides) {
                        apply_first_line_style_to_ruby_level(
                            annotation,
                            block_style,
                            first_line_style,
                        );
                        *side = annotation.style.ruby_position.interlinear_side();
                    }
                }
                InlineLineItem::Float(_) => {}
            }
        }
    }
    if apply_first_letter
        && let Some(first_letter_style) = block_style.first_letter_style.as_deref()
    {
        apply_first_letter_pseudo_to_line_items(items, first_letter_style);
    }
}

/// Replay the originating block's first-line inheritance into a ruby level's
/// already-selected inner line records. Ruby collection captures these records
/// before the parent line has been selected, so changing only the level style
/// would leave its text fragments with stale inherited values at paint time.
///
/// CSS Pseudo specifies that inherited first-line values apply to the first
/// line's descendants, rather than creating a new `ruby::first-line` scope.
/// <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
fn apply_first_line_style_to_ruby_level(
    level: &mut RubyInlineLevel,
    originating_style: &ComputedStyle,
    first_line_style: &ComputedStyle,
) {
    apply_first_line_style_delta(&mut level.style, originating_style, first_line_style);
    for record in &mut level.sequence.records {
        let Some(fragment) = &mut record.fragment else {
            continue;
        };
        for measured in Rc::make_mut(&mut fragment.items).iter_mut() {
            match &mut measured.item {
                InlineLineItem::Fragment(fragment) => {
                    apply_first_line_style_delta(
                        fragment.style_mut(),
                        originating_style,
                        first_line_style,
                    );
                }
                InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {}
            }
        }
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
pub(in crate::layout) fn apply_first_line_style_delta(
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
    if first_line_style.background.background_color != originating_style.background.background_color
    {
        fragment_style.background.background_color =
            first_line_style.background.background_color.clone();
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
    if first_line_style.ruby_position != originating_style.ruby_position
        && fragment_style.ruby_position == originating_style.ruby_position
    {
        fragment_style.ruby_position = first_line_style.ruby_position;
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
    /// Classify a complete CSS typographic character unit by its visible base
    /// character. Combining marks and default-ignorable controls remain in
    /// the selected unit rather than creating their own first-letter choice.
    fn unit_base(unit: &str) -> Option<char> {
        unit.chars().find(|character| {
            !character_is_unicode_mark(*character)
                && !character_is_default_ignorable_code_point(*character)
        })
    }

    let units = typographic_unit_ranges(text);
    let mut prefix_start = None;
    let mut selected_start = None;
    let mut selected_end = None;
    let mut pending_suffix_space_start = None;

    for range in units {
        let unit = &text[range.clone()];
        let Some(base) = unit_base(unit) else {
            continue;
        };

        let selected = selected_start.is_some();
        if !selected {
            if character_is_first_letter_associated_space(base) && prefix_start.is_some() {
                continue;
            }
            if base.is_whitespace() && prefix_start.is_none() {
                // Leading white space is selected separately when preserved;
                // ordinary collapsed space cannot create first-letter text.
                continue;
            }
            if character_is_unicode_punctuation(base) {
                prefix_start.get_or_insert(range.start);
                continue;
            }
            if character_is_unicode_first_letter_base(base) {
                selected_start = Some(prefix_start.unwrap_or(range.start));
                selected_end = Some(range.end);
                continue;
            }
            // A non-punctuation, non-base unit between an associated prefix
            // and a prospective initial prevents that prefix from attaching.
            if prefix_start.is_some() {
                return None;
            }
            continue;
        }

        if character_is_first_letter_associated_space(base) {
            pending_suffix_space_start.get_or_insert(range.start);
            continue;
        }
        if character_is_first_letter_suffix_punctuation(base) {
            if let Some(space_start) = pending_suffix_space_start.take() {
                selected_end = Some(range.end.max(space_start));
            } else {
                selected_end = Some(range.end);
            }
            continue;
        }
        break;
    }

    selected_start
        .zip(selected_end)
        .map(|(start, end)| start..end)
}

#[cfg(test)]
mod first_letter_tests {
    use super::first_letter_byte_range;

    #[test]
    fn selects_symbol_first_letters_and_associated_punctuation() {
        for (text, expected) in [
            ("$1,234.56", "$"),
            ("(£)78.90", "(£)"),
            ("₹10,000", "₹"),
            ("©2021", "©"),
        ] {
            let range = first_letter_byte_range(text).expect("expected first-letter text");
            assert_eq!(&text[range], expected, "{text}");
        }
    }

    #[test]
    fn preserves_complete_typographic_units_and_associated_spaces() {
        let text = "“\u{00a0}e\u{301}”x";
        let range = first_letter_byte_range(text).expect("expected first-letter text");
        assert_eq!(&text[range], "“\u{00a0}e\u{301}”");
    }

    #[test]
    fn rejects_unassociated_prefix_punctuation() {
        assert_eq!(first_letter_byte_range("(\u{3000}A"), None);
    }
}

#[cfg(test)]
mod inter_character_unit_tests {
    use super::*;

    fn fragment(text: &str, generated_leader: bool) -> InlineFragment {
        InlineFragment::new(
            text,
            ComputedStyle::initial(),
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            generated_leader,
            InlineHangingEdges::default(),
            Vec::new(),
        )
    }

    #[test]
    fn inter_character_unit_eligibility_requires_nonempty_non_leader_text() {
        assert!(!inline_fragment_is_inter_character_unit(&fragment(
            "", false
        )));
        assert!(!inline_fragment_is_inter_character_unit(&fragment(
            "text", true
        )));
        assert!(inline_fragment_is_inter_character_unit(&fragment(
            "text", false
        )));
        assert!(inline_fragment_is_inter_character_unit(&fragment(
            "\u{200e}", false
        )));
    }
}
