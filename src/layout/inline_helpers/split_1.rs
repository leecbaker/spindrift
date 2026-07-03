use super::*;

pub(in crate::layout) fn char_boundary_slice(
    text: &str,
    range: std::ops::Range<usize>,
) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let start = previous_char_boundary(text, range.start.min(text.len()));
    let end = next_char_boundary(text, range.end.min(text.len()));
    (start < end).then(|| text[start..end].to_string())
}

pub(in crate::layout) fn previous_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

pub(in crate::layout) fn next_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub(in crate::layout) fn inline_item_is_collapsible_space<T>(item: &T) -> bool
where
    T: AsRef<InlineItem> + ?Sized,
{
    matches!(
        item.as_ref(),
        InlineItem::Word(word)
            if word.style.white_space.collapses_spaces()
                && word.text.chars().all(is_css_collapsible_whitespace)
    )
}

pub(in crate::layout) fn trim_inline_item_edges<T>(items: &mut Vec<T>)
where
    T: AsRef<InlineItem>,
{
    let first_kept = items
        .iter()
        .position(|item| !inline_item_is_collapsible_space(item));
    match first_kept {
        Some(0) => {}
        Some(index) => {
            items.drain(..index);
        }
        None => {
            items.clear();
            return;
        }
    }
    trim_trailing_inline_spaces(items);
}

pub(in crate::layout) fn trim_trailing_inline_spaces<T>(items: &mut Vec<T>)
where
    T: AsRef<InlineItem>,
{
    while items.last().is_some_and(inline_item_is_collapsible_space) {
        items.pop();
    }
}

pub(in crate::layout) fn inline_line_item_is_collapsible_space(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Fragment(fragment)
            if inline_fragment_is_collapsible_space(fragment)
    )
}

pub(in crate::layout) fn inline_fragment_is_collapsible_space(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    fragment.style().white_space.collapses_spaces()
        && fragment.text().chars().all(is_css_collapsible_whitespace)
}

/// Return whether a line item is a `pre-wrap` space run that can hang.
///
/// CSS Text phase II makes preserved spaces at the end of a soft-wrapped
/// `pre-wrap` line hang, while `break-spaces` explicitly keeps such spaces
/// from hanging:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(in crate::layout) fn inline_line_item_is_pre_wrap_hanging_space(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Fragment(fragment)
            if inline_fragment_is_pre_wrap_hanging_space(fragment)
    )
}

/// Return whether a fragment is a preserved `pre-wrap` edge-space run.
///
/// CSS Text phase II lets preserved spaces at the end of a soft-wrapped
/// `pre-wrap` line hang. Keeping this predicate at the fragment level lets
/// line construction and justification agree on which trailing space advances
/// are outside the formatted line measure:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(in crate::layout) fn inline_fragment_is_pre_wrap_hanging_space(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    fragment.style().white_space == WhiteSpace::PreWrap
        && fragment.text().chars().all(is_css_preserved_document_space)
}

/// Return the inline-end `letter-spacing` advance excluded from mixed lines.
///
/// CSS Text line-edge tracking is excluded only for the final text fragment;
/// atomic inline boxes do not generate character tracking:
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
pub(in crate::layout) fn trailing_letter_spacing_width_for_line_items<T>(line: &[T]) -> f32
where
    T: AsRef<InlineLineItem>,
{
    line.iter()
        .rev()
        .find_map(|item| match item.as_ref() {
            InlineLineItem::Fragment(fragment) if !fragment.text().is_empty() => Some(
                line_end_letter_spacing_width(fragment.text(), fragment.style()),
            ),
            InlineLineItem::Atom(_) => Some(0.0),
            _ => None,
        })
        .unwrap_or(0.0)
}

pub(in crate::layout) fn trailing_hanging_space_separator_width_for_line_items<T>(
    line: &[T],
    font_system: &mut FontSystem,
) -> f32
where
    T: AsRef<InlineLineItem>,
{
    let mut width = 0.0;
    for item in line.iter().rev() {
        let InlineLineItem::Fragment(fragment) = item.as_ref() else {
            break;
        };
        if fragment.text().is_empty() {
            continue;
        }
        let measured =
            trim_trailing_css_hanging_space_separators(fragment.text(), fragment.style());
        if measured.len() == fragment.text().len() {
            break;
        }
        width += font_system.measure_text(&fragment.text()[measured.len()..], fragment.style());
        if !measured.is_empty() {
            break;
        }
    }
    width
}

