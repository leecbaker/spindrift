use super::*;
use crate::text::{
    SegmentBreakContext, character_is_currency_symbol, character_is_mandatory_line_break,
    segment_break_is_removable,
};
use std::rc::Rc;

/// Push CSS Text-normalized inline words into the shared inline item stream.
///
/// CSS Text white-space processing, segment breaks, visible control handling,
/// and preserved-space tokenization must be identical for normal inline
/// content, generated content, and page-margin text before all consumers build
/// an `InlineOpportunityGraph`:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
pub(in crate::layout) fn push_inline_words_for_style(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
    output: &mut Vec<InlineItem>,
) {
    // Text runs are collected directly from frozen formatting-box styles and
    // therefore do not necessarily pass through a block/replaced used-style
    // helper first. Give every run its own used-style clone so CSS `zoom`
    // reaches shaping, line metrics, and letter/word spacing exactly once.
    // <https://drafts.csswg.org/css-viewport/#zoom-property>
    let zoomed_style = css::LayoutStyle::from_computed(style).into_zoomed();
    let normalized_style;
    let style = if anonymous_inline_content_needs_normalized_style(&zoomed_style) {
        normalized_style = normalized_anonymous_inline_content_style(&zoomed_style);
        &normalized_style
    } else {
        &zoomed_style
    };
    push_inline_text_run(
        text,
        style,
        link_target,
        baseline_shift,
        visual_offset,
        output,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn word_styles(items: &[InlineItem]) -> Vec<InlineStyle> {
        items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(Rc::clone(&word.style)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn whitespace_normalization_reuses_inline_word_style_handles() {
        let style = ComputedStyle::initial();
        let mut items = Vec::new();

        push_inline_text_run(
            "A\tB",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        normalize_inline_whitespace_items(&mut items);

        let styles = word_styles(&items);
        assert_eq!(styles.len(), 3);
        assert!(Rc::ptr_eq(&styles[0], &styles[1]));
        assert!(Rc::ptr_eq(&styles[0], &styles[2]));
    }

    #[test]
    fn equal_nested_decoration_values_retain_distinct_origins() {
        let ancestor_style = ComputedStyle::initial();
        let mut ancestor_decoration = ancestor_style.text_decoration.clone();
        ancestor_decoration.underline = true;
        let ancestor_layer = css::TextDecorationLayer {
            decoration: ancestor_decoration.clone(),
            origin_style: Rc::new(ancestor_style.clone()),
        };

        let mut child_style = ComputedStyle::initial();
        child_style
            .text_decoration_layers
            .push(css::TextDecorationLayer {
                decoration: ancestor_decoration,
                origin_style: Rc::new(ancestor_style),
            });

        let chain = propagated_decoration_layers_for_child(&[ancestor_layer], &child_style);
        assert_eq!(chain.len(), 2);
        assert!(!Rc::ptr_eq(&chain[0].origin_style, &chain[1].origin_style,));
    }

    #[test]
    fn propagation_context_carries_in_flow_layers_but_stops_at_abspos() {
        let mut ancestor_style = ComputedStyle::initial();
        let mut decoration = ancestor_style.text_decoration.clone();
        decoration.underline = true;
        ancestor_style
            .text_decoration_layers
            .push(css::TextDecorationLayer {
                decoration: decoration.clone(),
                origin_style: Rc::new(ancestor_style.clone()),
            });
        let context = TextDecorationPropagationContext::from_style(&ancestor_style);

        let in_flow_style = ComputedStyle::initial();
        let propagated = context.used_child_style(&in_flow_style);
        assert_eq!(propagated.text_decoration_layers.len(), 1);
        assert!(Rc::ptr_eq(
            &propagated.text_decoration_layers[0].origin_style,
            &ancestor_style.text_decoration_layers[0].origin_style,
        ));

        let mut nested_style = ComputedStyle::initial();
        nested_style
            .text_decoration_layers
            .push(css::TextDecorationLayer {
                decoration,
                origin_style: Rc::new(ComputedStyle::initial()),
            });
        let nested = context.used_child_style(&nested_style);
        assert_eq!(nested.text_decoration_layers.len(), 2);

        let mut positioned_style = ComputedStyle::initial();
        positioned_style.position = Position::Absolute;
        let positioned = context.used_child_style(&positioned_style);
        assert!(positioned.text_decoration_layers.is_empty());
    }

    #[test]
    fn whitespace_normalization_keeps_slice_edges_on_source_boundary_words() {
        let style = ComputedStyle::initial();
        let edge = |logical_edge, physical_side| {
            InlineItem::Atom(Box::new(InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                    logical_edge,
                    physical_side,
                    positioning_containing_block_id: None,
                    advance: 0.0,
                    paint_extent: 0.0,
                })),
                style.clone(),
                None,
                InlineSize::new(0.0, style.line_height),
                0.0,
                0.0,
                None,
                None,
            )))
        };
        let mut items = vec![edge(InlineLogicalEdge::Start, PhysicalSide::Left)];
        push_inline_text_run(
            "target element",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        let Some(InlineItem::Word(word)) = items.last_mut() else {
            panic!("the source text should produce one inline word")
        };
        word.hanging_edges = InlineHangingEdges {
            blocks_start: true,
            blocks_end: true,
        };
        items.push(edge(InlineLogicalEdge::End, PhysicalSide::Right));

        normalize_inline_whitespace_items(&mut items);

        let words = items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some((word.text.as_str(), word.hanging_edges)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            words,
            [
                (
                    "target",
                    InlineHangingEdges {
                        blocks_start: true,
                        blocks_end: false,
                    },
                ),
                (" ", InlineHangingEdges::default()),
                (
                    "element",
                    InlineHangingEdges {
                        blocks_start: false,
                        blocks_end: true,
                    },
                ),
            ]
        );
    }

    #[test]
    fn inline_word_style_mutation_is_copy_on_write() {
        let mut style = ComputedStyle::initial();
        style.font_size = 12.0;
        let shared_style = inline_style(&style);
        let mut first = InlineWord {
            text: "A".to_string(),
            style: Rc::clone(&shared_style),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        };
        let second = InlineWord {
            text: "B".to_string(),
            style: shared_style,
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        };

        Rc::make_mut(&mut first.style).font_size = 20.0;

        assert_eq!(first.style.font_size, 20.0);
        assert_eq!(second.style.font_size, 12.0);
        assert!(!Rc::ptr_eq(&first.style, &second.style));
    }

    #[test]
    fn preserved_and_generated_segment_breaks_keep_distinct_origins() {
        let mut preserved_style = ComputedStyle::initial();
        preserved_style.white_space = WhiteSpace::PreWrap;
        let mut preserved = Vec::new();
        push_inline_text_run(
            "A\nB",
            &preserved_style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut preserved,
        );
        normalize_inline_whitespace_items(&mut preserved);
        assert!(matches!(
            preserved.get(1),
            Some(InlineItem::Break(InlineBreak {
                origin: InlineBreakOrigin::PreservedSegment,
                ..
            }))
        ));

        let mut generated = Vec::new();
        push_generated_inline_words_for_style(
            "A\nB",
            &preserved_style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut generated,
        );
        normalize_inline_whitespace_items(&mut generated);
        assert!(matches!(
            generated.get(1),
            Some(InlineItem::Break(InlineBreak {
                origin: InlineBreakOrigin::Explicit,
                ..
            }))
        ));
    }

    #[test]
    fn collapsed_space_keeps_wrap_ownership_from_a_normal_descendant() {
        let mut nowrap = ComputedStyle::initial();
        nowrap.white_space = WhiteSpace::NoWrap;
        let normal = ComputedStyle::initial();
        let mut items = Vec::new();
        push_inline_text_run(
            "12345 ",
            &nowrap,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        push_inline_text_run(
            " ",
            &normal,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );

        normalize_inline_whitespace_items(&mut items);

        let space = items
            .iter()
            .find_map(|item| match item {
                InlineItem::Word(word) if word.text == " " => Some(word),
                _ => None,
            })
            .expect("collapsed separator should remain in the stream");
        assert!(space.style.allows_soft_wrap());
    }

    #[test]
    fn forced_break_keeps_preceding_collapsed_source_space_for_phase_two() {
        let style = ComputedStyle::initial();
        let mut items = Vec::new();
        push_inline_text_run(
            "A ",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        items.push(InlineItem::Break(InlineBreak {
            clear: Clear::None,
            origin: InlineBreakOrigin::Explicit,
        }));

        normalize_inline_whitespace_items(&mut items);

        assert!(matches!(
            items.as_slice(),
            [InlineItem::Word(first), InlineItem::Word(space), InlineItem::Break(_)]
                if first.text == "A" && space.text == " "
        ));
    }

    #[test]
    fn word_space_transform_keeps_explicit_separator_distinct_from_authored_space() {
        let mut style = ComputedStyle::initial();
        style.word_space_transform = css::WordSpaceTransform {
            replacement: Some(css::WordSpaceReplacement::Space),
            auto_phrase: false,
        };
        let mut items = Vec::new();
        push_inline_text_run(
            "a\u{200b} b",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );

        normalize_inline_whitespace_items(&mut items);

        let text = items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "a  b");
    }

    #[test]
    fn word_space_transform_discards_separator_adjacent_to_forced_break() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::Pre;
        style.word_space_transform = css::WordSpaceTransform {
            replacement: Some(css::WordSpaceReplacement::IdeographicSpace),
            auto_phrase: false,
        };
        let mut items = Vec::new();
        push_inline_text_run(
            "a\u{200b}\n\u{200b}b",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );

        normalize_inline_whitespace_items(&mut items);

        let text = items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "ab");
        assert!(!text.contains('\u{3000}'));
    }

    #[test]
    fn segment_break_uses_east_asian_width_instead_of_autospace_ideographs() {
        let style = ComputedStyle::initial();
        let mut items = Vec::new();
        // Fullwidth Latin letters and CJK punctuation are not CSS Text 4
        // autospace ideographs, but CSS Text Phase I removes their segment
        // break because both sides have East Asian Width `F`, `W`, or `H`.
        push_inline_text_run(
            "Ａ\n～",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        normalize_inline_whitespace_items(&mut items);

        let text = items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "Ａ～");
    }

    #[test]
    fn segment_break_uses_neighbors_outside_collapsed_space_sequence() {
        let style = ComputedStyle::initial();
        assert!(segment_break_is_removable(SegmentBreakContext {
            before: '中',
            after: '文',
            before_is_currency_amount: false,
            language: None,
        }));
        let mut items = Vec::new();
        push_inline_text_run(
            "中  \n  文",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        normalize_inline_whitespace_items(&mut items);

        let text = items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "中文");
    }

    #[test]
    fn removable_segment_break_rejoins_one_typographic_run() {
        let mut style = ComputedStyle::initial();
        style.language = Some("ja".to_string());
        let mut items = Vec::new();
        push_inline_text_run(
            "Edge\n・\nChrome",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        normalize_inline_whitespace_items(&mut items);

        let words = items
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(words, ["Edge・Chrome"]);
    }

    #[test]
    fn uax14_bk_and_nl_controls_use_forced_breaks() {
        let style = ComputedStyle::initial();
        let mut items = Vec::new();
        push_inline_text_run(
            "1\u{000c}2\u{000b}3\u{2028}4\u{2029}5\u{0085}6",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        normalize_inline_whitespace_items(&mut items);

        assert_eq!(
            items
                .iter()
                .filter(|item| matches!(item, InlineItem::Break(_)))
                .count(),
            5
        );
    }

    #[test]
    fn cgj_remains_in_source_for_line_breaking_and_shaping_filtering() {
        let style = ComputedStyle::initial();
        let mut items = Vec::new();
        push_inline_text_run(
            "A\u{034f}",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut items,
        );
        normalize_inline_whitespace_items(&mut items);

        assert!(matches!(
            items.as_slice(),
            [InlineItem::Word(word)] if word.text == "A\u{034f}"
        ));
    }

    #[test]
    fn segment_break_skips_variation_selectors_but_not_hangul() {
        let style = ComputedStyle::initial();
        let mut removable = Vec::new();
        push_inline_text_run(
            "社\u{fe00}\n福\u{fe00}",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut removable,
        );
        normalize_inline_whitespace_items(&mut removable);
        let removable_text = removable
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(removable_text, "社\u{fe00}福\u{fe00}");

        let mut hangul = Vec::new();
        push_inline_text_run(
            "Ａ\n한",
            &style,
            None,
            0.0,
            InlineVisualOffset::zero(),
            &mut hangul,
        );
        normalize_inline_whitespace_items(&mut hangul);
        let hangul_text = hangul
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(hangul_text, "Ａ 한");
    }
}

pub(in crate::layout) fn push_inline_text_run(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
    output: &mut Vec<InlineItem>,
) {
    push_inline_text_run_with_source(
        text,
        style,
        link_target,
        baseline_shift,
        visual_offset,
        InlineTextSource::Normal,
        output,
    );
}

pub(in crate::layout) fn push_generated_inline_words_for_style(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
    output: &mut Vec<InlineItem>,
) {
    // Generated-content strings are subject to the same CSS Text white-space
    // processing as authored DOM text, including visible handling of
    // non-whitespace Unicode control characters.
    // <https://www.w3.org/TR/css-text-3/#white-space-processing>
    let text = text_with_visible_control_characters(text);
    let normalized_style;
    let style = if anonymous_inline_content_needs_normalized_style(style) {
        normalized_style = normalized_anonymous_inline_content_style(style);
        &normalized_style
    } else {
        style
    };
    push_inline_text_run_with_source(
        &text,
        style,
        link_target,
        baseline_shift,
        visual_offset,
        InlineTextSource::Generated,
        output,
    );
}

pub(in crate::layout) fn push_inline_text_run_with_source(
    text: &str,
    style: &ComputedStyle,
    link_target: Option<String>,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
    source: InlineTextSource,
    output: &mut Vec<InlineItem>,
) {
    if !text.is_empty() {
        output.push(InlineItem::Word(Box::new(InlineWord {
            text: text.to_string(),
            style: inline_style(style),
            baseline_shift,
            visual_offset,
            link_target: link_target.map(Rc::from),
            mergeable: true,
            source,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new().into(),
        })));
    }
}

/// Normalize one collected inline paragraph with CSS Text whitespace phases.
///
/// Inline collection preserves source text, generated content, inline edges,
/// bidi controls, and atomic boxes as a single item stream. This processor runs
/// before autospace and graph construction so segment-break transformation and
/// whitespace collapse can see across text nodes and transparent inline box
/// edges:
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>.
pub(in crate::layout) fn normalize_inline_whitespace_items(items: &mut Vec<InlineItem>) {
    let mut processor = InlineWhitespaceProcessor::default();
    for item in std::mem::take(items) {
        processor.push_item(item);
    }
    processor.flush();
    normalize_inline_box_hanging_edge_ownership(&mut processor.output);
    *items = processor.output;
}

/// Keep inline box edge effects on the source-adjacent text after whitespace
/// normalization splits one DOM text node into multiple inline words.
///
/// Collection marks the first and last visible text of an inline box before
/// CSS Text Phase I separates collapsible spaces from their neighboring text.
/// Copying that metadata verbatim to every resulting word makes a
/// `box-decoration-break: slice` inline paint its start and end decorations at
/// each soft-wrapped word boundary. Re-resolve the ownership against the
/// retained edge atoms so only text immediately inside the corresponding
/// source edge keeps the flag.
/// <https://www.w3.org/TR/css-text-3/#white-space-processing>
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
fn normalize_inline_box_hanging_edge_ownership(items: &mut [InlineItem]) {
    for item_index in 0..items.len() {
        let InlineItem::Word(word) = &items[item_index] else {
            continue;
        };
        if !inline_word_is_visible_for_hanging_edge(word) {
            let InlineItem::Word(word) = &mut items[item_index] else {
                unreachable!("the item was checked as an inline word")
            };
            word.hanging_edges = InlineHangingEdges::default();
            continue;
        }
        let hanging_edges = word.hanging_edges;
        let keeps_start = !hanging_edges.blocks_start
            || inline_word_is_adjacent_to_source_hanging_edge(
                items,
                item_index,
                InlineLogicalEdge::Start,
            );
        let keeps_end = !hanging_edges.blocks_end
            || inline_word_is_adjacent_to_source_hanging_edge(
                items,
                item_index,
                InlineLogicalEdge::End,
            );
        let InlineItem::Word(word) = &mut items[item_index] else {
            unreachable!("the item was checked as an inline word")
        };
        word.hanging_edges.blocks_start &= keeps_start;
        word.hanging_edges.blocks_end &= keeps_end;
    }
}

fn inline_word_is_visible_for_hanging_edge(word: &InlineWord) -> bool {
    let text = trim_css_collapsible_whitespace(&word.text);
    !text.is_empty() && !text.chars().all(character_is_bidi_format_control)
}

fn inline_word_is_adjacent_to_source_hanging_edge(
    items: &[InlineItem],
    item_index: usize,
    edge: InlineLogicalEdge,
) -> bool {
    let source_edge_is_adjacent = |item: &InlineItem| match item {
        InlineItem::Word(word) if inline_word_is_visible_for_hanging_edge(word) => Some(false),
        InlineItem::Atom(atom)
            if matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(box_edge))
                    if box_edge.logical_edge == edge
            ) =>
        {
            Some(true)
        }
        InlineItem::Word(_)
        | InlineItem::Atom(_)
        | InlineItem::Float(_)
        | InlineItem::Break(_)
        | InlineItem::PageScopeStart(_)
        | InlineItem::PageScopeEnd => None,
    };
    let result = match edge {
        InlineLogicalEdge::Start => items[..item_index]
            .iter()
            .rev()
            .find_map(source_edge_is_adjacent),
        InlineLogicalEdge::End => items[item_index + 1..]
            .iter()
            .find_map(source_edge_is_adjacent),
    };
    if let Some(result) = result {
        return result;
    }
    // Split inline fragments can own only one source edge. Preserve an
    // already-marked edge if its partner is not represented in this stream.
    true
}

