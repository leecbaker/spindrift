use super::*;

/// Remove preserved segment breaks at the end of a block container.
///
/// CSS Text preserves segment breaks for `pre`, `pre-wrap`, and
/// `break-spaces`, but the final segment break after existing line content
/// terminates the current line rather than generating an extra empty line box.
/// Trimming only that terminal segment-break suffix here leaves interior
/// authored line breaks and a sole preserved-break empty line intact:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
pub(in crate::layout) fn trim_terminal_preserved_segment_breaks<'t>(
    text: &'t str,
    style: &ComputedStyle,
) -> &'t str {
    if !style.white_space.preserves_newlines() {
        return text;
    }
    let mut end = text.len();
    while let Some((break_offset, break_len)) = last_segment_break_before(&text[..end]) {
        let suffix = &text[break_offset + break_len..end];
        if !suffix.chars().all(is_css_collapsible_whitespace) {
            break;
        }
        let full_break_offset = if text[..break_offset].ends_with('\r') {
            break_offset - '\r'.len_utf8()
        } else {
            break_offset
        };
        if full_break_offset == 0 {
            break;
        }
        end = break_offset;
        if text[..end].ends_with('\r') {
            end -= '\r'.len_utf8();
        }
    }
    &text[..end]
}

pub(in crate::layout) fn last_segment_break_before(text: &str) -> Option<(usize, usize)> {
    text.char_indices()
        .rev()
        .find(|(_, character)| matches!(*character, '\n' | '\r' | INLINE_BREAK))
        .map(|(offset, character)| (offset, character.len_utf8()))
}

/// Return whether a block container's own bidi value needs inline controls.
///
/// HTML's UA stylesheet sets `unicode-bidi: isolate` on many block containers,
/// but a block formatting context already separates its inline formatting
/// context from surrounding inline content. Literal UAX #9 controls are still
/// needed for block-level overrides and plaintext paragraph direction because
/// those values affect the inline content inside the block:
/// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering> and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(in crate::layout) fn block_bidi_scope_needs_inline_controls(style: &ComputedStyle) -> bool {
    matches!(
        style.unicode_bidi,
        UnicodeBidi::Embed
            | UnicodeBidi::BidiOverride
            | UnicodeBidi::IsolateOverride
            | UnicodeBidi::Plaintext
    )
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum InlineBoxEdge {
    Start,
    End,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct InlineElementScopeOptions {
    pub(in crate::layout) push_page_scope: bool,
    pub(in crate::layout) push_inside_marker: bool,
    pub(in crate::layout) mark_hanging_edges: bool,
    pub(in crate::layout) fragment_edges: box_tree::InlineBoxFragmentEdges,
}

impl InlineElementScopeOptions {
    pub(in crate::layout) const DOM_INTRINSIC: Self = Self {
        push_page_scope: false,
        push_inside_marker: false,
        mark_hanging_edges: true,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };
    pub(in crate::layout) const DOM_PAINT: Self = Self {
        push_page_scope: true,
        push_inside_marker: false,
        mark_hanging_edges: true,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };
    pub(in crate::layout) const BOX_PAINT: Self = Self {
        push_page_scope: true,
        push_inside_marker: true,
        mark_hanging_edges: true,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };
    pub(in crate::layout) const BOX_INTRINSIC: Self = Self {
        push_page_scope: false,
        push_inside_marker: false,
        mark_hanging_edges: true,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };

    pub(in crate::layout) fn with_fragment_edges(
        mut self,
        fragment_edges: box_tree::InlineBoxFragmentEdges,
    ) -> Self {
        self.fragment_edges = fragment_edges;
        self
    }
}

#[derive(Debug)]
pub(in crate::layout) struct InlineElementScopeState {
    pub(in crate::layout) inline_box_start: usize,
    pub(in crate::layout) link_target: Option<String>,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
    pub(in crate::layout) pushed_page_scope: bool,
    pub(in crate::layout) mark_hanging_edges: bool,
    pub(in crate::layout) fragment_edges: box_tree::InlineBoxFragmentEdges,
    pub(in crate::layout) counter_scope: CounterScopeState,
}

/// Return the inline-axis contribution of one regular inline box edge.
///
/// CSS 2.2 says horizontal margin, border, and padding of inline boxes are
/// respected at the start and end of the inline box. The values may be
/// negative for margins, which WPT references use to emulate hanging
/// punctuation:
/// <https://www.w3.org/TR/CSS22/box.html#inline-boxes>.
pub(in crate::layout) fn inline_box_edge_width(style: &ComputedStyle, edge: InlineBoxEdge) -> f32 {
    let (margin, border, padding) = inline_box_edge_components(style, edge);
    margin + border + padding
}

pub(in crate::layout) fn inline_box_edge_has_nonzero_component(
    style: &ComputedStyle,
    edge: InlineBoxEdge,
) -> bool {
    let (margin, border, padding) = inline_box_edge_components(style, edge);
    margin.abs() > 0.001 || border.abs() > 0.001 || padding.abs() > 0.001
}

pub(in crate::layout) fn inline_box_edge_components(
    style: &ComputedStyle,
    edge: InlineBoxEdge,
) -> (f32, f32, f32) {
    let side = inline_box_edge_physical_side(style, edge);
    let borders = used_border_widths(style);
    match side {
        PhysicalSide::Top => (style.margin.top, borders.top, style.padding.top),
        PhysicalSide::Right => (style.margin.right, borders.right, style.padding.right),
        PhysicalSide::Bottom => (style.margin.bottom, borders.bottom, style.padding.bottom),
        PhysicalSide::Left => (style.margin.left, borders.left, style.padding.left),
    }
}

pub(in crate::layout) fn inline_box_edge_physical_side(
    style: &ComputedStyle,
    edge: InlineBoxEdge,
) -> PhysicalSide {
    match edge {
        InlineBoxEdge::Start => inline_start_side(style.writing_mode, style.direction),
        InlineBoxEdge::End => inline_end_side(style.writing_mode, style.direction),
    }
}

/// Mark the text items blocked by an inline box's edge decorations.
///
/// CSS Text disallows hanging punctuation when inline-start or inline-end
/// padding/border separates the glyph from the line edge. The text fragment
/// itself does not own ancestor inline-box border/padding, so inline
/// collection records that edge on the first/last visible text item:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(in crate::layout) fn mark_inline_box_hanging_edges(
    output: &mut [InlineItem],
    inline_box_start: usize,
    style: &ComputedStyle,
    fragment_edges: box_tree::InlineBoxFragmentEdges,
) {
    let items = &mut output[inline_box_start..];
    let blocks_start = fragment_edges.owns_start && inline_box_blocks_hanging_start(style);
    let blocks_end = fragment_edges.owns_end && inline_box_blocks_hanging_end(style);
    let has_blocking_edge = blocks_start || blocks_end;
    let mut marked_visible_item = false;
    if blocks_start && let Some(word) = items.iter_mut().find_map(visible_hanging_edge_word_mut) {
        word.hanging_edges.blocks_start = true;
        marked_visible_item = true;
    }
    if blocks_end
        && let Some(word) = items
            .iter_mut()
            .rev()
            .find_map(visible_hanging_edge_word_mut)
    {
        word.hanging_edges.blocks_end = true;
        marked_visible_item = true;
    }
    if has_blocking_edge
        && !marked_visible_item
        && let Some(word) = output[..inline_box_start]
            .iter_mut()
            .rev()
            .find_map(visible_hanging_edge_word_mut)
    {
        word.hanging_edges.blocks_end = true;
    }
}