/// Return an atomic inline's logical inline-size in the containing line.
///
/// CSS Writing Modes maps inline-level layout to logical axes before painting
/// physical boxes. Atomic inline boxes keep physical dimensions internally,
/// so line measurement must remap them through the parent writing mode:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-inline-3/#atomic-inline>.
pub(in crate::layout) fn inline_atom_logical_inline_size(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    match containing_style.writing_mode {
        WritingMode::HorizontalTb => atom.width,
        WritingMode::VerticalRl | WritingMode::VerticalLr => atom.height,
    }
}

/// Return an atomic inline's logical block-size in the containing line.
///
/// Atomic inline boxes are stored as physical margin boxes, but line box
/// ascent/descent calculations use the logical block axis selected by the
/// parent inline formatting context:
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box> and
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::layout) fn inline_atom_logical_block_size(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    match containing_style.writing_mode {
        WritingMode::HorizontalTb => atom.height,
        WritingMode::VerticalRl | WritingMode::VerticalLr => atom.width,
    }
}

pub(in crate::layout) fn inline_atom_logical_inline_start_margin(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    inline_atom_margin_for_side(
        atom,
        inline_start_side(containing_style.writing_mode, containing_style.direction),
    )
}

pub(in crate::layout) fn inline_atom_logical_inline_end_margin(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    inline_atom_margin_for_side(
        atom,
        inline_end_side(containing_style.writing_mode, containing_style.direction),
    )
}

pub(in crate::layout) fn inline_atom_logical_block_start_margin(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    inline_atom_margin_for_side(atom, block_start_side(containing_style.writing_mode))
}

pub(in crate::layout) fn inline_atom_logical_block_end_margin(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    inline_atom_margin_for_side(atom, block_end_side(containing_style.writing_mode))
}

pub(in crate::layout) fn inline_atom_logical_border_inline_size(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    (inline_atom_logical_inline_size(atom, containing_style)
        - inline_atom_logical_inline_start_margin(atom, containing_style)
        - inline_atom_logical_inline_end_margin(atom, containing_style))
    .max(0.0)
}

pub(in crate::layout) fn inline_atom_logical_border_block_size(
    atom: &InlineAtom,
    containing_style: &ComputedStyle,
) -> f32 {
    (inline_atom_logical_block_size(atom, containing_style)
        - inline_atom_logical_block_start_margin(atom, containing_style)
        - inline_atom_logical_block_end_margin(atom, containing_style))
    .max(0.0)
}

/// Return a line item's logical block-size in its containing line.
///
/// Text fragments expose `line-height` in the line block axis. Atomic inline
/// boxes expose their physical margin boxes, which must be converted to the
/// parent logical block axis before line metrics are resolved:
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::layout) fn inline_line_item_logical_block_size(
    item: &InlineLineItem,
    containing_style: &ComputedStyle,
) -> f32 {
    match item {
        InlineLineItem::Fragment(fragment) => fragment.style().line_height,
        InlineLineItem::Atom(atom)
            if matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
            ) =>
        {
            atom.style().line_height
        }
        InlineLineItem::Atom(atom) => inline_atom_logical_block_size(atom, containing_style),
        InlineLineItem::Float(_) => 0.0,
    }
}