#[derive(Default)]
pub(in crate::layout) struct InlineWhitespaceProcessor {
    pub(in crate::layout) output: Vec<InlineItem>,
    pub(in crate::layout) run: String,
    pub(in crate::layout) run_meta: Option<InlineTextRunMeta>,
    pub(in crate::layout) run_is_document_space: bool,
    /// Whether the last emitted source run is a virtual ASCII space produced
    /// by `word-space-transform`. Phase I must not coalesce an immediately
    /// following authored collapsible space into this layout-only separator.
    pub(in crate::layout) output_ends_with_virtual_word_space: bool,
    /// A removable segment break separated the preceding emitted word from the
    /// current source run. If their text metadata is identical, CSS Text has
    /// made them one continuous typographic run and they must be reshaped as
    /// one rather than retaining a source-formatting seam.
    pub(in crate::layout) merge_next_run_after_removed_segment_break: bool,
    pub(in crate::layout) last_text_character: Option<char>,
    pub(in crate::layout) pending_segment_break: Option<InlineTextRunMeta>,
    /// Where the current collapsible segment-break sequence owns its eventual
    /// space, if it materializes one.
    pending_segment_break_placement: PendingSegmentBreakPlacement,
    pub(in crate::layout) pending_forced_segment_break: Option<PendingForcedSegmentBreak>,
    /// An explicit U+200B whose `word-space-transform` replacement depends on
    /// the following CSS Text boundary.
    pub(in crate::layout) pending_word_space_transform: Option<PendingWordSpaceTransform>,
}

