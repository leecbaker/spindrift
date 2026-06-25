use super::*;
use crate::text::{
    character_is_css_word_separator, is_css_preserved_document_space,
    line_end_letter_spacing_width, trim_css_collapsible_whitespace,
    trim_end_css_collapsible_whitespace, trim_start_css_collapsible_whitespace,
    trim_trailing_css_hanging_space_separators,
};

const INLINE_LINE_WIDTH_EPSILON: f32 = 0.5;

pub(super) fn char_boundary_slice(text: &str, range: std::ops::Range<usize>) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let start = previous_char_boundary(text, range.start.min(text.len()));
    let end = next_char_boundary(text, range.end.min(text.len()));
    (start < end).then(|| text[start..end].to_string())
}

pub(super) fn previous_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

pub(super) fn next_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub(super) fn inline_item_is_collapsible_space(item: &InlineItem) -> bool {
    matches!(
        item,
        InlineItem::Word(word)
            if word.style.white_space.collapses_spaces()
                && word.text.chars().all(is_css_collapsible_whitespace)
    )
}

pub(super) fn trim_inline_item_edges(items: &mut Vec<InlineItem>) {
    while items.first().is_some_and(inline_item_is_collapsible_space) {
        items.remove(0);
    }
    trim_trailing_inline_spaces(items);
}

pub(super) fn trim_trailing_inline_spaces(items: &mut Vec<InlineItem>) {
    while items.last().is_some_and(inline_item_is_collapsible_space) {
        items.pop();
    }
}

pub(super) fn inline_line_item_is_collapsible_space(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Fragment(fragment)
            if fragment.style.white_space.collapses_spaces()
                && fragment.text.chars().all(is_css_collapsible_whitespace)
    )
}

fn inline_fragment_is_collapsible_space(fragment: &InlineFragment) -> bool {
    fragment.style.white_space.collapses_spaces()
        && fragment.text.chars().all(is_css_collapsible_whitespace)
}

/// Return whether a line item is a `pre-wrap` space run that can hang.
///
/// CSS Text phase II makes preserved spaces at the end of a soft-wrapped
/// `pre-wrap` line hang, while `break-spaces` explicitly keeps such spaces
/// from hanging:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(super) fn inline_line_item_is_pre_wrap_hanging_space(item: &InlineLineItem) -> bool {
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
pub(super) fn inline_fragment_is_pre_wrap_hanging_space(fragment: &InlineFragment) -> bool {
    fragment.style.white_space == WhiteSpace::PreWrap
        && fragment.text.chars().all(is_css_preserved_document_space)
}

pub(super) fn trim_trailing_inline_line_spaces(
    line: &mut Vec<InlineLineItem>,
    font_system: &mut FontSystem,
) -> f32 {
    let mut trimmed_width = 0.0;
    while let Some(InlineLineItem::Fragment(fragment)) = line.last()
        && fragment.style.white_space.collapses_spaces()
        && fragment.text.chars().all(is_css_collapsible_whitespace)
    {
        trimmed_width += font_system.measure_text(&fragment.text, &fragment.style);
        line.pop();
    }
    trimmed_width
}

/// Remove `pre-wrap` spaces that hang at a soft line boundary.
///
/// The CSS Text white-space phase II rules consume these preserved spaces as
/// line-break opportunities without letting their advances affect line
/// measurement or painting:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(super) fn trim_trailing_pre_wrap_hanging_inline_line_spaces(
    line: &mut Vec<InlineLineItem>,
    font_system: &mut FontSystem,
) -> f32 {
    let mut trimmed_width = 0.0;
    while let Some(item) = line.last()
        && inline_line_item_is_pre_wrap_hanging_space(item)
    {
        if let InlineLineItem::Fragment(fragment) = item {
            trimmed_width += font_system.measure_text(&fragment.text, &fragment.style);
        }
        line.pop();
    }
    trimmed_width
}

/// Return the advance excluded by CSS Text trailing space-separator hanging.
///
/// CSS Text phase II keeps trailing "other space separators" in the formatted
/// line for painting, but excludes their advance from line measurement for
/// `white-space: normal`, `nowrap`, and `pre-line`:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>.
pub(super) fn trailing_hanging_space_separator_width_for_fragments(
    fragments: &[InlineFragment],
    font_system: &mut FontSystem,
) -> f32 {
    let mut width = 0.0;
    for fragment in fragments.iter().rev() {
        if fragment.text.is_empty() {
            continue;
        }
        let measured = trim_trailing_css_hanging_space_separators(&fragment.text, &fragment.style);
        if measured.len() == fragment.text.len() {
            break;
        }
        width += font_system.measure_text(&fragment.text[measured.len()..], &fragment.style);
        if !measured.is_empty() {
            break;
        }
    }
    width
}