/// Attach ancestor inline box decorations to descendant text fragments.
///
/// CSS paints an inline box's background and border behind all of its inline
/// content, including nested inline descendants with their own computed style.
/// Text fragments already paint their own style directly, so this records only
/// ancestor styles that differ from the word's own style and leaves inline
/// start/end side paint to the explicit box-edge atoms:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-boxes> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-color>.
pub(in crate::layout) fn mark_inline_box_ancestor_decorations(
    output: &mut [InlineItem],
    inline_box_start: usize,
    style: &ComputedStyle,
) {
    if !inline_box_has_paintable_decoration(style) {
        return;
    }
    for word in output[inline_box_start..]
        .iter_mut()
        .filter_map(visible_hanging_edge_word_mut)
    {
        if word.style.as_ref() == style {
            continue;
        }
        word.ancestor_inline_decorations
            .push(InlineAncestorDecoration {
                style: style.clone(),
                hanging_edges: InlineHangingEdges::default(),
            });
    }
}

pub(in crate::layout) fn inline_box_has_paintable_decoration(style: &ComputedStyle) -> bool {
    style.background_color.is_some()
        || style.background_image.is_some()
        || used_border_width(style) > 0.0
}

pub(in crate::layout) fn visible_hanging_edge_word_mut(
    item: &mut InlineItem,
) -> Option<&mut InlineWord> {
    let InlineItem::Word(word) = item else {
        return None;
    };
    let text = trim_css_collapsible_whitespace(&word.text);
    if text.is_empty() || text.chars().all(character_is_bidi_format_control) {
        return None;
    }
    Some(word)
}

pub(in crate::layout) fn inline_box_blocks_hanging_start(style: &ComputedStyle) -> bool {
    match style.direction {
        Direction::Ltr => style.padding.left != 0.0 || style.border_widths.left != 0.0,
        Direction::Rtl => style.padding.right != 0.0 || style.border_widths.right != 0.0,
    }
}