#[derive(Clone)]
pub(in crate::layout) struct InlineTextRunMeta {
    pub(in crate::layout) style: InlineStyle,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
    pub(in crate::layout) link_target: Option<Rc<str>>,
    pub(in crate::layout) mergeable: bool,
    pub(in crate::layout) source: InlineTextSource,
    pub(in crate::layout) hanging_edges: InlineHangingEdges,
    pub(in crate::layout) ancestor_inline_decorations: Rc<[InlineAncestorDecoration]>,
}

/// A source-stream insertion point, deliberately distinct from a character
/// offset or a line-item index.
#[derive(Clone, Copy)]
struct InlineStreamItemIndex(usize);

/// Ownership of a collapsed segment-break sequence crossing inline edges.
///
/// CSS Text collapses source whitespace across an inline boundary, but the
/// resulting advance is outside the following inline when the sequence began
/// before its start edge. Keeping this as state rather than inferring it from
/// adjacent zero-width atoms prevents containing-block geometry from silently
/// adopting the wrong side of the collapsed space.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-1>
#[derive(Clone, Copy, Default)]
enum PendingSegmentBreakPlacement {
    #[default]
    WithinCurrentInline,
    BeforeInlineStart(InlineStreamItemIndex),
}

#[derive(Clone, Copy, Default)]
pub(in crate::layout) struct InlinePlacement {
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
}

