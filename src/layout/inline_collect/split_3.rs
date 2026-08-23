use std::rc::Rc;

use super::*;
use crate::css::{TextLayoutPolicy, TextOrientation};

/// Return whether a block container's own bidi value needs inline controls.
///
/// HTML's UA stylesheet sets `unicode-bidi: isolate` on many block containers,
/// but a block formatting context already separates its inline formatting
/// context from surrounding inline content. Literal UAX #9 controls are still
/// needed for block-level embeddings and overrides. `plaintext` instead
/// selects the base direction of each selected bidi paragraph, so it is
/// resolved by the line bidi pass rather than by one control scope spanning
/// every paragraph in the block:
/// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering> and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(in crate::layout) fn block_bidi_scope_needs_inline_controls(style: &ComputedStyle) -> bool {
    matches!(
        style.unicode_bidi,
        UnicodeBidi::Embed | UnicodeBidi::BidiOverride | UnicodeBidi::IsolateOverride
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
    /// Retain a zero-advance strut when this otherwise-empty scope has font
    /// metrics distinct from its line-formatting parent.
    pub(in crate::layout) preserve_empty_metrics: bool,
    pub(in crate::layout) fragment_edges: box_tree::InlineBoxFragmentEdges,
}

impl InlineElementScopeOptions {
    pub(in crate::layout) const DOM_INTRINSIC: Self = Self {
        push_page_scope: false,
        push_inside_marker: true,
        mark_hanging_edges: true,
        preserve_empty_metrics: false,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };
    pub(in crate::layout) const DOM_PAINT: Self = Self {
        push_page_scope: true,
        push_inside_marker: true,
        mark_hanging_edges: true,
        preserve_empty_metrics: false,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };
    pub(in crate::layout) const BOX_PAINT: Self = Self {
        push_page_scope: true,
        push_inside_marker: true,
        mark_hanging_edges: true,
        preserve_empty_metrics: false,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };
    pub(in crate::layout) const BOX_INTRINSIC: Self = Self {
        push_page_scope: false,
        push_inside_marker: false,
        mark_hanging_edges: true,
        preserve_empty_metrics: false,
        fragment_edges: box_tree::InlineBoxFragmentEdges::ALL,
    };

    pub(in crate::layout) fn with_fragment_edges(
        mut self,
        fragment_edges: box_tree::InlineBoxFragmentEdges,
    ) -> Self {
        self.fragment_edges = fragment_edges;
        self
    }

    pub(in crate::layout) fn with_preserved_empty_metrics(mut self, preserve: bool) -> Self {
        self.preserve_empty_metrics = preserve;
        self
    }
}

/// Whether an empty inline scope establishes a strut distinct from its
/// line-formatting parent.
///
/// Most empty inline boxes are transparent and must not manufacture a line.
/// A font or line-height change, however, supplies the line's resolved
/// baseline even when it has no glyphs. Keep this predicate limited to metric
/// inputs rather than treating paint-only style differences as content.
/// <https://drafts.csswg.org/css-inline-3/#line-height>
pub(in crate::layout) fn empty_inline_scope_has_distinct_metrics(
    parent: &ComputedStyle,
    child: &ComputedStyle,
) -> bool {
    child.font_family != parent.font_family
        || child.font_size != parent.font_size
        || child.font_style != parent.font_style
        || child.font_weight != parent.font_weight
        || child.font_width != parent.font_width
        || child.font_size_adjust != parent.font_size_adjust
        || child.font_variation_settings != parent.font_variation_settings
        || child.line_height != parent.line_height
        || child.vertical_align != parent.vertical_align
}

#[derive(Debug)]
pub(in crate::layout) struct InlineElementScopeState {
    pub(in crate::layout) inline_box_start: usize,
    pub(in crate::layout) link_target: Option<String>,
    pub(in crate::layout) baseline_shift: f32,
    pub(in crate::layout) visual_offset: InlineVisualOffset,
    /// Used inline-edge metrics retained through fragment replay.
    pub(in crate::layout) edge_style: Box<css::ZoomedLayoutStyle>,
    pub(in crate::layout) positioning_containing_block_id:
        Option<InlinePositioningContainingBlockId>,
    pub(in crate::layout) pushed_page_scope: bool,
    pub(in crate::layout) mark_hanging_edges: bool,
    pub(in crate::layout) preserve_empty_metrics: bool,
    pub(in crate::layout) fragment_edges: box_tree::InlineBoxFragmentEdges,
    pub(in crate::layout) counter_scope: CounterScopeState,
    pub(in crate::layout) counter_snapshot: Option<CounterSet>,
}

impl InlineElementScopeState {
    /// Borrow the active positioned-inline source while this scope owns its
    /// used style. Deferred descendants promote the view before this state is
    /// consumed by [`LayoutBuilder::end_inline_element_scope`].
    pub(in crate::layout) fn positioning_containing_block_source(
        &self,
    ) -> Option<BorrowedInlinePositioningContainingBlockSource<'_>> {
        self.positioning_containing_block_id.map(|id| {
            BorrowedInlinePositioningContainingBlockSource {
                id,
                style: self.edge_style.as_ref(),
            }
        })
    }
}