pub(in crate::layout) fn inline_box_blocks_hanging_end(style: &ComputedStyle) -> bool {
    match style.direction {
        Direction::Ltr => style.padding.right != 0.0 || style.border_widths.right != 0.0,
        Direction::Rtl => style.padding.left != 0.0 || style.border_widths.left != 0.0,
    }
}

/// Insert CSS Text Level 4 automatic spacing into inline text item streams.
///
/// `text-autospace` creates layout spacing between Han ideographs and adjacent
/// non-ideographic letters or numbers. The spacing is modeled as an atomic
/// inline edge so it affects line fitting and paint positions without adding
/// selectable text or synthetic glyphs to the PDF output:
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>.
pub(in crate::layout) fn insert_text_autospace_items(items: &mut Vec<InlineItem>) {
    let mut output = Vec::with_capacity(items.len());
    let mut previous_text = None::<AutospaceTextEdge>;
    for item in std::mem::take(items) {
        match item {
            InlineItem::Word(word) => {
                push_autospaced_word(&mut output, *word, &mut previous_text);
            }
            InlineItem::Atom(atom) => {
                if !atom.content().is_inline_edge() || atom.width.abs() > 0.0 {
                    previous_text = None;
                }
                output.push(InlineItem::Atom(atom));
            }
            InlineItem::Float(float) => {
                previous_text = None;
                output.push(InlineItem::Float(float));
            }
            InlineItem::Break(break_) => {
                previous_text = None;
                output.push(InlineItem::Break(break_));
            }
            InlineItem::PageScopeStart(scope) => output.push(InlineItem::PageScopeStart(scope)),
            InlineItem::PageScopeEnd => output.push(InlineItem::PageScopeEnd),
        }
    }
    *items = output;
}

pub(in crate::layout) fn push_autospaced_word(
    output: &mut Vec<InlineItem>,
    word: InlineWord,
    previous_text: &mut Option<AutospaceTextEdge>,
) {
    if word.style.text_autospace.is_none() {
        push_autospace_boundary(output, previous_text, &word);
        *previous_text = AutospaceTextEdge::from_word_end(&word);
        output.push(InlineItem::Word(Box::new(word)));
        return;
    }

    let mut run = String::new();
    let mut run_end = None::<char>;
    let mut run_start_index = 0usize;
    for (index, character) in word.text.char_indices() {
        if let Some(previous) = run_end
            && text_autospace_boundary_needs_spacing(
                &word.style.text_autospace,
                previous,
                character,
            )
        {
            push_autospaced_word_run(
                output,
                &word,
                &mut run,
                previous,
                run_start_index,
                previous_text,
            );
            push_text_autospace_atom(output, &word.style, word.baseline_shift, word.visual_offset);
            *previous_text = None;
            run_start_index = index;
        }
        run.push(character);
        run_end = Some(character);
    }
    if let Some(last_character) = run_end {
        push_autospaced_word_run(
            output,
            &word,
            &mut run,
            last_character,
            run_start_index,
            previous_text,
        );
    }
}

pub(in crate::layout) fn push_autospaced_word_run(
    output: &mut Vec<InlineItem>,
    word: &InlineWord,
    run: &mut String,
    last_character: char,
    run_start_index: usize,
    previous_text: &mut Option<AutospaceTextEdge>,
) {
    if run.is_empty() {
        return;
    }
    let run_word = InlineWord {
        text: std::mem::take(run),
        style: word.style.clone(),
        baseline_shift: word.baseline_shift,
        visual_offset: word.visual_offset,
        link_target: word.link_target.clone(),
        mergeable: word.mergeable && run_start_index == 0,
        source: word.source,
        hanging_edges: word.hanging_edges,
        ancestor_inline_decorations: word.ancestor_inline_decorations.clone(),
    };
    push_autospace_boundary(output, previous_text, &run_word);
    *previous_text = Some(AutospaceTextEdge {
        character: last_character,
        style: run_word.style.clone(),
        baseline_shift: run_word.baseline_shift,
        visual_offset: run_word.visual_offset,
    });
    output.push(InlineItem::Word(Box::new(run_word)));
}

pub(in crate::layout) fn push_autospace_boundary(
    output: &mut Vec<InlineItem>,
    previous_text: &mut Option<AutospaceTextEdge>,
    word: &InlineWord,
) {
    let Some(current_character) = word.text.chars().next() else {
        return;
    };
    if let Some(previous) = previous_text
        && text_autospace_boundary_needs_spacing(
            &previous.style.text_autospace,
            previous.character,
            current_character,
        )
        && text_autospace_boundary_needs_spacing(
            &word.style.text_autospace,
            previous.character,
            current_character,
        )
    {
        push_text_autospace_atom(
            output,
            &previous.style,
            previous.baseline_shift,
            previous.visual_offset,
        );
    }
}