/// Return the inline-end `letter-spacing` advance excluded from line measure.
///
/// CSS Text applies tracking between typographic character units and excludes
/// it at line edges. Fragment-based inline layout sums raw fragment advances,
/// then subtracts only the final text fragment's trailing tracking:
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
pub(super) fn trailing_letter_spacing_width_for_fragments(fragments: &[InlineFragment]) -> f32 {
    fragments
        .iter()
        .rev()
        .find(|fragment| !fragment.text.is_empty())
        .map(|fragment| line_end_letter_spacing_width(&fragment.text, &fragment.style))
        .unwrap_or(0.0)
}

/// Measure the CSS line width of visible inline text fragments.
///
/// CSS Text trims collapsible line-edge spaces before alignment and
/// justification, while hanging space separators and line-edge tracking are
/// excluded from line measurement but can still paint:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-2> and
/// <https://www.w3.org/TR/css-text-3/#text-align-property>.
pub(super) fn inline_fragment_line_width(
    fragments: &[InlineFragment],
    font_system: &mut FontSystem,
) -> f32 {
    let width = fragments
        .iter()
        .map(|fragment| font_system.measure_text(&fragment.text, &fragment.style))
        .sum::<f32>();
    (width
        - trailing_hanging_space_separator_width_for_fragments(fragments, font_system)
        - trailing_letter_spacing_width_for_fragments(fragments))
    .max(0.0)
}

/// Return the inline-end `letter-spacing` advance excluded from mixed lines.
///
/// CSS Text line-edge tracking is excluded only for the final text fragment;
/// atomic inline boxes do not generate character tracking:
/// <https://www.w3.org/TR/css-text-3/#letter-spacing-property>.
pub(super) fn trailing_letter_spacing_width_for_line_items(line: &[InlineLineItem]) -> f32 {
    line.iter()
        .rev()
        .find_map(|item| match item {
            InlineLineItem::Fragment(fragment) if !fragment.text.is_empty() => Some(
                line_end_letter_spacing_width(&fragment.text, &fragment.style),
            ),
            InlineLineItem::Atom(_) => Some(0.0),
            _ => None,
        })
        .unwrap_or(0.0)
}

pub(super) fn trailing_hanging_space_separator_width_for_line_items(
    line: &[InlineLineItem],
    font_system: &mut FontSystem,
) -> f32 {
    let mut width = 0.0;
    for item in line.iter().rev() {
        let InlineLineItem::Fragment(fragment) = item else {
            break;
        };
        if fragment.text.is_empty() {
            continue;
        }
        let measured = trim_trailing_css_hanging_space_separators(&fragment.text, &fragment.style);
        if measured.len() == fragment.text.len() {
            break;
        }
        width += font_system.measure_text(&fragment.text[measured.len()..], &fragment.style);
        if !measured.is_empty() {
            break;
        }
    }
    width
}

/// Return whether an inline item fits on the current line.
///
/// CSS Inline line breaking places the next inline item in the current line
/// when its used inline-size fits the available measure. PDF/font/layout
/// calculations pass through separate floating-point paths, so exact
/// max-content fits get a sub-device-pixel tolerance rather than forcing a
/// spurious soft wrap:
/// <https://www.w3.org/TR/css-inline-3/#line-layout>.
pub(super) fn inline_items_fit_line(
    line_width: f32,
    item_width: f32,
    available_width: f32,
) -> bool {
    line_width + item_width <= available_width + INLINE_LINE_WIDTH_EPSILON
}

