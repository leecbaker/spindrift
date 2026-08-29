use std::rc::Rc;

use super::*;
use crate::css::{TextLayoutPolicy, TextOrientation};

pub(in crate::layout) fn insert_text_autospace_items(
    font_system: &mut FontSystem,
    scratch: &mut Vec<InlineItem>,
    items: &mut Vec<InlineItem>,
) {
    debug_assert!(scratch.is_empty());
    let mut previous_text = None::<AutospaceTextEdge>;
    for item in items.drain(..) {
        match item {
            InlineItem::Word(word) => {
                push_autospaced_word(font_system, scratch, word, &mut previous_text);
            }
            InlineItem::Atom(atom) => {
                // CSS Text makes an inline edge transparent only when no
                // margin, border, or padding separates the adjacent text.
                // Check the source box-model components instead of the atom's
                // advance: a negative margin can cancel a border or padding
                // advance without making the two text runs directly adjoin.
                // <https://drafts.csswg.org/css-text-4/#text-autospace-property>
                if !inline_edge_is_transparent_to_text_autospace(&atom) {
                    previous_text = None;
                }
                scratch.push(InlineItem::Atom(atom));
            }
            InlineItem::Float(float) => {
                previous_text = None;
                scratch.push(InlineItem::Float(float));
            }
            InlineItem::Break(break_) => {
                previous_text = None;
                scratch.push(InlineItem::Break(break_));
            }
            InlineItem::PageScopeStart(scope) => scratch.push(InlineItem::PageScopeStart(scope)),
            InlineItem::PageScopeEnd => scratch.push(InlineItem::PageScopeEnd),
        }
    }
    std::mem::swap(items, scratch);
}

/// Return whether an atomic inline edge preserves CSS Text autospace
/// adjacency between its neighboring text runs.
///
/// Text autospace atoms are their own transparent layout representation.
/// A box edge is transparent only if its originating inline box has no
/// inline-axis margin, border, or padding on that edge. This is deliberately
/// based on the computed box-model components, not the resolved edge advance;
/// negative margins may hide the advance while retaining a physical separator.
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>
pub(in crate::layout) fn inline_edge_is_transparent_to_text_autospace(atom: &InlineAtom) -> bool {
    match atom.content() {
        InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(_)) => true,
        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) => {
            let edge = match edge.logical_edge {
                InlineLogicalEdge::Start => InlineBoxEdge::Start,
                InlineLogicalEdge::End => InlineBoxEdge::End,
            };
            !inline_box_edge_has_nonzero_component(atom.style(), edge)
        }
        _ => false,
    }
}

pub(in crate::layout) fn push_autospaced_word(
    font_system: &mut FontSystem,
    output: &mut Vec<InlineItem>,
    mut word: Box<InlineWord>,
    previous_text: &mut Option<AutospaceTextEdge>,
) {
    while let Some(boundary) = first_internal_autospace_boundary(&word) {
        let suffix = InlineWord {
            text: word.text.split_off(boundary),
            style: Rc::clone(&word.style),
            baseline_shift: word.baseline_shift,
            visual_offset: word.visual_offset,
            link_target: word.link_target.clone(),
            mergeable: false,
            source: word.source,
            hanging_edges: word.hanging_edges,
            ancestor_inline_decorations: Rc::clone(&word.ancestor_inline_decorations),
        };
        push_autospace_boundary(font_system, output, previous_text, &word);
        *previous_text = AutospaceTextEdge::from_word_end(&word);
        output.push(InlineItem::Word(word));
        push_text_autospace_atom(
            font_system,
            output,
            &suffix.style,
            suffix.baseline_shift,
            suffix.visual_offset,
        );
        *previous_text = None;
        word = Box::new(suffix);
    }

    push_autospace_boundary(font_system, output, previous_text, &word);
    *previous_text = AutospaceTextEdge::from_word_end(&word);
    output.push(InlineItem::Word(word));
}

/// Find the first source position where CSS Text automatic spacing divides a
/// word. Combining marks inherit the preceding base character's context, so a
/// split always begins at the following non-inheriting scalar value.
fn first_internal_autospace_boundary(word: &InlineWord) -> Option<usize> {
    if word.style.text_autospace.is_none() {
        return None;
    }

    let mut previous = None::<char>;
    for (index, character) in word.text.char_indices() {
        if character_inherits_autospace_boundary_context(character) {
            continue;
        }
        if let Some(previous) = previous
            && text_autospace_boundary_needs_spacing(
                &word.style.text_autospace,
                previous,
                &word.style,
                character,
                &word.style,
            )
        {
            return Some(index);
        }
        previous = Some(character);
    }
    None
}