impl InlinePlacement {
    pub(in crate::layout) fn new(baseline_shift: f32, visual_offset: InlineVisualOffset) -> Self {
        Self {
            baseline_shift,
            visual_offset,
        }
    }

    pub(in crate::layout) fn zero() -> Self {
        Self::default()
    }

    pub(in crate::layout) fn with_added_baseline_shift(self, baseline_shift: f32) -> Self {
        Self {
            baseline_shift: self.baseline_shift + baseline_shift,
            ..self
        }
    }

    pub(in crate::layout) fn with_added_visual_offset(
        self,
        visual_offset: InlineVisualOffset,
    ) -> Self {
        Self {
            visual_offset: self.visual_offset.plus(visual_offset),
            ..self
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::layout) struct PendingForcedSegmentBreak {
    pub(in crate::layout) preserve_at_end: bool,
    pub(in crate::layout) clear: Clear,
    pub(in crate::layout) origin: InlineBreakOrigin,
}

#[derive(Clone)]
pub(in crate::layout) struct PendingWordSpaceTransform {
    pub(in crate::layout) replacement: css::WordSpaceReplacement,
    pub(in crate::layout) meta: InlineTextRunMeta,
}

#[derive(Clone)]
pub(in crate::layout) struct IntrinsicInlineCollectionContext<'a> {
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
    pub(in crate::layout) block_style: &'a ComputedStyle,
    /// Decorations originating on in-flow ancestors of the box currently
    /// being collected.  These are a box-tree paint concern, not inherited
    /// computed CSS values.
    /// <https://www.w3.org/TR/css-text-decor-4/#line-decoration>
    pub(in crate::layout) propagated_decoration_layers: Vec<css::TextDecorationLayer>,
}

impl<'a> IntrinsicInlineCollectionContext<'a> {
    pub(in crate::layout) fn with_baseline_shift(self, baseline_shift: f32) -> Self {
        Self {
            baseline_shift,
            ..self
        }
    }

    pub(in crate::layout) fn with_visual_offset(self, visual_offset: InlineVisualOffset) -> Self {
        Self {
            visual_offset,
            ..self
        }
    }

    pub(in crate::layout) fn with_block_style(self, block_style: &'a ComputedStyle) -> Self {
        Self {
            block_style,
            ..self
        }
    }

    pub(in crate::layout) fn with_propagated_decoration_layers(
        self,
        propagated_decoration_layers: Vec<css::TextDecorationLayer>,
    ) -> Self {
        Self {
            propagated_decoration_layers,
            ..self
        }
    }
}

/// Extend a decoration-origin chain when entering an in-flow box.
///
/// The `text-decoration-*` longhands are not inherited. Instead, each box
/// with a visible line becomes another painting origin whose values must be
/// retained independently through eligible descendants.
/// <https://www.w3.org/TR/css-text-decor-4/#line-decoration>
pub(in crate::layout) fn propagated_decoration_layers_for_child(
    ancestor_layers: &[css::TextDecorationLayer],
    child_style: &ComputedStyle,
) -> Vec<css::TextDecorationLayer> {
    let mut layers = ancestor_layers.to_vec();
    // A used inline style can already carry this exact ancestry after an
    // earlier collection boundary. Compare provenance identity, not CSS
    // values: two nested boxes with identical declarations are distinct line
    // origins and must both paint.
    // <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
    for child_layer in &child_style.text_decoration_layers {
        if !layers.iter().any(|ancestor_layer| {
            Rc::ptr_eq(&ancestor_layer.origin_style, &child_layer.origin_style)
        }) {
            layers.push(child_layer.clone());
        }
    }
    layers
}

/// Decoration origins that reach one in-flow formatting-context boundary.
///
/// Text-decoration lines do not inherit as computed CSS values. Instead, a
/// decorating box's used line is carried through eligible in-flow descendants,
/// retaining the originating style needed for font-relative values. Keeping
/// this state separate from the computed style makes crossing a block, table,
/// or anonymous-box boundary explicit while still materializing the result
/// only for layout and paint.
///
/// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
#[derive(Debug, Clone, Default)]
pub(in crate::layout) struct TextDecorationPropagationContext {
    layers: Vec<css::TextDecorationLayer>,
}

impl TextDecorationPropagationContext {
    pub(in crate::layout) fn from_style(style: &ComputedStyle) -> Self {
        Self {
            layers: style.text_decoration_layers.clone(),
        }
    }

