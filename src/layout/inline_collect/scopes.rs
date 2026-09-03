use std::rc::Rc;

use super::*;

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
impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_inline_box_edge_item(
        &mut self,
        style: &ComputedStyle,
        edge: InlineBoxEdge,
        positioning_containing_block_id: Option<InlinePositioningContainingBlockId>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        link_target: Option<String>,
        output: &mut Vec<InlineItem>,
    ) {
        // Inline atom advance is a line-coordinate input.
        let width = inline_box_edge_width(style, edge).points();
        // Retain zero-advance edges as lexical scope markers. CSS Text gives
        // a visual tracking boundary to the innermost inline ancestor shared
        // by its two typographic units; eliding an undecorated `span` loses
        // that ancestry even though it has no box geometry. Positioned
        // inlines additionally use the same marker for their containing
        // block, so one durable representation serves both concerns.
        // <https://www.w3.org/TR/css-text-3/#letter-spacing> and
        // <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>
        let (_, border, padding) = inline_box_edge_components(style, edge);
        let edge_fragment = InlineBoxEdgeFragment {
            logical_edge: match edge {
                InlineBoxEdge::Start => InlineLogicalEdge::Start,
                InlineBoxEdge::End => InlineLogicalEdge::End,
            },
            physical_side: inline_box_edge_physical_side(style, edge),
            positioning_containing_block_id,
            advance: width,
            paint_extent: (border + padding).max(0.0),
        };
        let baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
        output.push(InlineItem::Atom(Box::new(
            InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge_fragment)),
                style.clone(),
                None,
                InlineSize::new(width, style.line_height),
                baseline_offset,
                baseline_shift,
                link_target,
                None,
            )
            .with_visual_offset(visual_offset),
        )));
    }

    /// Emit a lexical positioned-containing-block boundary without adding a
    /// second margin/border/padding advance to the inline box.
    ///
    /// A bidi isolate may visually reorder an ordinary zero-width edge that
    /// sits outside its controls. The positioned marker therefore travels
    /// *inside* the isolate, while the real box edge remains in its normal
    /// source position outside it.
    /// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>
    /// <https://www.w3.org/TR/css-position-3/#def-cb>
    #[allow(clippy::too_many_arguments)]
    fn push_zero_advance_positioning_edge_item(
        &mut self,
        style: &ComputedStyle,
        edge: InlineBoxEdge,
        positioning_containing_block_id: InlinePositioningContainingBlockId,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        output: &mut Vec<InlineItem>,
    ) {
        let edge_fragment = InlineBoxEdgeFragment {
            logical_edge: match edge {
                InlineBoxEdge::Start => InlineLogicalEdge::Start,
                InlineBoxEdge::End => InlineLogicalEdge::End,
            },
            physical_side: inline_box_edge_physical_side(style, edge),
            positioning_containing_block_id: Some(positioning_containing_block_id),
            advance: 0.0,
            paint_extent: 0.0,
        };
        let baseline_offset = self.inline_box_text_line_layout_baseline_offset(style);
        output.push(InlineItem::Atom(Box::new(
            InlineAtom::new(
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge_fragment)),
                style.clone(),
                None,
                InlineSize::new(0.0, style.line_height),
                baseline_offset,
                baseline_shift,
                None,
                None,
            )
            .with_visual_offset(visual_offset),
        )));
    }

    /// Push the source-order opening structure of an inline scope.
    ///
    /// An inline box contributes a lexical zero-advance boundary even when it
    /// has no used box edge, followed by any UAX #9 scope control selected by
    /// `unicode-bidi`. Generated inside markers use this same structure: CSS
    /// Lists makes an inside marker an inline child, rather than a separate
    /// line or layout scope.
    /// <https://www.w3.org/TR/css-inline-3/#inline-boxes> and
    /// <https://drafts.csswg.org/css-lists-3/#marker-content>
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_inline_scope_start_items(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        positioning_containing_block_id: Option<InlinePositioningContainingBlockId>,
        include_box_edge: bool,
        output: &mut Vec<InlineItem>,
    ) {
        let positioned_bidi_isolate = positioning_containing_block_id.is_some()
            && matches!(
                style.unicode_bidi,
                UnicodeBidi::Isolate | UnicodeBidi::IsolateOverride | UnicodeBidi::Plaintext
            );
        let outer_positioning_containing_block_id = if positioned_bidi_isolate {
            None
        } else {
            positioning_containing_block_id
        };
        if include_box_edge {
            self.push_inline_box_edge_item(
                style,
                InlineBoxEdge::Start,
                outer_positioning_containing_block_id,
                baseline_shift,
                visual_offset,
                None,
                output,
            );
        }
        self.push_bidi_scope_start(style, link_target, baseline_shift, visual_offset, output);
        if positioned_bidi_isolate
            && let Some(positioning_containing_block_id) = positioning_containing_block_id
        {
            self.push_zero_advance_positioning_edge_item(
                style,
                InlineBoxEdge::Start,
                positioning_containing_block_id,
                baseline_shift,
                visual_offset,
                output,
            );
        }
    }

    /// Push the source-order closing structure of an inline scope.
    ///
    /// See [`Self::push_inline_scope_start_items`] for why generated inside
    /// markers retain the same transparent boundaries as authored inline
    /// scopes.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn push_inline_scope_end_items(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        positioning_containing_block_id: Option<InlinePositioningContainingBlockId>,
        include_box_edge: bool,
        output: &mut Vec<InlineItem>,
    ) {
        let positioned_bidi_isolate = positioning_containing_block_id.is_some()
            && matches!(
                style.unicode_bidi,
                UnicodeBidi::Isolate | UnicodeBidi::IsolateOverride | UnicodeBidi::Plaintext
            );
        let outer_positioning_containing_block_id = if positioned_bidi_isolate {
            None
        } else {
            positioning_containing_block_id
        };
        if positioned_bidi_isolate
            && let Some(positioning_containing_block_id) = positioning_containing_block_id
        {
            self.push_zero_advance_positioning_edge_item(
                style,
                InlineBoxEdge::End,
                positioning_containing_block_id,
                baseline_shift,
                visual_offset,
                output,
            );
        }
        self.push_bidi_scope_end(style, link_target, baseline_shift, visual_offset, output);
        if include_box_edge {
            self.push_inline_box_edge_item(
                style,
                InlineBoxEdge::End,
                outer_positioning_containing_block_id,
                baseline_shift,
                visual_offset,
                None,
                output,
            );
        }
    }

    pub(in crate::layout) fn begin_inline_element_scope(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        link_target: Option<String>,
        placement: InlinePlacement,
        options: InlineElementScopeOptions,
        output: &mut Vec<InlineItem>,
    ) -> InlineElementScopeState {
        let counter_snapshot = (!options.push_page_scope).then(|| self.counter_set.clone());
        let counter_scope = self.begin_counter_scope(element, style);
        let inline_box_start = output.len();
        // Inline box edges consume used margins, borders, and padding. Resolve
        // selected-font metric units before projecting those edges so `ch`,
        // `ex`, and related values do not silently become their stale
        // length-only cache value.
        // <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
        // <https://www.w3.org/TR/CSS22/visuren.html#inline-formatting>
        let mut edge_style = self.style_with_current_used_lengths(style);
        // The zero-width edge is the lexical owner of the inline box's line
        // relative alignment. Its `normal` line-height must use the same
        // selected font metrics as its text descendants; preserving `normal`
        // on this detached style can instead select the provisional fallback
        // metric. Materialize the text-used height at this layout boundary so
        // `vertical-align: bottom` cannot enlarge an otherwise identical
        // line and move adjacent text. Other used edge geometry still comes
        // from the current formatting context below.
        // <https://www.w3.org/TR/CSS22/visudet.html#propdef-line-height>
        if style.line_height_is_normal() {
            let used_line_height = self
                .font_system
                .resolved_inline_text_metrics(style)
                .line_block_size()
                .points();
            edge_style.line_height = used_line_height;
            edge_style.line_height_value = css::ComputedLineHeight::from_points(used_line_height);
        }
        let edge_percentage_basis = if options.push_page_scope {
            self.current_content_logical_inline_content_size()
        } else {
            LogicalInlineContentSize::new(content_box_pt(0.0))
        };
        apply_used_box_metrics_for_logical_inline_basis(
            &mut edge_style,
            PercentageBasis::definite(edge_percentage_basis),
        );
        // CSS 2.2's block-in-inline fixup records which logical outer-inline
        // fragment owns the original start/end edge.  That ownership remains
        // the source of truth for `box-decoration-break: slice`, whereas
        // `clone` turns *every* generated inline fragment into a complete
        // decorated box.  Apply the distinction before emitting edge atoms:
        // after collection, the inline graph cannot recover a suppressed
        // source start edge for a trailing outer-inline fragment.
        //
        // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
        // <https://www.w3.org/TR/css-break-3/#break-decoration>
        let fragment_edges = if edge_style.box_decoration_break == css::BoxDecorationBreak::Clone {
            box_tree::InlineBoxFragmentEdges::ALL
        } else {
            options.fragment_edges
        };
        let positioning_containing_block_id =
            inline_scope_establishes_positioning_containing_block(&edge_style)
                .then_some(InlinePositioningContainingBlockId(inline_box_start));
        // CSS Paged Media applies `page` only to boxes that establish a
        // class-A break opportunity. Inline boxes stay inside their enclosing
        // inline formatting context, so their declarations must not split a
        // line or materialize a named page group.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        let pushed_page_scope = false;
        if pushed_page_scope {
            output.push(InlineItem::PageScopeStart(
                style
                    .page
                    .specified_name()
                    .map(|name| name.as_str().to_string()),
            ));
        }
        self.push_inline_scope_start_items(
            &edge_style,
            link_target.clone(),
            placement.baseline_shift(),
            placement.visual_offset,
            positioning_containing_block_id,
            fragment_edges.owns_start,
            output,
        );
        if options.push_inside_marker
            && style.display.is_list_item()
            && (style.list_style_position == ListStylePosition::Inside
                || style.display.is_inline_level())
            && let Some(marker) =
                self.marker_for_list_item(element, style, self.containing_block_direction)
        {
            self.push_inside_marker_items(&marker, style, link_target.clone(), output);
        }
        InlineElementScopeState {
            inline_box_start,
            link_target,
            baseline_shift: placement.baseline_shift(),
            visual_offset: placement.visual_offset,
            edge_style: Box::new(edge_style),
            positioning_containing_block_id,
            pushed_page_scope,
            mark_hanging_edges: options.mark_hanging_edges,
            preserve_empty_metrics: options.preserve_empty_metrics,
            fragment_edges,
            counter_scope,
            counter_snapshot,
        }
    }

    pub(in crate::layout) fn end_inline_element_scope(
        &mut self,
        state: InlineElementScopeState,
        _style: &ComputedStyle,
        output: &mut Vec<InlineItem>,
    ) {
        let InlineElementScopeState {
            inline_box_start,
            link_target,
            baseline_shift,
            visual_offset,
            edge_style,
            positioning_containing_block_id,
            pushed_page_scope,
            mark_hanging_edges,
            preserve_empty_metrics,
            fragment_edges,
            counter_scope,
            counter_snapshot,
        } = state;
        self.push_inline_scope_end_items(
            &edge_style,
            link_target,
            baseline_shift,
            visual_offset,
            positioning_containing_block_id,
            fragment_edges.owns_end,
            output,
        );
        if pushed_page_scope {
            output.push(InlineItem::PageScopeEnd);
        }
        if mark_hanging_edges {
            mark_inline_box_hanging_edges(output, inline_box_start, &edge_style, fragment_edges);
        }
        // `display: contents` does not generate an inline box. Its style is
        // inherited by the promoted contents during box-tree construction,
        // but it cannot contribute an additional box decoration of its own.
        // <https://drafts.csswg.org/css-display-4/#box-generation>
        if !edge_style.display.is_contents() {
            mark_inline_box_ancestor_decorations(
                output,
                inline_box_start,
                &edge_style,
                positioning_containing_block_id,
            );
        }
        if preserve_empty_metrics
            && inline_scope_has_only_structural_items(&output[inline_box_start..])
            && let Some(atom) = output[inline_box_start..].iter_mut().find_map(|item| {
                let InlineItem::Atom(atom) = item else {
                    return None;
                };
                matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
                    if edge.advance == 0.0 && edge.paint_extent == 0.0 && !edge.is_positioning_marker())
                .then_some(atom)
            })
        {
            atom.mark_metrics_only_strut();
        }
        self.end_counter_scope(counter_scope);
        if let Some(counter_snapshot) = counter_snapshot {
            self.counter_set = counter_snapshot;
        }
    }

    pub(in crate::layout) fn collect_element_content_or_inline_items(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inherited_link: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        if style.content.is_generated() {
            self.push_element_content_items_from_dom(
                element,
                style,
                stylesheets,
                inherited_link,
                placement,
                output,
            );
        } else {
            self.collect_inline_items(
                element,
                style,
                stylesheets,
                inherited_link,
                placement,
                output,
            );
        }
    }
}