pub(in crate::layout) fn push_autospace_boundary(
    font_system: &mut FontSystem,
    output: &mut Vec<InlineItem>,
    previous_text: &mut Option<AutospaceTextEdge>,
    word: &InlineWord,
) {
    let Some(current_character) = autospace_boundary_character_at_start(&word.text) else {
        return;
    };
    if let Some(previous) = previous_text
        && text_autospace_boundary_has_eligible_character_classes(
            previous.character,
            &previous.style,
            current_character,
            &word.style,
        )
    {
        push_text_autospace_atom(
            font_system,
            output,
            &previous.style,
            previous.baseline_shift,
            previous.visual_offset,
        );
    }
}

pub(in crate::layout) fn push_text_autospace_atom(
    font_system: &mut FontSystem,
    output: &mut Vec<InlineItem>,
    style: &ComputedStyle,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
) {
    let spacing = InlineTextBoundarySpacing::new(font_system.ic_advance_for_style(style) / 8.0);
    output.push(InlineItem::Atom(Box::new(
        InlineAtom::new(
            InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(spacing)),
            style.clone(),
            None,
            InlineSize::new(spacing.advance().points(), 0.0),
            0.0,
            baseline_shift,
            None,
            None,
        )
        .with_visual_offset(visual_offset),
    )));
}

pub(in crate::layout) fn text_autospace_boundary_needs_spacing(
    autospace: &TextAutospace,
    first: char,
    first_style: &ComputedStyle,
    second: char,
    second_style: &ComputedStyle,
) -> bool {
    if autospace.is_none() {
        return false;
    }
    let first_is_ideograph = character_is_autospace_ideograph(first);
    let second_is_ideograph = character_is_autospace_ideograph(second);
    if first_is_ideograph == second_is_ideograph {
        return false;
    }
    let (other, other_style) = if first_is_ideograph {
        (second, second_style)
    } else {
        (first, first_style)
    };
    !autospace_character_is_upright_in_vertical_text(other, other_style)
        && ((autospace.ideograph_alpha && character_is_autospace_alpha(other))
            || (autospace.ideograph_numeric && character_is_autospace_numeric(other)))
}

/// Return whether two adjacent base characters could be separated by
/// `text-autospace` once their common inline scope is known.
///
/// Collection intentionally preserves these candidate boundaries even when a
/// leaf text style disables autospace: CSS Text assigns an inline-boundary
/// adjustment to the innermost common inline box, which is resolved only
/// after graph construction has recorded lexical scopes.
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>
pub(in crate::layout) fn text_autospace_boundary_has_eligible_character_classes(
    first: char,
    first_style: &ComputedStyle,
    second: char,
    second_style: &ComputedStyle,
) -> bool {
    let first_is_ideograph = character_is_autospace_ideograph(first);
    let second_is_ideograph = character_is_autospace_ideograph(second);
    if first_is_ideograph == second_is_ideograph {
        return false;
    }
    let (other, other_style) = if first_is_ideograph {
        (second, second_style)
    } else {
        (first, first_style)
    };
    !autospace_character_is_upright_in_vertical_text(other, other_style)
        && (character_is_autospace_alpha(other) || character_is_autospace_numeric(other))
}

/// Return whether `character` is upright under its own vertical text policy.
///
/// CSS Text excludes characters that are upright through `text-orientation`
/// from the non-ideographic letter and numeral classes used by
/// `text-autospace`. The character's own style is significant at inline
/// element boundaries: the containing element can own the autospace property
/// while a descendant makes the adjacent character upright.
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>
pub(in crate::layout) fn autospace_character_is_upright_in_vertical_text(
    character: char,
    style: &ComputedStyle,
) -> bool {
    match style.text_layout_policy() {
        TextLayoutPolicy::Vertical(TextOrientation::Upright) => true,
        TextLayoutPolicy::Vertical(TextOrientation::Mixed) => {
            typographic_unit_is_upright_in_mixed_orientation(character.encode_utf8(&mut [0; 4]))
        }
        TextLayoutPolicy::Horizontal
        | TextLayoutPolicy::Vertical(TextOrientation::Sideways)
        | TextLayoutPolicy::Sideways(_) => false,
    }
}

#[derive(Clone)]
pub(in crate::layout) struct AutospaceTextEdge {
    pub(in crate::layout) character: char,
    pub(in crate::layout) style: InlineStyle,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
}

impl AutospaceTextEdge {
    pub(in crate::layout) fn from_word_end(word: &InlineWord) -> Option<Self> {
        autospace_boundary_character_at_end(&word.text).map(|character| Self {
            character,
            style: Rc::clone(&word.style),
            baseline_shift: word.baseline_shift,
            visual_offset: word.visual_offset,
        })
    }
}