    /// Enter a child formatting context.
    ///
    /// Floats, out-of-flow boxes, and atomic inline/table contents do not
    /// receive an ancestor's propagated decorations. Their own decoration
    /// origins remain paintable, so this returns a context rooted at the
    /// child's computed style rather than an empty used style.
    pub(in crate::layout) fn for_child(&self, child_style: &ComputedStyle) -> Self {
        if text_decoration_propagation_stops_at(child_style) {
            return Self::from_style(child_style);
        }
        Self {
            layers: propagated_decoration_layers_for_child(&self.layers, child_style),
        }
    }

    pub(in crate::layout) fn apply_to(&self, style: &mut ComputedStyle) {
        apply_propagated_decoration_layers(style, &self.layers);
    }

    pub(in crate::layout) fn used_child_style(&self, child_style: &ComputedStyle) -> ComputedStyle {
        let context = self.for_child(child_style);
        let mut used_style = child_style.clone();
        context.apply_to(&mut used_style);
        used_style
    }
}

fn text_decoration_propagation_stops_at(style: &ComputedStyle) -> bool {
    matches!(style.position, Position::Absolute | Position::Fixed)
        || style.float != Float::None
        || style.display.is_atomic_inline()
}

/// Apply the line-decoration origins reaching a text-producing box to its
/// used inline style. This intentionally leaves computed CSS inheritance
/// untouched.
pub(in crate::layout) fn apply_propagated_decoration_layers(
    style: &mut ComputedStyle,
    layers: &[css::TextDecorationLayer],
) {
    style.text_decoration_layers = layers.to_vec();
}

impl InlineWhitespaceProcessor {
    pub(in crate::layout) fn push_item(&mut self, item: InlineItem) {
        let role = inline_item_boundary_role(&item);
        match role {
            InlineBoundaryRole::Text => {
                let InlineItem::Word(word) = item else {
                    unreachable!("text boundary role must come from a word")
                };
                self.push_word(*word);
            }
            InlineBoundaryRole::TransparentTextBoundary
            | InlineBoundaryRole::PageScopeStart
            | InlineBoundaryRole::PageScopeEnd => {
                debug_assert!(role.is_transparent_to_whitespace());
                // A forced segment break (notably HTML `br::before`) must
                // terminate the current line before a zero-width bidi control
                // is collected. Otherwise the following inline scope's UAX #9
                // start control becomes part of the preceding line, leaving
                // the next line without its embedding, isolate, or override:
                // <https://www.w3.org/TR/css-text-3/#segment-break-transformation-rules>
                // and <https://www.unicode.org/reports/tr9/#Explicit_Levels_and_Directions>.
                if self.pending_forced_segment_break.is_some() {
                    self.emit_forced_break();
                }
                self.flush_run();
                if self.pending_segment_break.is_some()
                    && matches!(
                        &item,
                        InlineItem::Atom(atom)
                            if matches!(
                                atom.content(),
                                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
                                    if edge.logical_edge == InlineLogicalEdge::Start
                            )
                    )
                    && matches!(
                        self.pending_segment_break_placement,
                        PendingSegmentBreakPlacement::WithinCurrentInline
                    )
                {
                    self.pending_segment_break_placement =
                        PendingSegmentBreakPlacement::BeforeInlineStart(InlineStreamItemIndex(
                            self.output.len(),
                        ));
                }
                self.output.push(item);
            }
            InlineBoundaryRole::Float => {
                // A float does not itself participate in CSS Text whitespace
                // processing, but it must not cross a preceding forced
                // segment break. In particular, HTML `br` is represented by
                // generated newline content; delaying that newline past a
                // following float reverses their source order and applies
                // `clear` only after the float has already been positioned.
                // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
                // <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
                if self.pending_forced_segment_break.is_some() {
                    self.emit_forced_break();
                }
                self.discard_pending_word_space_transform();
                self.flush_run();
                self.output.push(item);
            }
            InlineBoundaryRole::OpaqueAtomic | InlineBoundaryRole::IndependentFormattingContext => {
                self.discard_pending_word_space_transform();
                self.resolve_pending_before_boundary();
                self.flush_run();
                self.output.push(item);
                if role.resets_text_context() {
                    self.reset_text_context();
                }
            }
            InlineBoundaryRole::ForcedBreak => {
                self.discard_pending_word_space_transform();
                let clear = match item {
                    InlineItem::Break(break_) => break_.clear,
                    _ => Clear::None,
                };
                self.discard_pending_segment_breaks();
                self.emit_forced_break_with_clear(clear);
            }
        }
    }