pub(in crate::layout) fn push_text_autospace_atom(
    output: &mut Vec<InlineItem>,
    style: &ComputedStyle,
    baseline_shift: f32,
    visual_offset: InlineVisualOffset,
) {
    output.push(InlineItem::Atom(Box::new(
        InlineAtom::new(
            InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace),
            style.clone(),
            None,
            style.font_size / 8.0,
            0.0,
            0.0,
            baseline_shift,
            None,
            None,
        )
        .with_visual_offset(visual_offset),
    )));
}

pub(in crate::layout) fn quote_pair(style: &ComputedStyle, depth: usize) -> (String, String) {
    match &style.quotes {
        Quotes::None => (String::new(), String::new()),
        Quotes::Pairs(pairs) => pairs
            .get(depth)
            .or_else(|| pairs.last())
            .cloned()
            .unwrap_or_else(default_quote_pair),
        Quotes::Auto { .. } => {
            let (open, close) = quotes::language_quote_pair(style.quotes.auto_language(), depth);
            (open.to_string(), close.to_string())
        }
    }
}

pub(in crate::layout) fn default_quote_pair() -> (String, String) {
    ("“".to_string(), "”".to_string())
}

pub(in crate::layout) fn text_autospace_boundary_needs_spacing(
    autospace: &TextAutospace,
    first: char,
    second: char,
) -> bool {
    if autospace.is_none() {
        return false;
    }
    let first_is_ideograph = character_is_autospace_ideograph(first);
    let second_is_ideograph = character_is_autospace_ideograph(second);
    if first_is_ideograph == second_is_ideograph {
        return false;
    }
    let other = if first_is_ideograph { second } else { first };
    (autospace.ideograph_alpha && character_is_autospace_alpha(other))
        || (autospace.ideograph_numeric && character_is_autospace_numeric(other))
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
        word.text.chars().last().map(|character| Self {
            character,
            style: word.style.clone(),
            baseline_shift: word.baseline_shift,
            visual_offset: word.visual_offset,
        })
    }
}

/// Return whether an atomic inline box contains nested inline formatting boxes.
///
/// CSS 2.2 lays out inline-block contents as an independent formatting
/// context. When the contents include inline child boxes, preserving those
/// boxes is required so descendant styles participate in line construction:
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>.
pub(in crate::layout) fn has_inline_container_formatting_box(
    children: &[box_tree::FormattingBox<'_>],
) -> bool {
    children.iter().any(|child| match child {
        box_tree::FormattingBox::Inline(box_) if box_.element.tag == "br" => {
            has_inline_container_formatting_box(&box_.children)
        }
        box_tree::FormattingBox::Inline(_) => true,
        box_tree::FormattingBox::Text(_) | box_tree::FormattingBox::Replaced(_) => false,
        _ => has_inline_container_formatting_box(child.children()),
    })
}

/// Return whether an atomic inline box contains positioned descendants.
///
/// CSS Positioned Layout removes absolutely positioned and fixed descendants
/// from normal flow, but they still paint in their containing stacking context.
/// Inline-block layout must therefore use the fragment-backed path whenever
/// such descendants exist, even if no in-flow child requires a block formatting
/// context:
/// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
/// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>.
pub(in crate::layout) fn has_out_of_flow_formatting_box(
    children: &[box_tree::FormattingBox<'_>],
) -> bool {
    children.iter().any(|child| {
        box_tree::is_out_of_flow_box(child) || has_out_of_flow_formatting_box(child.children())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn autospace_splitting_reuses_inline_word_style_handles() {
        let mut style = ComputedStyle::initial();
        style.text_autospace = TextAutospace::NORMAL;
        let shared_style = inline_style(&style);
        let word = InlineWord {
            text: "中A".to_string(),
            style: shared_style.clone(),
            baseline_shift: 0.0,
            visual_offset: InlineVisualOffset::zero(),
            link_target: None,
            mergeable: true,
            source: InlineTextSource::Normal,
            hanging_edges: InlineHangingEdges::default(),
            ancestor_inline_decorations: Vec::new(),
        };
        let mut output = Vec::new();
        let mut previous_text = None;

        push_autospaced_word(&mut output, word, &mut previous_text);

        let styles = output
            .iter()
            .filter_map(|item| match item {
                InlineItem::Word(word) => Some(word.style.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(styles.len(), 2);
        assert!(Rc::ptr_eq(&shared_style, &styles[0]));
        assert!(Rc::ptr_eq(&shared_style, &styles[1]));
    }
}