pub(in crate::layout) fn inline_atom_margin_for_side(atom: &InlineAtom, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => atom.style().margin.top,
        PhysicalSide::Right => atom.style().margin.right,
        PhysicalSide::Bottom => atom.style().margin.bottom,
        PhysicalSide::Left => atom.style().margin.left,
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Return graph-backed max-content width for text in inline contexts.
    ///
    /// CSS Sizing defines max-content inline size from CSS Text's transformed
    /// text, white-space processing, tab advances, and hanging behavior. Use
    /// the same `InlineOpportunityGraph` measurement as inline layout instead
    /// of measuring a cleanup string directly:
    /// <https://www.w3.org/TR/css-sizing-3/#max-content-inline-size> and
    /// <https://www.w3.org/TR/css-text-3/#text-processing-order>.
    pub(in crate::layout) fn graph_max_content_text_width(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
    ) -> f32 {
        self.intrinsic_inline_measurement_for_text(text, style, available_width)
            .contribution
            .max_content
    }

    pub(in crate::layout) fn inline_boxes_max_content_width(
        &mut self,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        available_width: f32,
    ) -> f32 {
        children.iter().fold(0.0_f32, |width, child| {
            width.max(match child {
                box_tree::FormattingBox::Text(box_) => {
                    self.graph_max_content_text_width(&box_.text, &box_.style, available_width)
                }
                box_tree::FormattingBox::Inline(box_) => {
                    let child_width = self
                        .inline_boxes_max_content_width(
                            &box_.children,
                            stylesheets,
                            available_width,
                        )
                        .max(self.graph_max_content_text_width(
                            &inline_text_for_style(box_.element, &box_.style),
                            &box_.style,
                            available_width,
                        ));
                    child_width
                        + box_.style.margin.left
                        + box_.style.margin.right
                        + box_.style.padding.left
                        + box_.style.padding.right
                        + horizontal_border_width(&box_.style)
                }
                box_tree::FormattingBox::AtomicInline(box_) => {
                    if let Some(fragment) = box_.table_fragment.as_ref() {
                        let (_, max_content) = self.table_outer_intrinsic_widths_from_fragment(
                            box_.element,
                            &box_.style,
                            stylesheets,
                            fragment,
                            available_width,
                        );
                        return width.max(max_content);
                    }
                    let child_width = self
                        .inline_boxes_max_content_width(
                            &box_.children,
                            stylesheets,
                            available_width,
                        )
                        .max(self.graph_max_content_text_width(
                            &inline_text_for_style(box_.element, &box_.style),
                            &box_.style,
                            available_width,
                        ));
                    child_width
                        + box_.style.margin.left
                        + box_.style.margin.right
                        + box_.style.padding.left
                        + box_.style.padding.right
                        + horizontal_border_width(&box_.style)
                }
                box_tree::FormattingBox::AnonymousBlock(box_) => self
                    .inline_boxes_max_content_width(&box_.children, stylesheets, available_width),
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                    .inline_boxes_max_content_width(&box_.children, stylesheets, available_width),
                box_tree::FormattingBox::Block(box_) => {
                    box_.style
                        .box_values
                        .width
                        .length_if_no_percent()
                        .unwrap_or_else(|| {
                            self.inline_boxes_max_content_width(
                                &box_.children,
                                stylesheets,
                                available_width,
                            )
                            .max(self.graph_max_content_text_width(
                                &inline_text_for_style(box_.element, &box_.style),
                                &box_.style,
                                available_width,
                            ))
                        })
                        + box_.style.margin.left
                        + box_.style.margin.right
                }
                box_tree::FormattingBox::Table(box_) => {
                    let (_, max_content) = self.table_outer_intrinsic_widths_from_fragment(
                        box_.element,
                        &box_.style,
                        stylesheets,
                        &box_.fragment,
                        available_width,
                    );
                    max_content
                }
                box_tree::FormattingBox::Flex(box_) => box_
                    .style
                    .box_values
                    .width
                    .length_if_no_percent()
                    .unwrap_or_else(|| {
                        self.inline_boxes_max_content_width(
                            &box_.children,
                            stylesheets,
                            available_width,
                        )
                        .max(box_.style.font_size)
                    }),
                box_tree::FormattingBox::Replaced(box_) => {
                    box_.style
                        .box_values
                        .width
                        .length_if_no_percent()
                        .unwrap_or(box_.style.font_size)
                        + box_.style.margin.left
                        + box_.style.margin.right
                }
            })
        })
    }
}