    pub(in crate::layout) fn push_word(&mut self, word: InlineWord) {
        let meta = InlineTextRunMeta {
            style: word.style,
            baseline_shift: word.baseline_shift,
            visual_offset: word.visual_offset,
            link_target: word.link_target,
            mergeable: word.mergeable,
            source: word.source,
            hanging_edges: word.hanging_edges,
            ancestor_inline_decorations: word.ancestor_inline_decorations,
        };
        let mut chars = word.text.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '\u{200b}'
                && let Some(replacement) = meta.style.word_space_transform.replacement
            {
                self.resolve_pending_word_space_transform_before_text();
                self.pending_word_space_transform = Some(PendingWordSpaceTransform {
                    replacement,
                    meta: meta.clone(),
                });
            } else if character == '\r' {
                self.discard_pending_word_space_transform();
                // CSS Text maps CR to a space; it is not a segment break in
                // authored or generated text. HTML input CRLF pairs are
                // canonicalized before collection, but retain this handling
                // for CSS strings and programmatic sources.
                if meta.style.white_space.collapses_spaces() {
                    self.push_collapsible_space(&meta);
                } else {
                    self.push_text_character(' ', &meta);
                }
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            } else if character == '\n' {
                self.discard_pending_word_space_transform();
                self.push_segment_break(&meta, false);
            } else if character == INLINE_BREAK || character_is_mandatory_line_break(character) {
                self.discard_pending_word_space_transform();
                self.push_segment_break(&meta, true);
            } else if meta.style.white_space.collapses_spaces() && matches!(character, ' ' | '\t') {
                self.resolve_pending_word_space_transform_before_text();
                self.push_collapsible_space(&meta);
            } else {
                self.resolve_pending_word_space_transform_before_text();
                self.push_text_character(character, &meta);
            }
        }
    }

    /// Materialize an explicit virtual word separator for CSS Text 4 layout.
    ///
    /// `<wbr>` is represented by generated U+200B in the HTML UA sheet, so
    /// this is deliberately applied to both authored U+200B and the generated
    /// form. The transformed character remains a text fragment long enough for
    /// the shared graph to discover its normal line-break opportunity; it is
    /// not sent through Phase I's collapsible-space coalescer.
    ///
    /// A following authored collapsible space is still handled by Phase I. The
    /// remaining source-range ownership work is tracked separately from this
    /// explicit-separator conversion:
    /// <https://drafts.csswg.org/css-text-4/#word-space-transform>.
    fn push_explicit_word_space_transform(
        &mut self,
        replacement: css::WordSpaceReplacement,
        meta: &InlineTextRunMeta,
    ) {
        match replacement {
            css::WordSpaceReplacement::Space => {
                self.flush_run();
                self.push_word_run(" ", meta);
                self.output_ends_with_virtual_word_space = true;
                self.last_text_character = Some(' ');
            }
            css::WordSpaceReplacement::IdeographicSpace => {
                self.push_text_character('\u{3000}', meta)
            }
        }
    }

    /// Expand a deferred explicit virtual separator after observing following
    /// in-flow text. Forced breaks and independent formatting contexts clear
    /// the pending separator instead:
    /// <https://drafts.csswg.org/css-text-4/#word-space-transform>.
    fn resolve_pending_word_space_transform_before_text(&mut self) {
        let Some(pending) = self.pending_word_space_transform.take() else {
            return;
        };
        if self.pending_forced_segment_break.is_some() {
            return;
        }
        self.push_explicit_word_space_transform(pending.replacement, &pending.meta);
    }

    fn discard_pending_word_space_transform(&mut self) {
        self.pending_word_space_transform = None;
    }

    pub(in crate::layout) fn push_segment_break(&mut self, meta: &InlineTextRunMeta, forced: bool) {
        self.discard_pending_word_space_transform();
        if forced || meta.style.white_space.preserves_newlines() {
            if self.pending_forced_segment_break.is_some() {
                self.emit_forced_break();
            }
            self.flush_run();
            self.pending_segment_break = None;
            self.pending_forced_segment_break = Some(PendingForcedSegmentBreak {
                // A preserved segment break creates the final line box even
                // at the end of the block. CSS Text gives `pre`, `pre-wrap`,
                // and `break-spaces` the same segment-break preservation;
                // treating only `pre` as terminally significant loses empty
                // lines from the latter two modes.
                // <https://drafts.csswg.org/css-text-3/#white-space-phase-1>
                preserve_at_end: forced
                    || meta.source.is_generated()
                    || meta.style.white_space.preserves_newlines(),
                clear: if meta.source.is_generated() {
                    meta.style.clear
                } else {
                    Clear::None
                },
                origin: if forced || meta.source.is_generated() {
                    InlineBreakOrigin::Explicit
                } else {
                    InlineBreakOrigin::PreservedSegment
                },
            });
        } else if meta.style.white_space.collapses_spaces() {
            self.flush_run();
            if self.pending_segment_break.is_none() {
                self.pending_segment_break_placement = PendingSegmentBreakPlacement::default();
            }
            self.pending_segment_break = Some(meta.clone());
        } else {
            self.push_text_character('\n', meta);
        }
    }

    pub(in crate::layout) fn push_collapsible_space(&mut self, meta: &InlineTextRunMeta) {
        if self.pending_forced_segment_break.is_some() || self.pending_segment_break.is_some() {
            return;
        }
        self.flush_run();
        if self.output_ends_with_virtual_word_space {
            self.push_word_run(" ", meta);
            self.output_ends_with_virtual_word_space = false;
            self.last_text_character = Some(' ');
            return;
        }
        if self.output_ends_at_space_or_line_start() {
            self.promote_trailing_collapsible_space_wrap(meta);
            return;
        }
        self.push_word_run(" ", meta);
        self.last_text_character = Some(' ');
    }

    /// Preserve wrap ownership from every source space that collapsed into the
    /// current separator.
    ///
    /// CSS Text collapses the glyph advances across inline boundaries, but a
    /// `normal` descendant must still provide its legal soft-wrap boundary
    /// when it merges with a preceding `nowrap` space. Keep the retained
    /// source style for shaping/painting and promote only its graph wrap mode.
    /// <https://drafts.csswg.org/css-text-3/#white-space-processing> and
    /// <https://drafts.csswg.org/css-text-3/#white-space-property>
    fn promote_trailing_collapsible_space_wrap(&mut self, meta: &InlineTextRunMeta) {
        if !meta.style.allows_soft_wrap() {
            return;
        }
        for item in self.output.iter_mut().rev() {
            match item {
                InlineItem::Word(word)
                    if word.style.white_space.collapses_spaces()
                        && word.text.chars().all(is_css_collapsible_whitespace) =>
                {
                    if !word.style.allows_soft_wrap() {
                        Rc::make_mut(&mut word.style).text_wrap_mode = css::TextWrapMode::Wrap;
                    }
                    return;
                }
                InlineItem::Atom(atom) if atom.content().is_box_edge() => {}
                InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd | InlineItem::Float(_) => {
                }
                InlineItem::Word(_) | InlineItem::Break(_) | InlineItem::Atom(_) => return,
            }
        }
    }