/// Combining marks and default-ignorable controls inherit their neighboring
/// typographic base for autospace decisions. They must stay with that base
/// when an adjacent Latin letter or number creates a spacing boundary.
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>
pub(in crate::layout) fn character_inherits_autospace_boundary_context(character: char) -> bool {
    character_is_unicode_mark(character) || character_is_default_ignorable_code_point(character)
}

pub(in crate::layout) fn autospace_boundary_character_at_start(text: &str) -> Option<char> {
    text.chars()
        .find(|character| !character_inherits_autospace_boundary_context(*character))
}

pub(in crate::layout) fn autospace_boundary_character_at_end(text: &str) -> Option<char> {
    text.chars()
        .rev()
        .find(|character| !character_inherits_autospace_boundary_context(*character))
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;

    #[test]
    fn autospace_splitting_reuses_inline_word_style_handles() {
        let mut style = ComputedStyle::initial();
        style.text_autospace = TextAutospace::NORMAL;
        let shared_style = inline_style(&style);
        let word = InlineWord {
            text: "中A".to_string(),
            style: Rc::clone(&shared_style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        };
        let mut font_system = FontSystem::new();
        let mut output = Vec::new();
        let mut previous_text = None;

        push_autospaced_word(
            &mut font_system,
            &mut output,
            Box::new(word),
            &mut previous_text,
        );

        let styles = output
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(Rc::clone(&word.style)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(styles.len(), 2);
        assert!(Rc::ptr_eq(&shared_style, &styles[0]));
        assert!(Rc::ptr_eq(&shared_style, &styles[1]));
    }

    #[test]
    fn autospace_unsplit_word_retains_word_and_text_allocations() {
        let mut style = ComputedStyle::initial();
        style.text_autospace = TextAutospace::NORMAL;
        let word = Box::new(InlineWord {
            text: "ordinary text".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        });
        let word_pointer: *const InlineWord = &*word;
        let text_pointer = word.text.as_ptr();
        let mut font_system = FontSystem::new();
        let mut output = Vec::new();
        let mut previous = None;

        push_autospaced_word(&mut font_system, &mut output, word, &mut previous);

        let [InlineItem::Word(word)] = output.as_slice() else {
            panic!("an unsplit word must remain one word item");
        };
        assert!(std::ptr::eq::<InlineWord>(&**word, word_pointer));
        assert_eq!(word.text.as_ptr(), text_pointer);
    }

    #[test]
    fn autospace_preprocessing_swaps_reusable_item_buffers() {
        fn word(style: &ComputedStyle) -> InlineItem {
            InlineItem::Word(Box::new(InlineWord {
                text: "ordinary text".to_string(),
                style: inline_style(style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new().into(),
            }))
        }

        let mut style = ComputedStyle::initial();
        style.text_autospace = TextAutospace::NORMAL;
        let mut font_system = FontSystem::new();
        let mut scratch = Vec::with_capacity(1);
        let scratch_buffer = scratch.as_ptr();
        let mut items = Vec::with_capacity(1);
        items.push(word(&style));
        let input_buffer = items.as_ptr();

        insert_text_autospace_items(&mut font_system, &mut scratch, &mut items);

        assert_eq!(items.as_ptr(), scratch_buffer);
        assert_eq!(scratch.as_ptr(), input_buffer);
        assert!(scratch.is_empty());

        items.clear();
        items.push(word(&style));
        let next_input_buffer = items.as_ptr();
        let next_scratch_buffer = scratch.as_ptr();

        insert_text_autospace_items(&mut font_system, &mut scratch, &mut items);

        assert_eq!(items.as_ptr(), next_scratch_buffer);
        assert_eq!(scratch.as_ptr(), next_input_buffer);
        assert!(scratch.is_empty());
    }

    #[test]
    fn autospace_keeps_combining_marks_with_their_base_character() {
        let mut style = ComputedStyle::initial();
        style.text_autospace = TextAutospace::NORMAL;
        let word = InlineWord {
            text: "国\u{301}X".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        };
        let mut font_system = FontSystem::new();
        let mut output = Vec::new();
        let mut previous = None;

        push_autospaced_word(&mut font_system, &mut output, Box::new(word), &mut previous);

        assert!(matches!(
            output.as_slice(),
            [InlineItem::Word(first), InlineItem::Atom(atom), InlineItem::Word(last)]
                if first.text == "国\u{301}"
                    && matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(_)))
                    && last.text == "X"
        ));
    }

    #[test]
    fn autospace_edge_uses_the_selected_font_ic_advance() {
        let mut style = ComputedStyle::initial();
        style.font_size = 24.0;
        style.text_autospace = TextAutospace::NORMAL;
        let word = InlineWord {
            text: "国A".to_string(),
            style: inline_style(&style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        };
        let mut font_system = FontSystem::new();
        let expected_width = font_system.ic_advance_for_style(&style).points() / 8.0;
        let mut output = Vec::new();
        let mut previous = None;

        push_autospaced_word(&mut font_system, &mut output, Box::new(word), &mut previous);

        let spacing = output.iter().find_map(|item| match item {
            InlineItem::Atom(atom)
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(_))
                ) =>
            {
                let InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(spacing)) =
                    atom.content()
                else {
                    unreachable!("match guard selects text autospace")
                };
                Some((spacing.advance().points(), atom.size.width))
            }
            InlineItem::Word(_)
            | InlineItem::Atom(_)
            | InlineItem::Float(_)
            | InlineItem::Break(_)
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => None,
        });
        assert_eq!(spacing, Some((expected_width, expected_width)));
    }

    #[test]
    fn autospace_excludes_upright_vertical_letters_and_numerals() {
        fn word(text: &str, style: &ComputedStyle) -> InlineItem {
            InlineItem::Word(Box::new(InlineWord {
                text: text.to_string(),
                style: inline_style(style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new().into(),
            }))
        }

        fn autospace_count(items: &[InlineItem]) -> usize {
            items
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        InlineItem::Atom(atom)
                            if matches!(
                                atom.content(),
                                InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(_))
                            )
                    )
                })
                .count()
        }

        let mut upright_style = ComputedStyle::initial();
        upright_style.writing_mode = WritingMode::VerticalRl;
        upright_style.text_orientation = TextOrientation::Upright;
        upright_style.text_autospace = TextAutospace::NORMAL;
        let mut font_system = FontSystem::new();
        let mut scratch = Vec::new();

        for text in ["国X国", "国1国"] {
            let mut items = vec![word(text, &upright_style)];
            insert_text_autospace_items(&mut font_system, &mut scratch, &mut items);
            assert_eq!(autospace_count(&items), 0, "{text}: {items:?}");
        }

        let mut mixed_style = upright_style.clone();
        mixed_style.text_orientation = TextOrientation::Mixed;
        let mut mixed_items = vec![word("国X国", &mixed_style)];
        insert_text_autospace_items(&mut font_system, &mut scratch, &mut mixed_items);
        assert_eq!(autospace_count(&mixed_items), 2, "{mixed_items:?}");

        for text in ["X", "1"] {
            let mut items = vec![
                word("国", &mixed_style),
                word(text, &upright_style),
                word("国", &mixed_style),
            ];
            insert_text_autospace_items(&mut font_system, &mut scratch, &mut items);
            assert_eq!(autospace_count(&items), 0, "{text}: {items:?}");
        }
    }

    #[test]
    fn autospace_requires_direct_text_adjacency_across_inline_box_edges() {
        fn word(text: &str, style: &ComputedStyle) -> InlineItem {
            InlineItem::Word(Box::new(InlineWord {
                text: text.to_string(),
                style: inline_style(style),
                baseline_shift: 0.0,
                visual_offset: InlineVisualOffset::zero(),
                link_target: None,
                mergeable: true,
                source: InlineTextSource::Normal,
                hanging_edges: InlineHangingEdges::default(),
                ancestor_inline_decorations: Vec::new().into(),
            }))
        }

        fn box_edge(style: ComputedStyle) -> InlineItem {
            InlineItem::Atom(Box::new(InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                    logical_edge: InlineLogicalEdge::End,
                    physical_side: PhysicalSide::Right,
                    positioning_containing_block_id: None,
                    // Deliberately zero: a negative margin could cancel a
                    // nonzero decoration advance in a real collected edge.
                    advance: 0.0,
                    paint_extent: 0.0,
                })),
                style,
                None,
                InlineSize::new(0.0, 0.0),
                0.0,
                0.0,
                None,
                None,
            )))
        }

        fn autospace_count(items: &[InlineItem]) -> usize {
            items
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        InlineItem::Atom(atom)
                            if matches!(
                                atom.content(),
                                InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(_))
                            )
                    )
                })
                .count()
        }

        let mut text_style = ComputedStyle::initial();
        text_style.text_autospace = TextAutospace::NORMAL;
        let mut font_system = FontSystem::new();
        let mut scratch = Vec::new();

        let mut zero_edge_items = vec![
            word("国", &text_style),
            box_edge(text_style.clone()),
            word("A", &text_style),
        ];
        insert_text_autospace_items(&mut font_system, &mut scratch, &mut zero_edge_items);
        assert_eq!(autospace_count(&zero_edge_items), 1);

        let mut decorated_edge_style = text_style.clone();
        decorated_edge_style.margin.right = -2.0;
        let mut decorated_edge_items = vec![
            word("国", &text_style),
            box_edge(decorated_edge_style),
            word("A", &text_style),
        ];
        insert_text_autospace_items(&mut font_system, &mut scratch, &mut decorated_edge_items);
        assert_eq!(autospace_count(&decorated_edge_items), 0);
    }
}