/// Return whether adjacent inline text fragments can share one painted line.
///
/// CSS Inline Layout creates line boxes from adjacent inline boxes, while PDF
/// text emission can keep distinct font runs inside one text object when the
/// shared line-level paint state is compatible:
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::layout) fn can_paint_inline_fragments_together(
    left: &(impl InlineFragmentAccess + ?Sized),
    right: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    left.mergeable()
        && right.mergeable()
        && inline_text_sources_are_paint_compatible(left.source(), right.source())
        && (left.baseline_shift() - right.baseline_shift()).abs() < 0.01
        && left.visual_offset() == right.visual_offset()
        && left.link_target() == right.link_target()
        && (left.style().font_size - right.style().font_size).abs() < 0.01
        && left.style().vertical_align == right.style().vertical_align
        && left.style().color == right.style().color
        && left.style().visibility == right.style().visibility
        && left.style().text_decoration == right.style().text_decoration
}

pub(in crate::layout) fn inline_text_sources_are_paint_compatible(
    left: InlineTextSource,
    right: InlineTextSource,
) -> bool {
    match (left, right) {
        (InlineTextSource::Marker, InlineTextSource::Marker) => true,
        (InlineTextSource::Marker, _) | (_, InlineTextSource::Marker) => false,
        (InlineTextSource::Normal | InlineTextSource::Generated, _) => true,
    }
}

/// Return whether fragments can stay in one pending paint-time shaping group.
///
/// Visible text with different paint state must still flush into separate PDF
/// text runs, but Unicode join controls are invisible shaping controls. Keeping
/// a join-control-only fragment next to its visible neighbors preserves CSS
/// Text boundary shaping across styled inline boxes without merging visible
/// paint state:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/alreq/#h_disjoining_enforcement>.
pub(in crate::layout) fn can_queue_inline_fragments_for_shaping(
    left: &(impl InlineFragmentAccess + ?Sized),
    right: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    if !inline_text_sources_are_paint_compatible(left.source(), right.source()) {
        return false;
    }
    can_paint_inline_fragments_together(left, right)
        || ((inline_fragment_is_join_control_only(left)
            || inline_fragment_is_join_control_only(right))
            && can_shape_inline_fragments_together(left, right))
        || ((inline_fragment_is_arabic_tatweel_only(left)
            || inline_fragment_is_arabic_tatweel_only(right))
            && can_shape_inline_fragments_together(left, right))
        || ((inline_fragment_contains_joining_context(left)
            || inline_fragment_contains_joining_context(right))
            && can_shape_inline_fragments_together(left, right))
}

/// Return whether adjacent inline fragments can be shaped as one text run.
///
/// CSS Text shaping operates over typographic runs after inline box tree
/// construction. Font/style changes can split the resulting font runs, but
/// they must not by themselves remove adjacent context for cursive-script
/// shaping; CSS Text only requires an inline-boundary break for separating
/// margin/border/padding, non-baseline alignment, or bidi isolation:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(in crate::layout) fn can_shape_inline_fragments_together(
    left: &(impl InlineFragmentAccess + ?Sized),
    right: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    if inline_fragment_is_join_control_only(left) {
        return !inline_box_edge_breaks_shaping(right.style())
            && !inline_box_bidi_isolation_breaks_shaping(right.style());
    }
    if inline_fragment_is_join_control_only(right) {
        return !inline_box_edge_breaks_shaping(left.style())
            && !inline_box_bidi_isolation_breaks_shaping(left.style());
    }
    if left.visual_offset() != right.visual_offset() {
        return false;
    }
    left.style().vertical_align == right.style().vertical_align
        && left.style().writing_mode == right.style().writing_mode
        && left.style().language == right.style().language
        && !inline_box_edge_breaks_shaping(left.style())
        && !inline_box_edge_breaks_shaping(right.style())
        && !inline_box_bidi_isolation_breaks_shaping(left.style())
        && !inline_box_bidi_isolation_breaks_shaping(right.style())
}

pub(in crate::layout) fn inline_fragment_is_join_control_only(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    !fragment.text().is_empty() && fragment.text().chars().all(character_is_join_control)
}

pub(in crate::layout) fn inline_fragment_is_arabic_tatweel_only(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    !fragment.text().is_empty() && fragment.text().chars().all(character_is_arabic_tatweel)
}

pub(in crate::layout) fn inline_fragment_contains_joining_context(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    fragment.text().chars().any(|character| {
        character_is_join_control(character) || character_is_arabic_tatweel(character)
    })
}