    pub(in crate::layout) fn push_text_character(
        &mut self,
        character: char,
        meta: &InlineTextRunMeta,
    ) {
        if character != ' ' {
            self.output_ends_with_virtual_word_space = false;
        }
        // Default-ignorable controls (notably variation selectors) do not
        // decide CSS Text segment-break transformation. Keep them in source
        // order, but defer an ordinary pending break until the following
        // relevant character is known. U+200B is the explicit CSS Text
        // exception: it itself removes an adjoining segment break.
        //
        // A preserved segment break is different: CSS Text keeps it as a
        // forced line break, and a following bidi control belongs to the new
        // line. Deferring that break would append a leading RLM/ALM/etc. to
        // the preceding line, changing `unicode-bidi: plaintext` paragraph
        // direction. Flush the already-forced boundary before retaining the
        // otherwise transparent control:
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-1> and
        // <https://www.unicode.org/reports/tr9/#P2>.
        if character_is_bidi_format_control(character)
            && self.pending_forced_segment_break.is_some()
        {
            self.emit_forced_break();
        }
        let defers_segment_break_context =
            character != '\u{200b}' && character_is_default_ignorable_code_point(character);
        if !defers_segment_break_context {
            self.resolve_pending_before_character(character);
        }
        if meta.style.white_space == WhiteSpace::BreakSpaces {
            self.flush_run();
            let mut buffer = [0; 4];
            self.push_word_run(character.encode_utf8(&mut buffer), meta);
        } else {
            let character_is_document_space = matches!(character, ' ' | '\t');
            if self.run.is_empty() {
                self.run_meta = Some(meta.clone());
                self.run_is_document_space = character_is_document_space;
            } else if self.run_is_document_space != character_is_document_space
                || !self.run_meta_matches(meta)
            {
                self.flush_run();
                self.run_meta = Some(meta.clone());
                self.run_is_document_space = character_is_document_space;
            }
            self.run.push(character);
        }
        if !character_is_bidi_format_control(character) && !defers_segment_break_context {
            self.last_text_character = Some(character);
        }
    }

    pub(in crate::layout) fn resolve_pending_before_character(&mut self, next: char) {
        if self.pending_forced_segment_break.is_some() {
            self.emit_forced_break();
        }
        let Some(meta) = self.pending_segment_break.take() else {
            return;
        };
        let placement = std::mem::take(&mut self.pending_segment_break_placement);
        let previous = self
            .last_text_character
            .filter(|character| !is_css_collapsible_whitespace(*character))
            .or_else(|| self.preceding_segment_break_character());
        if previous.is_some_and(|previous| {
            segment_break_is_removable(SegmentBreakContext {
                before: previous,
                after: next,
                before_is_currency_amount: self.preceding_segment_break_token_has_currency(),
                language: meta.style.language.as_deref(),
            })
        }) {
            self.remove_trailing_collapsible_segment_whitespace();
            self.merge_next_run_after_removed_segment_break = true;
            self.last_text_character = previous;
            return;
        }
        let output_len = self.output.len();
        self.push_collapsible_space(&meta);
        if let PendingSegmentBreakPlacement::BeforeInlineStart(InlineStreamItemIndex(
            start_edge_index,
        )) = placement
            && self.output.len() > output_len
            && matches!(self.output.last(), Some(InlineItem::Word(word)) if word.text == " ")
        {
            let space = self.output.pop().expect("checked trailing space");
            self.output.insert(start_edge_index, space);
        }
    }

    /// Remove the Phase I collapsible-space prefix of a segment-break
    /// sequence after its neighboring characters select the East Asian or
    /// U+200B removal branch. Unlike selected-line Phase II trimming, this is
    /// a source-stream transformation: the removed document whitespace does
    /// not participate in the following opportunity graph.
    fn remove_trailing_collapsible_segment_whitespace(&mut self) {
        let mut remove = Vec::new();
        for (index, item) in self.output.iter().enumerate().rev() {
            match item {
                InlineItem::Word(word)
                    if word.style.white_space.collapses_spaces()
                        && word.text.chars().all(is_css_collapsible_whitespace) =>
                {
                    remove.push(index);
                }
                InlineItem::Atom(atom) if atom.content().is_box_edge() => {}
                _ => break,
            }
        }
        for index in remove {
            self.output.remove(index);
        }
    }

    /// Return the character immediately before the complete collapsible-space
    /// and segment-break sequence currently being resolved.
    ///
    /// CSS Text's segment-break transformation examines the neighboring
    /// characters outside the sequence, so retained Phase I spaces must not
    /// replace the preceding CJK or U+200B context merely because they were
    /// emitted before the pending break. Formatting controls remain
    /// transparent, matching the neighboring-character rule:
    /// <https://drafts.csswg.org/css-text-3/#line-break-transform>.
    fn preceding_segment_break_character(&self) -> Option<char> {
        for item in self.output.iter().rev() {
            match item {
                InlineItem::Word(word) => {
                    for character in word.text.chars().rev() {
                        if is_css_collapsible_whitespace(character)
                            || (character != '\u{200b}'
                                && character_is_default_ignorable_code_point(character))
                        {
                            continue;
                        }
                        return Some(character);
                    }
                }
                InlineItem::Atom(atom) if atom.content().is_box_edge() => {}
                InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd | InlineItem::Float(_) => {
                }
                InlineItem::Break(_) | InlineItem::Atom(_) => return None,
            }
        }
        None
    }

    /// Return whether the token immediately before the pending segment break
    /// contains a currency symbol.
    ///
    /// Currency syntax commonly places the symbol before a numeric amount, so
    /// the character adjoining a later segment break may be a digit rather
    /// than the symbol itself. The segment-break resolver receives this typed
    /// token fact so Japanese/Chinese tailoring cannot erase the required
    /// separator after the complete amount.
    /// <https://drafts.csswg.org/css-text-3/#line-break-transform>
    fn preceding_segment_break_token_has_currency(&self) -> bool {
        for item in self.output.iter().rev() {
            match item {
                InlineItem::Word(word) => {
                    return word.text.chars().any(character_is_currency_symbol);
                }
                InlineItem::Atom(atom) if atom.content().is_box_edge() => {}
                InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd | InlineItem::Float(_) => {
                }
                InlineItem::Break(_) | InlineItem::Atom(_) => return false,
            }
        }
        false
    }