pub(super) fn inline_line_item_height(item: &InlineLineItem) -> f32 {
    match item {
        InlineLineItem::Fragment(fragment) => fragment.style.line_height,
        InlineLineItem::Atom(atom) => atom.height,
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

    pub(super) fn inline_boxes_max_content_width(
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
                        let (_, max_content) = self.table_intrinsic_widths_from_fragment(
                            box_.element,
                            &box_.style,
                            stylesheets,
                            fragment,
                            available_width,
                        );
                        return width
                            .max(max_content + box_.style.margin.left + box_.style.margin.right);
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
                box_tree::FormattingBox::Line(box_) => box_
                    .children
                    .iter()
                    .map(|text| {
                        self.graph_max_content_text_width(&text.text, &text.style, available_width)
                    })
                    .fold(0.0_f32, f32::max),
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
                    let (_, max_content) = self.table_intrinsic_widths_from_fragment(
                        box_.element,
                        &box_.style,
                        stylesheets,
                        &box_.fragment,
                        available_width,
                    );
                    max_content + box_.style.margin.left + box_.style.margin.right
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
pub(super) fn can_paint_inline_fragments_together(
    left: &InlineFragment,
    right: &InlineFragment,
) -> bool {
    left.mergeable
        && right.mergeable
        && (left.baseline_shift - right.baseline_shift).abs() < 0.01
        && left.link_target == right.link_target
        && (left.style.font_size - right.style.font_size).abs() < 0.01
        && left.style.vertical_align == right.style.vertical_align
        && left.style.color == right.style.color
        && left.style.visibility == right.style.visibility
        && left.style.text_decoration == right.style.text_decoration
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
pub(super) fn can_queue_inline_fragments_for_shaping(
    left: &InlineFragment,
    right: &InlineFragment,
) -> bool {
    can_paint_inline_fragments_together(left, right)
        || ((inline_fragment_is_join_control_only(left)
            || inline_fragment_is_join_control_only(right))
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
pub(super) fn can_shape_inline_fragments_together(
    left: &InlineFragment,
    right: &InlineFragment,
) -> bool {
    if inline_fragment_is_join_control_only(left) {
        return !inline_box_edge_breaks_shaping(&right.style)
            && !inline_box_bidi_isolation_breaks_shaping(&right.style);
    }
    if inline_fragment_is_join_control_only(right) {
        return !inline_box_edge_breaks_shaping(&left.style)
            && !inline_box_bidi_isolation_breaks_shaping(&left.style);
    }
    (left.baseline_shift - right.baseline_shift).abs() < 0.01
        && left.style.vertical_align == right.style.vertical_align
        && left.style.writing_mode == right.style.writing_mode
        && left.style.language == right.style.language
        && !inline_box_edge_breaks_shaping(&left.style)
        && !inline_box_edge_breaks_shaping(&right.style)
        && !inline_box_bidi_isolation_breaks_shaping(&left.style)
        && !inline_box_bidi_isolation_breaks_shaping(&right.style)
}

pub(super) fn inline_fragment_is_join_control_only(fragment: &InlineFragment) -> bool {
    !fragment.text.is_empty() && fragment.text.chars().all(character_is_join_control)
}

/// Return whether a style's bidi scope should affect inline line ordering.
///
/// HTML's UA stylesheet sets `unicode-bidi: isolate` on many block containers,
/// but a block formatting context already separates the block from surrounding
/// inline content. Inline-level scopes, block overrides, and plaintext still
/// need UAX #9 controls during line ordering:
/// <https://html.spec.whatwg.org/multipage/rendering.html#bidi-rendering> and
/// <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>.
pub(super) fn inline_bidi_scope_affects_line_ordering(style: &ComputedStyle) -> bool {
    bidi_control_scope_for_style(style).is_some()
        && !(style.display.is_block_level() && style.unicode_bidi == UnicodeBidi::Isolate)
}

/// Return the inline-end hanging width for `hanging-punctuation: last`.
///
/// CSS Text says a closing bracket or quote at the end of the last formatted
/// line can hang, and non-zero inline-axis padding or border between the glyph
/// and the line edge prevents hanging:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(super) fn last_hanging_punctuation_width(
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
pub(super) fn hanging_punctuation_widths(
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

/// Return the physical x offset for line-start hanging punctuation.
///
/// CSS Text defines `first` at the inline-start edge. Line alignment already
/// excludes the hanging advance from measurement. In horizontal LTR text, the
/// glyph is then painted before the measured content with a negative x offset.
/// In horizontal RTL text, the measurement exclusion moves the line's physical
/// origin toward the inline-start edge, so applying a second positive paint
/// offset would double-count the hang:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(super) fn line_start_hanging_punctuation_paint_offset(
    style: &ComputedStyle,
    hanging_width: f32,
) -> f32 {
    match style.direction {
        Direction::Ltr => -hanging_width,
        Direction::Rtl => 0.0,
    }
}

/// Return the physical x offset for line-end hanging punctuation.
///
/// CSS Text excludes inline-end hanging punctuation from the measured line
/// width. In horizontal LTR text, that leaves the line origin unchanged and the
/// glyph paints beyond the physical right edge. In horizontal RTL text, the
/// inline-end edge is physical left, so the painted line origin must move left
/// by the hanging advance:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(super) fn line_end_hanging_punctuation_paint_offset(
    style: &ComputedStyle,
    hanging_width: f32,
) -> f32 {
    match style.direction {
        Direction::Ltr => 0.0,
        Direction::Rtl => -hanging_width,
    }
}

fn first_hanging_punctuation_width(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
    is_first_line: bool,
) -> f32 {
    if !block_style.hanging_punctuation.first || !is_first_line {
        return 0.0;
    }
    let Some(fragment) = fragments
        .iter()
        .find(|fragment| !trim_css_collapsible_whitespace(&fragment.text).is_empty())
    else {
        return 0.0;
    };
    let Some(character) = trim_start_css_collapsible_whitespace(&fragment.text)
        .chars()
        .next()
    else {
        return 0.0;
    };
    if !character_is_first_hangable_punctuation(character) {
        return 0.0;
    }
    if fragment.hanging_edges.blocks_start {
        return 0.0;
    }
    font_system.measure_text(&character.to_string(), &fragment.style)
}

fn end_hanging_punctuation_width(
    font_system: &mut FontSystem,
    fragments: &[InlineFragment],
    block_style: &ComputedStyle,
    is_last_line: bool,
    line_overflows: bool,
) -> f32 {
    let Some(fragment) = fragments
        .iter()
        .rev()
        .find(|fragment| !trim_css_collapsible_whitespace(&fragment.text).is_empty())
    else {
        return 0.0;
    };
    let Some(character) = trim_end_css_collapsible_whitespace(&fragment.text)
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
    if fragment.hanging_edges.blocks_end {
        return 0.0;
    }
    intrinsic::hanging_punctuation_character_width(font_system, character, &fragment.style)
}

pub(super) fn last_hanging_punctuation_width_for_line_items(
    font_system: &mut FontSystem,
    items: &[InlineLineItem],
    block_style: &ComputedStyle,
) -> f32 {
    end_hanging_punctuation_width_for_line_items(font_system, items, block_style, true, false)
}

/// Return the inline-end hanging punctuation width for mixed inline items.
///
/// CSS Text applies the same hanging punctuation eligibility to inline text
/// even when that text is split across inline boxes and atomic inline items:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property> and
/// <https://www.w3.org/TR/css-inline-3/#line-box>.
pub(super) fn end_hanging_punctuation_width_for_line_items(
    font_system: &mut FontSystem,
    items: &[InlineLineItem],
    block_style: &ComputedStyle,
    is_last_line: bool,
    line_overflows: bool,
) -> f32 {
    let Some(fragment) = items.iter().rev().find_map(|item| match item {
        InlineLineItem::Fragment(fragment)
            if !trim_css_collapsible_whitespace(&fragment.text).is_empty() =>
        {
            Some(fragment)
        }
        InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) => None,
    }) else {
        return 0.0;
    };
    end_hanging_punctuation_width(
        font_system,
        std::slice::from_ref(fragment),
        block_style,
        is_last_line,
        line_overflows,
    )
}

pub(super) fn hanging_punctuation_widths_for_line_items(
    font_system: &mut FontSystem,
    items: &[InlineLineItem],
    block_style: &ComputedStyle,
    is_first_line: bool,
    is_last_line: bool,
    line_overflows: bool,
) -> HangingPunctuationWidths {
    let fragments = items
        .iter()
        .filter_map(|item| match item {
            InlineLineItem::Fragment(fragment) => Some(fragment.clone()),
            InlineLineItem::Atom(_) => None,
        })
        .collect::<Vec<_>>();
    hanging_punctuation_widths(
        font_system,
        &fragments,
        block_style,
        is_first_line,
        is_last_line,
        line_overflows,
    )
}

/// Return first-line start hanging width for a candidate inline line.
///
/// CSS Text excludes `hanging-punctuation: first` from line measurement, so
/// line breaking must use the same reduced measure as painting and alignment;
/// otherwise a first line can wrap too early before the punctuation is hung:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(super) fn start_hanging_punctuation_width_for_candidate_line(
    font_system: &mut FontSystem,
    line: &[InlineLineItem],
    item: &InlineLineItem,
    block_style: &ComputedStyle,
    is_first_line: bool,
) -> f32 {
    if !block_style.hanging_punctuation.first || !is_first_line {
        return 0.0;
    }
    let mut candidate = line.to_vec();
    candidate.push(item.clone());
    hanging_punctuation_widths_for_line_items(
        font_system,
        &candidate,
        block_style,
        true,
        false,
        false,
    )
    .start
}

/// Return inline-end hanging width for a candidate inline line.
///
/// CSS Text evaluates `last`, `force-end`, and `allow-end` at the formatted
/// line's inline-end edge. Inline layout therefore has to inspect the
/// candidate line after appending the next item, not only the item being
/// appended, so empty and zero-width inline boxes after punctuation do not
/// suppress allowed hanging:
/// <https://www.w3.org/TR/css-text-3/#hanging-punctuation-property>.
pub(super) fn end_hanging_punctuation_width_for_candidate_line(
    font_system: &mut FontSystem,
    line: &[InlineLineItem],
    item: &InlineLineItem,
    block_style: &ComputedStyle,
    is_last_line: bool,
    line_overflows: bool,
) -> f32 {
    if !(block_style.hanging_punctuation.last
        || block_style.hanging_punctuation.force_end
        || block_style.hanging_punctuation.allow_end)
    {
        return 0.0;
    }
    let mut candidate = line.to_vec();
    candidate.push(item.clone());
    end_hanging_punctuation_width_for_line_items(
        font_system,
        &candidate,
        block_style,
        is_last_line,
        line_overflows,
    )
}

/// Make a soft hyphen visible when it is the chosen soft-wrap boundary.
///
/// CSS Text renders U+00AD SOFT HYPHEN only when the line breaks at that
/// opportunity; otherwise it remains a shaping/line-breaking control:
/// <https://www.w3.org/TR/css-text-3/#hyphenation>.
pub(super) fn show_trailing_soft_hyphen_for_line(line: &mut [InlineLineItem]) {
    let Some(InlineLineItem::Fragment(fragment)) = line.iter_mut().rev().find(|item| match item {
        InlineLineItem::Fragment(fragment) => !fragment.text.is_empty(),
        InlineLineItem::Atom(_) => true,
    }) else {
        return;
    };
    if fragment.text.ends_with('\u{00ad}') {
        fragment.text.pop();
        fragment.text.push('-');
    }
}

/// Remove U+200B after it has contributed its CSS Text break opportunity.
///
/// HTML's UA stylesheet represents `wbr` as generated U+200B. That character
/// must influence line breaking, but it must not paint or appear in rendered
/// line summaries once the selected line has been materialized:
/// <https://html.spec.whatwg.org/multipage/rendering.html#phrasing-content-3>
/// and <https://www.w3.org/TR/css-text-3/#line-breaking>.
pub(super) fn strip_zero_width_space_from_line_items(line: &mut Vec<InlineLineItem>) {
    const ZERO_WIDTH_SPACE: char = '\u{200b}';
    let mut index = 0;
    while index < line.len() {
        let remove = match &mut line[index] {
            InlineLineItem::Fragment(fragment) => {
                if fragment.text.contains(ZERO_WIDTH_SPACE) {
                    fragment.text = fragment.text.replace(ZERO_WIDTH_SPACE, "");
                }
                fragment.text.is_empty()
            }
            InlineLineItem::Atom(_) => false,
        };
        if remove {
            line.remove(index);
        } else {
            index += 1;
        }
    }
}

/// Select formatted CSS text lines from one paragraph of inline items.
///
/// Normal inline layout, generated/page-margin text, and text-only atomic
/// inline boxes all need the same CSS Text processing order: white-space and
/// transform normalization feed the inline opportunity graph, then selected
/// graph ranges are trimmed, hung, soft-hyphen materialized, and shaped into
/// durable line records:
/// <https://www.w3.org/TR/css-text-3/#text-processing-order> and
/// <https://www.w3.org/TR/css-text-3/#line-breaking>.
pub(super) fn graph_text_lines_for_paragraph(
    font_system: &mut FontSystem,
    paragraph: &mut Vec<InlineItem>,
    style: &ComputedStyle,
    available_width: f32,
) -> Vec<TextLine> {
    trim_inline_item_edges(paragraph);
    if paragraph.is_empty() {
        return Vec::new();
    }
    let graph = inline_layout::build_inline_opportunity_graph(font_system, paragraph);
    paragraph.clear();
    if graph.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut start = 0;
    while start < graph.runs.len() {
        while start < graph.runs.len()
            && inline_line_item_is_collapsible_space(&graph.runs[start].item)
        {
            start += 1;
        }
        if start >= graph.runs.len() {
            break;
        }
        let end = select_graph_text_line_end(
            &graph,
            start,
            lines.len(),
            font_system,
            style,
            available_width,
        )
        .max(start + 1)
        .min(graph.runs.len());
        let is_soft_break = end < graph.runs.len();
        if let Some(line) =
            materialize_graph_text_line(&graph, start..end, style, font_system, is_soft_break)
        {
            lines.push(line);
        }
        start = end;
    }
    lines
}

fn select_graph_text_line_end(
    graph: &inline_layout::InlineOpportunityGraph,
    start: usize,
    line_index: usize,
    font_system: &mut FontSystem,
    style: &ComputedStyle,
    available_width: f32,
) -> usize {
    let mut end = start;
    let mut line_width = 0.0_f32;
    let line_available_width = available_width.max(1.0);
    while end < graph.runs.len() {
        let run = &graph.runs[end];
        let line = graph.line_items(start..end);
        let first_hanging_punctuation_width = start_hanging_punctuation_width_for_candidate_line(
            font_system,
            &line,
            &run.item,
            style,
            line_index == 0,
        );
        let remaining_allows_last = graph.runs[end + 1..].iter().all(|run| {
            inline_line_item_is_collapsible_space(&run.item)
                || inline_line_item_is_pre_wrap_hanging_space(&run.item)
        });
        let final_hanging_punctuation_width = end_hanging_punctuation_width_for_candidate_line(
            font_system,
            &line,
            &run.item,
            style,
            remaining_allows_last,
            true,
        );
        let candidate_fits = inline_items_fit_line(
            line_width,
            run.width,
            line_available_width
                + first_hanging_punctuation_width
                + final_hanging_punctuation_width,
        );
        let final_preserved_space =
            end + 1 == graph.runs.len() && inline_line_item_is_pre_wrap_hanging_space(&run.item);
        if style.white_space.allows_soft_wrap()
            && end > start
            && !final_preserved_space
            && !candidate_fits
            && let Some(boundary) = best_graph_text_break_before(graph, start, end)
        {
            return boundary;
        }
        line_width += run.width;
        end += 1;
    }
    end
}

fn best_graph_text_break_before(
    graph: &inline_layout::InlineOpportunityGraph,
    start: usize,
    before_run: usize,
) -> Option<usize> {
    if let Some(boundary) = (start + 1..=before_run).rev().find(|boundary| {
        matches!(
            &graph.runs[*boundary].item,
            InlineLineItem::Fragment(fragment)
                if fragment.style.white_space == WhiteSpace::BreakSpaces
                    && fragment.text.chars().all(is_css_collapsible_whitespace)
        )
    }) {
        return Some(boundary);
    }
    (start + 1..=before_run)
        .rev()
        .find(|boundary| graph.break_opportunity_before(*boundary).is_some())
}

fn materialize_graph_text_line(
    graph: &inline_layout::InlineOpportunityGraph,
    range: std::ops::Range<usize>,
    style: &ComputedStyle,
    font_system: &mut FontSystem,
    is_soft_break: bool,
) -> Option<TextLine> {
    let mut items = graph.line_items(range.clone());
    let mut width = graph.line_width(range);
    width -= trim_trailing_inline_line_spaces(&mut items, font_system);
    if is_soft_break {
        width -= trim_trailing_pre_wrap_hanging_inline_line_spaces(&mut items, font_system);
        show_trailing_soft_hyphen_for_line(&mut items);
    }
    strip_zero_width_space_from_line_items(&mut items);
    width -= trailing_hanging_space_separator_width_for_line_items(&items, font_system);
    width -= trailing_letter_spacing_width_for_line_items(&items);
    let width = width.max(0.0);
    let text = graph_text_line_text(&items);
    if text.is_empty() && width <= 0.0 {
        return None;
    }
    let text = graph_text_line_visual_text(font_system, &text, style);
    let shaped = font_system.shape_unwrapped_line(&text, style, style.line_height);
    Some(TextLine::new(text, width, style.line_height).with_shaped(shaped))
}

fn graph_text_line_text(items: &[InlineLineItem]) -> String {
    items
        .iter()
        .filter_map(|item| match item {
            InlineLineItem::Fragment(fragment) => Some(fragment.text.as_str()),
            InlineLineItem::Atom(_) => None,
        })
        .collect()
}

fn graph_text_line_visual_text(
    font_system: &mut FontSystem,
    text: &str,
    style: &ComputedStyle,
) -> String {
    const ZERO_WIDTH_SPACE: char = '\u{200b}';
    let ranges = font_system.visual_ranges_for_unwrapped_text(text, style);
    if ranges.is_empty() {
        return text_without_bidi_format_controls(text).replace(ZERO_WIDTH_SPACE, "");
    }
    let mut output = String::new();
    for range in ranges {
        let Some(text) = char_boundary_slice(text, range) else {
            continue;
        };
        output.push_str(&text);
    }
    text_without_bidi_format_controls(&output).replace(ZERO_WIDTH_SPACE, "")
}

pub(super) fn last_hanging_punctuation_width_for_inline_items(
    font_system: &mut FontSystem,
    items: &[InlineItem],
    block_style: &ComputedStyle,
) -> f32 {
    if !block_style.hanging_punctuation.last {
        return 0.0;
    }
    let Some(word) = items.iter().rev().find_map(|item| match item {
        InlineItem::Word(word) if !trim_css_collapsible_whitespace(&word.text).is_empty() => {
            Some(word)
        }
        InlineItem::Word(_) | InlineItem::Atom(_) | InlineItem::Float(_) | InlineItem::Break => {
            None
        }
        InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => None,
    }) else {
        return 0.0;
    };
    last_hanging_punctuation_width(
        font_system,
        std::slice::from_ref(&InlineFragment {
            text: transform_text(&word.text, &word.style),
            style: word.style.clone(),
            baseline_shift: word.baseline_shift,
            link_target: word.link_target.clone(),
            mergeable: word.mergeable,
            hanging_edges: word.hanging_edges,
        }),
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
pub(super) fn line_box_uses_hanging_punctuation_alignment(style: &ComputedStyle) -> bool {
    !style.box_values.width.is_auto()
}

pub(super) fn anonymous_inline_content_needs_normalized_style(style: &ComputedStyle) -> bool {
    (style.display.is_block_level() && style.unicode_bidi == UnicodeBidi::Isolate)
        || (!style.display.is_inline_level() && style.vertical_align != VerticalAlign::Baseline)
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
pub(super) fn normalized_anonymous_inline_content_style(style: &ComputedStyle) -> ComputedStyle {
    let mut style = if style.display.is_block_level() && style.unicode_bidi == UnicodeBidi::Isolate
    {
        inline_content_style_without_block_isolate(style)
    } else {
        style.clone()
    };
    if !style.display.is_inline_level() {
        style.vertical_align = VerticalAlign::Baseline;
    }
    style
}

fn inline_content_style_without_block_isolate(style: &ComputedStyle) -> ComputedStyle {
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
fn inline_box_edge_breaks_shaping(style: &ComputedStyle) -> bool {
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
fn inline_box_bidi_isolation_breaks_shaping(style: &ComputedStyle) -> bool {
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
pub(super) fn inline_fragment_is_inter_word_justification_space(fragment: &InlineFragment) -> bool {
    fragment.text.chars().all(character_is_css_word_separator)
}

/// Return whether an inline fragment needs glyph or decoration paint.
///
/// CSS Color defines alpha as part of the used color. Fully transparent text
/// still participates in layout and can have backgrounds, but emits no visible
/// glyph paint unless an explicit visible text-decoration color is present:
/// <https://www.w3.org/TR/css-color-4/#alpha-value> and
/// <https://www.w3.org/TR/css-text-decor-4/#painting>.
pub(super) fn inline_fragment_has_visible_text_paint(fragment: &InlineFragment) -> bool {
    fragment.style.color.is_visible()
        || (fragment.style.text_decoration.has_visible_line()
            && fragment
                .style
                .text_decoration
                .color
                .unwrap_or(fragment.style.color)
                .is_visible())
}

pub(super) fn justifiable_fragment_space_count(fragments: &[InlineFragment]) -> usize {
    let mut end = fragments.len();
    while end > 0 && inline_fragment_is_pre_wrap_hanging_space(&fragments[end - 1]) {
        end -= 1;
    }
    fragments[..end]
        .iter()
        .filter(|fragment| inline_fragment_is_inter_word_justification_space(fragment))
        .map(|fragment| fragment.text.chars().count())
        .sum()
}

pub(super) fn justifiable_mixed_space_count(items: &[InlineLineItem]) -> usize {
    let mut end = items.len();
    while end > 0 && inline_line_item_is_pre_wrap_hanging_space(&items[end - 1]) {
        end -= 1;
    }
    items[..end]
        .iter()
        .filter_map(|item| match item {
            InlineLineItem::Fragment(fragment)
                if inline_fragment_is_inter_word_justification_space(fragment) =>
            {
                Some(fragment.text.chars().count())
            }
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) => None,
        })
        .sum()
}

pub(super) fn inter_character_fragment_gap_count(fragments: &[InlineFragment]) -> usize {
    fragments
        .iter()
        .map(|fragment| typographic_unit_count(&fragment.text))
        .sum::<usize>()
        .saturating_sub(1)
}

pub(super) fn inter_character_mixed_gap_count(items: &[InlineLineItem]) -> usize {
    let mut units = 0usize;
    let mut in_atom_run = false;
    for item in items {
        match item {
            InlineLineItem::Fragment(fragment) => {
                in_atom_run = false;
                units += typographic_unit_count(&fragment.text);
            }
            InlineLineItem::Atom(_) if !in_atom_run => {
                in_atom_run = true;
                units += 1;
            }
            InlineLineItem::Atom(_) => {}
        }
    }
    units.saturating_sub(1)
}

pub(super) fn split_fragments_into_inter_character_units(
    fragments: &[InlineFragment],
) -> Vec<InlineFragment> {
    fragments
        .iter()
        .flat_map(split_fragment_into_inter_character_units)
        .collect()
}

pub(super) fn split_mixed_line_into_inter_character_units(
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
        })
        .collect()
}

pub(super) fn inter_character_gap_after_mixed_item(items: &[InlineLineItem], index: usize) -> bool {
    let Some(item) = items.get(index) else {
        return false;
    };
    if matches!(item, InlineLineItem::Atom(_))
        && items
            .get(index + 1)
            .is_some_and(|next| matches!(next, InlineLineItem::Atom(_)))
    {
        return false;
    }
    items[index + 1..].iter().any(|item| match item {
        InlineLineItem::Fragment(fragment) => typographic_unit_count(&fragment.text) > 0,
        InlineLineItem::Atom(_) => true,
    })
}

fn split_fragment_into_inter_character_units(fragment: &InlineFragment) -> Vec<InlineFragment> {
    let boundaries = GraphemeClusterSegmenter::new()
        .segment_str(&fragment.text)
        .collect::<Vec<_>>();
    if boundaries.len() <= 2 {
        return vec![fragment.clone()];
    }
    boundaries
        .windows(2)
        .filter_map(|window| {
            let text = &fragment.text[window[0]..window[1]];
            (!text.is_empty()).then(|| InlineFragment {
                text: text.to_string(),
                style: fragment.style.clone(),
                baseline_shift: fragment.baseline_shift,
                link_target: fragment.link_target.clone(),
                mergeable: false,
                hanging_edges: fragment.hanging_edges,
            })
        })
        .collect()
}

fn typographic_unit_count(text: &str) -> usize {
    GraphemeClusterSegmenter::new()
        .segment_str(text)
        .collect::<Vec<_>>()
        .len()
        .saturating_sub(1)
}

pub(super) fn trim_inline_fragment_edges(fragments: &mut Vec<InlineFragment>) {
    while fragments
        .first()
        .is_some_and(inline_fragment_is_collapsible_space)
    {
        fragments.remove(0);
    }
    while fragments
        .last()
        .is_some_and(inline_fragment_is_collapsible_space)
    {
        fragments.pop();
    }
    if let Some(first) = fragments.first_mut()
        && first.mergeable
        && first.style.white_space.collapses_spaces()
    {
        first.text = trim_start_css_collapsible_whitespace(&first.text).to_string();
    }
    if let Some(last) = fragments.last_mut()
        && last.mergeable
        && last.style.white_space.collapses_spaces()
    {
        last.text = trim_end_css_collapsible_whitespace(&last.text).to_string();
    }
}

pub(super) fn apply_first_line_pseudos_to_fragments(
    fragments: &mut Vec<InlineFragment>,
    block_style: &ComputedStyle,
) {
    if let Some(first_line_style) = block_style.first_line_style.as_deref() {
        for fragment in fragments.iter_mut() {
            fragment.style = first_line_style.clone();
        }
    }
    if let Some(first_letter_style) = block_style.first_letter_style.as_deref() {
        apply_first_letter_pseudo_to_fragments(fragments, first_letter_style);
    }
}

pub(super) fn apply_first_line_pseudos_to_line_items(
    items: &mut Vec<InlineLineItem>,
    block_style: &ComputedStyle,
) {
    if let Some(first_line_style) = block_style.first_line_style.as_deref() {
        for item in items.iter_mut() {
            if let InlineLineItem::Fragment(fragment) = item {
                fragment.style = first_line_style.clone();
            }
        }
    }
    if let Some(first_letter_style) = block_style.first_letter_style.as_deref() {
        apply_first_letter_pseudo_to_line_items(items, first_letter_style);
    }
}

fn apply_first_letter_pseudo_to_line_items(
    items: &mut Vec<InlineLineItem>,
    first_letter_style: &ComputedStyle,
) {
    for index in 0..items.len() {
        let InlineLineItem::Fragment(fragment) = &items[index] else {
            continue;
        };
        let Some(range) = first_letter_byte_range(&fragment.text) else {
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

fn apply_first_letter_pseudo_to_fragments(
    fragments: &mut Vec<InlineFragment>,
    first_letter_style: &ComputedStyle,
) {
    for index in 0..fragments.len() {
        let Some(range) = first_letter_byte_range(&fragments[index].text) else {
            continue;
        };
        let pieces = split_fragment_for_first_letter(&fragments[index], range, first_letter_style);
        fragments.splice(index..=index, pieces);
        break;
    }
}

fn split_fragment_for_first_letter(
    fragment: &InlineFragment,
    range: std::ops::Range<usize>,
    first_letter_style: &ComputedStyle,
) -> Vec<InlineFragment> {
    let mut pieces = Vec::new();
    if range.start > 0 {
        let mut before = fragment.clone();
        before.text = fragment.text[..range.start].to_string();
        pieces.push(before);
    }
    let mut letter = fragment.clone();
    letter.text = fragment.text[range.clone()].to_string();
    letter.style = first_letter_style.clone();
    letter.mergeable = false;
    pieces.push(letter);
    if range.end < fragment.text.len() {
        let mut after = fragment.clone();
        after.text = fragment.text[range.end..].to_string();
        pieces.push(after);
    }
    pieces
}

fn first_letter_byte_range(text: &str) -> Option<std::ops::Range<usize>> {
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
use icu_segmenter::GraphemeClusterSegmenter;