/// Return whether a style's bidi scope should affect inline line ordering.
///
/// HTML's UA stylesheet sets `unicode-bidi: isolate` on many block containers,
/// but a block formatting context already separates the block from surrounding
/// inline content. Inline-level scopes, block overrides, and plaintext still
/// need UAX #9 controls during line ordering:
/// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering> and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(in crate::layout) fn inline_bidi_scope_affects_line_ordering(style: &ComputedStyle) -> bool {
    bidi_control_scope_for_style(style).is_some()
        && !(style.display.is_block_level() && style.unicode_bidi == UnicodeBidi::Isolate)
}

/// Return the inline-end hanging width for `hanging-punctuation: last`.
///
/// CSS Text says a closing bracket or quote at the end of the last formatted
/// line can hang, and non-zero inline-axis padding or border between the glyph
/// and the line edge prevents hanging:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(in crate::layout) fn last_hanging_punctuation_width(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
) -> f32 {
    hanging_punctuation_widths(font_system, fragments, block_style, false, true, false).end
}

/// Return start/end hanging punctuation advances for one line.
///
/// CSS Text excludes at most one hangable glyph at each line edge from line
/// measurement. `first` affects only the first formatted line, `last` only the
/// last formatted line, `force-end` affects every line end, and `allow-end`
/// conditionally hangs only when the line would otherwise overflow:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(in crate::layout) fn hanging_punctuation_widths(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
    is_first_line: bool,
    is_last_line: bool,
    line_overflows: bool,
) -> HangingPunctuationWidths {
    HangingPunctuationWidths {
        start: first_hanging_punctuation_width(font_system, fragments, block_style, is_first_line),
        end: end_hanging_punctuation_width(
            font_system,
            fragments,
            block_style,
            is_last_line,
            line_overflows,
        ),
    }
}

pub(in crate::layout) fn first_hanging_punctuation_width(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
    is_first_line: bool,
) -> f32 {
    let fragment = fragments
        .iter()
        .find(|fragment| !trim_css_collapsible_whitespace(fragment.text()).is_empty());
    first_hanging_punctuation_width_for_fragment(font_system, fragment, block_style, is_first_line)
}

pub(in crate::layout) fn first_hanging_punctuation_width_for_fragment(
    font_system: &mut FontSystem,
    fragment: Option<&InlineFragment>,
    block_style: &ComputedStyle,
    is_first_line: bool,
) -> f32 {
    if !block_style.hanging_punctuation.first || !is_first_line {
        return 0.0;
    }
    let Some(fragment) = fragment else {
        return 0.0;
    };
    let Some(character) = trim_start_css_collapsible_whitespace(fragment.text())
        .chars()
        .next()
    else {
        return 0.0;
    };
    if !character_is_first_hangable_punctuation(character) {
        return 0.0;
    }
    if fragment.hanging_edges().blocks_start {
        return 0.0;
    }
    font_system.measure_text(&character.to_string(), fragment.style())
}

pub(in crate::layout) fn end_hanging_punctuation_width(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
    is_last_line: bool,
    line_overflows: bool,
) -> f32 {
    let fragment = fragments
        .iter()
        .rev()
        .find(|fragment| !trim_css_collapsible_whitespace(fragment.text()).is_empty());
    end_hanging_punctuation_width_for_fragment(
        font_system,
        fragment,
        block_style,
        is_last_line,
        line_overflows,
    )
}

pub(in crate::layout) fn end_hanging_punctuation_width_for_fragment(
    font_system: &mut FontSystem,
    fragment: Option<&InlineFragment>,
    block_style: &ComputedStyle,
    is_last_line: bool,
    line_overflows: bool,
) -> f32 {
    let Some(fragment) = fragment else {
        return 0.0;
    };
    let Some(character) = trim_end_css_collapsible_whitespace(fragment.text())
        .chars()
        .next_back()
    else {
        return 0.0;
    };
    let hangs_by_last = block_style.hanging_punctuation.last && is_last_line;
    let hangs_by_force_end =
        block_style.hanging_punctuation.force_end && character_is_hangable_stop_or_comma(character);
    let hangs_by_allow_end = block_style.hanging_punctuation.allow_end
        && line_overflows
        && character_is_hangable_stop_or_comma(character);
    if !(hangs_by_last && character_is_last_hangable_punctuation(character)
        || hangs_by_force_end
        || hangs_by_allow_end)
    {
        return 0.0;
    }
    if fragment.hanging_edges().blocks_end {
        return 0.0;
    }
    intrinsic::hanging_punctuation_character_width(font_system, character, fragment.style())
}