/// Return the inline-axis contribution of one regular inline box edge.
///
/// CSS 2.2 says horizontal margin, border, and padding of inline boxes are
/// respected at the start and end of the inline box. The values may be
/// negative for margins, which WPT references use to emulate hanging
/// punctuation:
/// <https://www.w3.org/TR/CSS22/box.html#inline-boxes>.
pub(in crate::layout) fn inline_box_edge_width(
    style: &ComputedStyle,
    edge: InlineBoxEdge,
) -> LayoutLength {
    let (margin, border, padding) = inline_box_edge_components(style, edge);
    layout_pt(margin + border + padding)
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
        InlineBoxEdge::Start => inline_start_side(style.writing_mode, style.used_direction()),
        InlineBoxEdge::End => inline_end_side(style.writing_mode, style.used_direction()),
    }
}

pub(in crate::layout) fn inline_scope_establishes_positioning_containing_block(
    style: &ComputedStyle,
) -> bool {
    matches!(
        style.position,
        Position::Absolute | Position::Fixed | Position::Relative | Position::Sticky
    ) || style.has_transform()
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
    positioning_containing_block_id: Option<InlinePositioningContainingBlockId>,
) {
    let has_paint_effect_scope = style.opacity.value() < 1.0;
    // Allocate once per lexical inline box, then copy that opaque identity to
    // every descendant word.  The copied metadata survives source slicing and
    // bidi reordering without making equal-opacity siblings coalesce.
    let paint_effect_scope_id = has_paint_effect_scope.then(InlinePaintScopeId::allocate);
    if !inline_box_has_paintable_decoration(style)
        && !has_paint_effect_scope
        && positioning_containing_block_id.is_none()
    {
        return;
    }
    // Scope edges carry lexical nesting independently of the computed-style
    // snapshots used for painting. A direct text node carries its owning
    // inline's computed background and border itself; only an *outer* inline
    // scope is an ancestor decoration. Nested scopes retain that chain in
    // source order.
    // <https://www.w3.org/TR/CSS22/visuren.html#relative-positioning>
    let mut scope_depth = 0usize;
    for item in &mut output[inline_box_start..] {
        if let InlineItem::Atom(atom) = item
            && let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content()
        {
            match edge.logical_edge {
                InlineLogicalEdge::Start => {
                    scope_depth += 1;
                    continue;
                }
                InlineLogicalEdge::End => {
                    scope_depth = scope_depth.saturating_sub(1);
                    continue;
                }
            }
        }
        let Some(word) = visible_hanging_edge_word_mut(item) else {
            continue;
        };
        // DOM collection gives a direct text run the inline element's style,
        // whereas frozen atomic subtrees can retain the enclosing formatting
        // context's text style.  In the former case the word already paints
        // this background itself; in the latter, retain it as an ancestor
        // decoration even at the first lexical scope.
        let word_owns_scope_background = style.background.background_color.is_potentially_visible()
            && word.style.background.background_color == style.background.background_color;
        let paints_background_or_border =
            scope_depth > 1 || (scope_depth > 0 && !word_owns_scope_background);
        if !paints_background_or_border
            && !has_paint_effect_scope
            && positioning_containing_block_id.is_none()
        {
            continue;
        }
        let mut decorations = word.ancestor_inline_decorations.to_vec();
        decorations.push(InlineAncestorDecoration {
            style: style.clone(),
            hanging_edges: InlineHangingEdges::default(),
            paints_background_or_border,
            positioning_containing_block_id,
            paint_effect_scope_id,
        });
        word.ancestor_inline_decorations = Rc::from(decorations.into_boxed_slice());
    }
}

pub(in crate::layout) fn inline_box_has_paintable_decoration(style: &ComputedStyle) -> bool {
    style.background.background_color.is_potentially_visible()
        || style.background.background_image.is_image()
        || used_border_width(style).points() > 0.0
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

pub(in crate::layout) fn quote_pair(style: &ComputedStyle, depth: usize) -> (String, String) {
    match &style.quotes {
        Quotes::None => (String::new(), String::new()),
        Quotes::Pairs(pairs) => pairs
            .get(depth)
            .or_else(|| pairs.last())
            .cloned()
            .unwrap_or_else(default_quote_pair),
        Quotes::Auto(_) => {
            let (open, close) = style.quotes.auto_quote_pair(depth);
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
        box_tree::FormattingBox::Inline(box_) if box_.core.element.tag == "br" => {
            has_inline_container_formatting_box(&box_.core.children)
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
    use std::rc::Rc;

    use super::*;

    #[test]
    fn block_plaintext_uses_per_paragraph_bidi_resolution_without_controls() {
        let mut plaintext = ComputedStyle::initial();
        plaintext.unicode_bidi = UnicodeBidi::Plaintext;
        assert!(
            !block_bidi_scope_needs_inline_controls(&plaintext),
            "block plaintext must not wrap multiple forced paragraphs in one FSI/PDI scope"
        );

        let mut inline_plaintext = ComputedStyle::initial();
        inline_plaintext.display = Display::INLINE;
        inline_plaintext.unicode_bidi = UnicodeBidi::Plaintext;
        assert_eq!(
            bidi_control_scope_for_style(&inline_plaintext),
            Some(("\u{2068}", "\u{2069}")),
            "inline plaintext remains an isolate"
        );

        let mut override_style = ComputedStyle::initial();
        override_style.unicode_bidi = UnicodeBidi::BidiOverride;
        assert!(block_bidi_scope_needs_inline_controls(&override_style));
    }

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