/// Whether a scope contains only bookkeeping for an otherwise empty inline
/// box. Bidi controls remain structural; text, markers, floats, and atomic
/// descendants make the scope non-empty.
fn inline_scope_has_only_structural_items(items: &[InlineItem]) -> bool {
    items.iter().all(|item| match item {
        InlineItem::Atom(atom) => matches!(
            atom.content(),
            InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
        ),
        InlineItem::Word(word) => word.source == InlineTextSource::BidiControl,
        InlineItem::StaticPositionSourceMarker(_) => true,
        InlineItem::Float(_)
        | InlineItem::Break(_)
        | InlineItem::PageScopeStart(_)
        | InlineItem::PageScopeEnd => false,
    })
}

pub(super) fn mark_inline_text_items_as_run_in(items: &mut [InlineItem]) {
    for item in items {
        if let InlineItem::Word(word) = item
            && word.source != InlineTextSource::Marker
        {
            word.source = InlineTextSource::RunIn;
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn push_bidi_scope_start(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        output: &mut Vec<InlineItem>,
    ) {
        if let Some((start, _)) = bidi_control_scope_for_style(style) {
            self.push_bidi_control_text(
                start,
                style,
                link_target,
                InlinePlacement::new(baseline_shift, visual_offset),
                output,
            );
        }
    }

    /// Push UBA end controls for a CSS `unicode-bidi` inline scope.
    ///
    /// CSS Writing Modes scopes embedding, isolation, and override controls to
    /// the element's inline box and terminates them with UAX #9 PDF/PDI
    /// controls:
    /// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
    pub(in crate::layout) fn push_bidi_scope_end(
        &mut self,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        output: &mut Vec<InlineItem>,
    ) {
        if let Some((_, end)) = bidi_control_scope_for_style(style) {
            self.push_bidi_control_text(
                end,
                style,
                link_target,
                InlinePlacement::new(baseline_shift, visual_offset),
                output,
            );
        }
    }

    /// Push invisible bidi control text without CSS text transforms.
    ///
    /// Directional formatting controls are UAX #9 algorithmic input; they
    /// affect ordering but do not create visible CSS text or PDF glyphs:
    /// <https://www.unicode.org/reports/tr9/#Directional_Formatting_Characters>.
    pub(in crate::layout) fn push_bidi_control_text(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        link_target: Option<String>,
        placement: InlinePlacement,
        output: &mut Vec<InlineItem>,
    ) {
        if !text.is_empty() {
            output.push(InlineItem::Word(Box::new(InlineWord {
                text: text.to_string(),
                style: inline_style(style),
                baseline_shift: placement.baseline_shift(),
                visual_offset: placement.visual_offset,
                link_target: link_target.map(Rc::from),
                mergeable: true,
                // This is CSS-generated UAX #9 input, rather than authored
                // text. Retaining that provenance lets later line selection
                // balance only these controls across a soft wrap.
                source: InlineTextSource::BidiControl,
                hanging_edges: InlineHangingEdges::default(),
                excluded_positioning_geometry_source: None,
                ancestor_inline_decorations: Vec::new().into(),
            })));
        }
    }

    pub(in crate::layout) fn push_inline_words(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        link_target: Option<String>,
        baseline_shift: f32,
        visual_offset: InlineVisualOffset,
        output: &mut Vec<InlineItem>,
    ) {
        push_inline_words_for_style(
            text,
            style,
            link_target,
            baseline_shift,
            visual_offset,
            output,
        );
    }
}

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
}