pub(in crate::layout) fn last_hanging_punctuation_width_for_line_items<T>(
    font_system: &mut FontSystem,
    items: &[T],
    block_style: &ComputedStyle,
) -> f32
where
    T: AsRef<InlineLineItem>,
{
    end_hanging_punctuation_width_for_line_items(font_system, items, block_style, true, false)
}

/// Return the inline-end hanging punctuation width for mixed inline items.
///
/// CSS Text applies the same hanging punctuation eligibility to inline text
/// even when that text is split across inline boxes and atomic inline items:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property> and
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(in crate::layout) fn end_hanging_punctuation_width_for_line_items<T>(
    font_system: &mut FontSystem,
    items: &[T],
    block_style: &ComputedStyle,
    is_last_line: bool,
    line_overflows: bool,
) -> f32
where
    T: AsRef<InlineLineItem>,
{
    let mut fragment = None;
    for item in items.iter().rev() {
        match item.as_ref() {
            InlineLineItem::Fragment(candidate)
                if !trim_css_collapsible_whitespace(candidate.text()).is_empty() =>
            {
                fragment = Some(candidate);
                break;
            }
            InlineLineItem::Atom(atom) if atom.content().is_box_edge() => break,
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {}
        }
    }
    end_hanging_punctuation_width_for_fragment(
        font_system,
        fragment,
        block_style,
        is_last_line,
        line_overflows,
    )
}

pub(in crate::layout) fn hanging_punctuation_widths_for_line_items<T>(
    font_system: &mut FontSystem,
    items: &[T],
    block_style: &ComputedStyle,
    is_first_line: bool,
    is_last_line: bool,
    line_overflows: bool,
) -> HangingPunctuationWidths
where
    T: AsRef<InlineLineItem>,
{
    let first_fragment = items.iter().find_map(|item| match item.as_ref() {
        InlineLineItem::Fragment(fragment)
            if !trim_css_collapsible_whitespace(fragment.text()).is_empty() =>
        {
            Some(fragment)
        }
        InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
    });
    let last_fragment = items.iter().rev().find_map(|item| match item.as_ref() {
        InlineLineItem::Fragment(fragment)
            if !trim_css_collapsible_whitespace(fragment.text()).is_empty() =>
        {
            Some(fragment)
        }
        InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
    });
    HangingPunctuationWidths {
        start: first_hanging_punctuation_width_for_fragment(
            font_system,
            first_fragment,
            block_style,
            is_first_line,
        ),
        end: end_hanging_punctuation_width_for_fragment(
            font_system,
            last_fragment,
            block_style,
            is_last_line,
            line_overflows,
        ),
    }
}

pub(in crate::layout) fn last_hanging_punctuation_width_for_inline_items(
    font_system: &mut FontSystem,
    items: &[InlineItem],
    block_style: &ComputedStyle,
) -> f32 {
    if !block_style.hanging_punctuation.last {
        return 0.0;
    }
    let mut word = None;
    for item in items.iter().rev() {
        match item {
            InlineItem::Word(candidate)
                if !trim_css_collapsible_whitespace(&candidate.text).is_empty() =>
            {
                word = Some(candidate);
                break;
            }
            InlineItem::Atom(atom) if atom.content().is_box_edge() => break,
            InlineItem::Word(_)
            | InlineItem::Atom(_)
            | InlineItem::Float(_)
            | InlineItem::Break(_)
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => {}
        }
    }
    let Some(word) = word else {
        return 0.0;
    };
    last_hanging_punctuation_width(
        font_system,
        std::slice::from_ref(&InlineFragment::new_shared_style(
            transform_text(&word.text, &word.style),
            word.style.clone(),
            word.baseline_shift,
            word.link_target.clone(),
            word.mergeable,
            word.source,
            false,
            word.hanging_edges,
            word.ancestor_inline_decorations.clone(),
        )),
        block_style,
    )
}