    pub(in crate::layout) fn resolve_pending_before_boundary(&mut self) {
        if self.pending_forced_segment_break.is_some() {
            self.emit_forced_break();
        }
        if let Some(meta) = self.pending_segment_break.take() {
            // CSS Text segment breaks collapse across inline-level atomic
            // boundaries just as they do between text runs. Atomic inline boxes
            // reset character context for CJK autospace suppression, but they
            // still occupy the inline stream for line-edge whitespace tests:
            // <https://www.w3.org/TR/css-text-3/#white-space-processing>,
            // <https://www.w3.org/TR/css-inline-3/#atomic-inline>.
            if !self.output_ends_at_space_or_line_start() {
                self.push_collapsible_space(&meta);
            }
        }
    }

    pub(in crate::layout) fn emit_forced_break(&mut self) {
        let clear = self
            .pending_forced_segment_break
            .map(|break_| break_.clear)
            .unwrap_or(Clear::None);
        let origin = self
            .pending_forced_segment_break
            .map(|break_| break_.origin)
            .unwrap_or(InlineBreakOrigin::Explicit);
        self.emit_forced_break_with_origin(clear, origin);
    }

    pub(in crate::layout) fn emit_forced_break_with_clear(&mut self, clear: Clear) {
        self.emit_forced_break_with_origin(clear, InlineBreakOrigin::Explicit);
    }

    fn emit_forced_break_with_origin(&mut self, clear: Clear, origin: InlineBreakOrigin) {
        self.flush_run();
        self.pending_forced_segment_break = None;
        self.pending_segment_break = None;
        self.merge_next_run_after_removed_segment_break = false;
        // A forced break selects a line edge, but it must not erase the
        // preceding source whitespace. CSS Text Phase II owns trimming after
        // the inline opportunity graph has selected that line. Keeping the
        // source run here lets visual ordering, inline decorations, and PDF
        // text extraction consume the same selected-edge record:
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>.
        self.output
            .push(InlineItem::Break(InlineBreak { clear, origin }));
        self.last_text_character = None;
    }

    pub(in crate::layout) fn flush(&mut self) {
        self.discard_pending_word_space_transform();
        self.flush_run();
        if self
            .pending_forced_segment_break
            .is_some_and(|break_| break_.preserve_at_end)
        {
            self.emit_forced_break();
        } else {
            self.discard_pending_segment_breaks();
        }
    }

    pub(in crate::layout) fn flush_run(&mut self) {
        if self.run.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.run);
        let meta = self
            .run_meta
            .take()
            .expect("non-empty inline text run must carry metadata");
        self.push_word_run(&text, &meta);
    }

    pub(in crate::layout) fn push_word_run(&mut self, text: &str, meta: &InlineTextRunMeta) {
        if text.is_empty() {
            return;
        }
        let text = text_with_visible_control_characters(text);
        if self.merge_next_run_after_removed_segment_break {
            self.merge_next_run_after_removed_segment_break = false;
            if let Some(InlineItem::Word(previous)) = self.output.last_mut()
                && previous.style.as_ref() == meta.style.as_ref()
                && previous.baseline_shift == meta.baseline_shift
                && previous.visual_offset == meta.visual_offset
                && previous.link_target == meta.link_target
                && previous.mergeable == meta.mergeable
                && previous.source == meta.source
                && previous.hanging_edges == meta.hanging_edges
                && previous.ancestor_inline_decorations == meta.ancestor_inline_decorations
            {
                previous.text.push_str(&text);
                return;
            }
        }
        self.output.push(InlineItem::Word(Box::new(InlineWord {
            text,
            style: Rc::clone(&meta.style),
            baseline_shift: meta.baseline_shift,
            visual_offset: meta.visual_offset,
            link_target: meta.link_target.clone(),
            mergeable: meta.mergeable,
            source: meta.source,
            hanging_edges: meta.hanging_edges,
            ancestor_inline_decorations: Rc::clone(&meta.ancestor_inline_decorations),
        })));
    }

    pub(in crate::layout) fn run_meta_matches(&self, meta: &InlineTextRunMeta) -> bool {
        self.run_meta.as_ref().is_some_and(|current| {
            current.style.as_ref() == meta.style.as_ref()
                && current.baseline_shift == meta.baseline_shift
                && current.link_target == meta.link_target
                && current.mergeable == meta.mergeable
                && current.source == meta.source
                && current.hanging_edges == meta.hanging_edges
                && current.ancestor_inline_decorations == meta.ancestor_inline_decorations
        })
    }

    pub(in crate::layout) fn discard_pending_segment_breaks(&mut self) {
        self.pending_segment_break = None;
        self.pending_segment_break_placement = PendingSegmentBreakPlacement::default();
        self.pending_forced_segment_break = None;
    }

    pub(in crate::layout) fn reset_text_context(&mut self) {
        self.last_text_character = None;
        self.discard_pending_word_space_transform();
        self.discard_pending_segment_breaks();
    }

    pub(in crate::layout) fn output_ends_at_space_or_line_start(&self) -> bool {
        for item in self.output.iter().rev() {
            match item {
                InlineItem::Atom(atom) if atom.content().is_box_edge() => {}
                InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {}
                // Floats are out of flow in CSS 2.2, so they must not block
                // CSS Text collapsible whitespace from looking through the
                // zero-width marker left in the inline stream:
                // <https://www.w3.org/TR/css-text-3/#white-space-processing>
                // and <https://www.w3.org/TR/CSS22/visuren.html#float-position>.
                InlineItem::Float(_) => {}
                InlineItem::Word(_) => return inline_item_is_collapsible_space(item),
                InlineItem::Break(_) => return true,
                InlineItem::Atom(_) => return false,
            }
        }
        true
    }
}