/// Return whether paint-time line alignment should exclude hanging punctuation.
///
/// CSS Text excludes hanging punctuation from line measurement. For
/// shrink-to-fit boxes with `width: auto`, intrinsic sizing has already
/// resolved that exclusion into the used inline size, so subtracting it again
/// during alignment double-applies the adjustment:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property> and
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic>.
pub(in crate::layout) fn line_box_uses_hanging_punctuation_alignment(
    style: &ComputedStyle,
) -> bool {
    !style.box_values.width.is_auto()
}

pub(in crate::layout) fn anonymous_inline_content_needs_normalized_style(
    style: &ComputedStyle,
) -> bool {
    (style.display.is_block_level() && style.unicode_bidi == UnicodeBidi::Isolate)
        || (!style.display.is_inline_level() && style.vertical_align != VerticalAlign::BASELINE)
}

/// Return the style used by anonymous inline text inside a block container.
///
/// CSS Inline lays out anonymous text inside a block container as inline-level
/// boxes, but properties such as table-cell `vertical-align` align the cell's
/// contents rather than shifting that anonymous text's baseline. Resetting the
/// inline-only value here prevents table-cell alignment from becoming a false
/// CSS Text shaping boundary. The block's isolate value likewise remains on
/// the block formatting context boundary instead of becoming an extra anonymous
/// inline isolation span:
/// <https://www.w3.org/TR/css-inline-3/#anonymous>,
/// <https://www.w3.org/TR/CSS22/tables.html#height-layout>, and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(in crate::layout) fn normalized_anonymous_inline_content_style(
    style: &ComputedStyle,
) -> ComputedStyle {
    let mut style = if style.display.is_block_level() && style.unicode_bidi == UnicodeBidi::Isolate
    {
        inline_content_style_without_block_isolate(style)
    } else {
        style.clone()
    };
    if !style.display.is_inline_level() {
        style.vertical_align = VerticalAlign::BASELINE;
    }
    style
}

pub(in crate::layout) fn inline_content_style_without_block_isolate(
    style: &ComputedStyle,
) -> ComputedStyle {
    let mut style = style.clone();
    style.unicode_bidi = UnicodeBidi::Normal;
    style
}

/// Return whether an inline box boundary must interrupt text shaping.
///
/// CSS Text boundary shaping allows shaping across inline boundaries unless
/// the boundary has nonzero margin, border, or padding, which creates a real
/// visual separation:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(in crate::layout) fn inline_box_edge_breaks_shaping(style: &ComputedStyle) -> bool {
    style.display.is_inline_level()
        && (max_edge(style.margin) != 0.0
            || max_edge(style.padding) != 0.0
            || used_border_width(style) != 0.0)
}

/// Return whether an inline bidi-isolation boundary interrupts shaping.
///
/// CSS Text boundary shaping treats bidi isolation boundaries as shaping
/// boundaries because isolated text is reordered as an independent bidi scope:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(in crate::layout) fn inline_box_bidi_isolation_breaks_shaping(style: &ComputedStyle) -> bool {
    style.display.is_inline_level()
        && matches!(
            style.unicode_bidi,
            UnicodeBidi::Isolate | UnicodeBidi::IsolateOverride | UnicodeBidi::Plaintext
        )
}

/// Return whether an inline fragment is an inter-word justification opportunity.
///
/// CSS Text defines justification expansion over text justification
/// opportunities, while white-space processing decides which preserved edge
/// spaces hang before alignment. Inter-word justification expands word
/// separators that remain in the formatted line, including no-break and
/// historical Unicode word separators:
/// <https://www.w3.org/TR/css-text-3/#text-justify-property> and
/// <https://www.w3.org/TR/css-text-3/#word-separator>.
pub(in crate::layout) fn inline_fragment_is_inter_word_justification_space(
    fragment: &(impl InlineFragmentAccess + ?Sized),
) -> bool {
    !fragment.generated_leader() && fragment.text().chars().all(character_is_css_word_separator)
}
