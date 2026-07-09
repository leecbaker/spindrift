use super::*;
use crate::text::manual_suppresses_break_between;
use std::rc::Rc;

/// Reusable temporary storage for one inline opportunity-graph build.
///
/// Neighboring [`InlineFragment`] values own separate `Rc<str>` buffers, but
/// ICU line breaking requires a contiguous string to inspect their boundary.
/// Keep that necessary joined buffer and the transient break vectors alive for
/// the complete graph build instead of reallocating them at every boundary.
#[derive(Default)]
struct InlineBreakScratch {
    joined_text: String,
    break_positions: Vec<usize>,
    grapheme_positions: Vec<usize>,
}

impl InlineBreakScratch {
    fn join(&mut self, before: &str, after: &str) {
        self.joined_text.clear();
        self.joined_text.push_str(before);
        self.joined_text.push_str(after);
    }
}

/// One side of a graph boundary evaluated by CSS Text line breaking.
///
/// Text and atomic inline objects have different CSS ownership even though
/// UAX #14 represents an atomic object internally as U+FFFC. Keeping that
/// representation private to the resolver prevents the graph from treating a
/// replacement object as authored text while sharing the same policy for text,
/// transparent inline edges, and atomic boundaries:
/// <https://www.w3.org/TR/css-text-3/#line-break-details> and
/// <https://www.unicode.org/reports/tr14/#LB30b>.
#[derive(Clone, Copy)]
enum InlineBreakBoundaryContext<'a> {
    Text { text: &'a str },
    Atomic,
}

impl InlineBreakBoundaryContext<'_> {
    fn leading_character(self) -> Option<char> {
        match self {
            Self::Text { text } => text.chars().next(),
            Self::Atomic => None,
        }
    }

    fn trailing_character(self) -> Option<char> {
        match self {
            Self::Text { text } => text.chars().next_back(),
            Self::Atomic => None,
        }
    }

    fn append_uax14_input(self, output: &mut String) {
        match self {
            Self::Text { text } => output.push_str(text),
            Self::Atomic => output.push(OBJECT_REPLACEMENT_CHARACTER),
        }
    }

    fn is_atomic(self) -> bool {
        matches!(self, Self::Atomic)
    }
}

pub(in crate::layout) fn normalize_materialized_fragment_text(
    text: &str,
    visible_trailing_soft_hyphen: bool,
    preserve_joining_context: bool,
    hyphenate_character: &str,
) -> Option<String> {
    const SOFT_HYPHEN: char = '\u{00ad}';
    const ZERO_WIDTH_SPACE: char = '\u{200b}';
    let has_zero_width_space = text.contains(ZERO_WIDTH_SPACE);
    let has_soft_hyphen = text.contains(SOFT_HYPHEN);
    if !has_zero_width_space && !has_soft_hyphen {
        return None;
    }
    let mut normalized = if has_zero_width_space {
        text.replace(ZERO_WIDTH_SPACE, "")
    } else {
        text.to_string()
    };
    if !has_soft_hyphen {
        return Some(normalized);
    }
    if visible_trailing_soft_hyphen && normalized.ends_with(SOFT_HYPHEN) {
        normalized.pop();
        normalized = normalized.replace(SOFT_HYPHEN, "");
        // The matching leading ZWJ is added to the following line by graph
        // materialization. Together these controls retain the source word's
        // joining context while the used hyphenate character is inserted at
        // the selected line edge.
        if preserve_joining_context {
            normalized.push('\u{200d}');
        }
        normalized.push_str(hyphenate_character);
        Some(normalized)
    } else {
        Some(normalized.replace(SOFT_HYPHEN, ""))
    }
}

pub(in crate::layout) fn remeasure_materialized_item(
    item: &mut MeasuredInlineItem,
    font_system: &mut FontSystem,
) {
    let InlineLineItem::Fragment(fragment) = &item.item else {
        return;
    };
    item.shaped = font_system
        .shape_unwrapped_line(
            fragment.text(),
            fragment.style(),
            fragment.style().line_height,
        )
        .map(Rc::new);
    item.width = item
        .shaped
        .as_deref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
}

pub(in crate::layout) fn text_for_measured_items(items: &[MeasuredInlineItem]) -> String {
    items
        .iter()
        .filter_map(|item| match &item.item {
            InlineLineItem::Fragment(fragment) => Some(fragment.text()),
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
        })
        .collect()
}

pub(in crate::layout) fn inline_break_opportunities_for_runs(
    runs: &[InlineParagraphRun],
    boundary_style: &ComputedStyle,
) -> Vec<InlineBreakOpportunity> {
    let mut opportunities = Vec::new();
    let mut scratch = InlineBreakScratch::default();
    for (run_index, run) in runs.iter().enumerate() {
        append_inline_break_opportunities_inside_run(
            run_index,
            run,
            &mut opportunities,
            &mut scratch,
        );
    }
    append_inline_break_opportunities_across_transparent_edges(
        runs,
        &mut opportunities,
        &mut scratch,
    );
    for boundary in 1..runs.len() {
        if let Some(opportunity) =
            inline_break_opportunity_at_boundary(boundary, runs, boundary_style, &mut scratch)
        {
            opportunities.push(opportunity);
        }
    }
    if !runs.is_empty() {
        opportunities.push(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(runs.len()),
            kind: InlineBreakKind::Forced,
            priority: u8::MAX,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: false,
        });
    }
    opportunities.sort_by_key(|opportunity| (opportunity.position, opportunity.priority));
    opportunities.dedup_by(|left, right| {
        left.position == right.position
            && left.kind == right.kind
            && left.emergency == right.emergency
            && left.min_content == right.min_content
    });
    opportunities
}

fn append_inline_break_opportunities_inside_run(
    run_index: usize,
    run: &InlineParagraphRun,
    output: &mut Vec<InlineBreakOpportunity>,
    scratch: &mut InlineBreakScratch,
) {
    let InlineLineItem::Fragment(fragment) = &run.item else {
        return;
    };
    if !fragment.style().allows_soft_wrap() {
        return;
    }
    let text = fragment.text();
    // Keep Unicode/`word-break`/`line-break` opportunities distinct from
    // overflow wrapping. `overflow-wrap:anywhere` creates emergency breaks
    // which affect min-content, whereas `break-word` creates the same fitting
    // fallback without changing min-content. Treating either as an ordinary
    // UAX #14 wrap lets it incorrectly compete with normal opportunities.
    // <https://drafts.csswg.org/css-text-3/#overflow-wrap-property>
    let line_break_policy = TextBreakPolicy::from(fragment.style()).without_overflow_wrap();
    collect_measured_break_opportunities(text, line_break_policy, &mut scratch.break_positions);
    scratch.break_positions.retain(|position| {
        *position > 0
            && *position < text.len()
            && text.is_char_boundary(*position)
            && !text[*position..].starts_with('\u{200b}')
    });
    for &position in &scratch.break_positions {
        output.push(inline_text_break_opportunity(
            run_index,
            text,
            fragment.style(),
            position,
            false,
            text_break_is_min_content_eligible(text, fragment.style(), position),
        ));
    }
    if let Some(overflow_wrap) = effective_overflow_wrap(fragment.style()) {
        let affects_min_content = matches!(overflow_wrap, css::OverflowWrap::Anywhere);
        collect_grapheme_cluster_inner_boundaries(text, &mut scratch.grapheme_positions);
        for &position in &scratch.grapheme_positions {
            if position == 0
                || position >= text.len()
                || scratch.break_positions.binary_search(&position).is_ok()
            {
                continue;
            }
            output.push(inline_text_break_opportunity(
                run_index,
                text,
                fragment.style(),
                position,
                true,
                affects_min_content,
            ));
        }
    } else if keep_all_allows_last_resort_breaking(fragment.style()) {
        // `keep-all` suppresses ordinary CJK opportunities, but CSS Text
        // permits the UA to relax that restriction if no otherwise-acceptable
        // break can fit. Model those as graph emergency opportunities so they
        // remain out of min-content sizing and lose to every ordinary wrap.
        // <https://www.w3.org/TR/css-text-3/#word-break-property>
        collect_grapheme_cluster_inner_boundaries(text, &mut scratch.grapheme_positions);
        for &position in &scratch.grapheme_positions {
            if position == 0
                || position >= text.len()
                || scratch.break_positions.binary_search(&position).is_ok()
            {
                continue;
            }
            output.push(inline_text_break_opportunity(
                run_index,
                text,
                fragment.style(),
                position,
                true,
                false,
            ));
        }
    }
}

pub(in crate::layout) fn inline_text_break_opportunity(
    run_index: usize,
    text: &str,
    style: &ComputedStyle,
    byte_offset: usize,
    emergency: bool,
    min_content: bool,
) -> InlineBreakOpportunity {
    let soft_hyphen = text[..byte_offset].ends_with('\u{00ad}');
    let hangs = style.white_space == WhiteSpace::PreWrap
        && text[..byte_offset]
            .chars()
            .next_back()
            .is_some_and(is_css_preserved_document_space);
    InlineBreakOpportunity {
        position: InlineGraphPosition {
            run_index,
            byte_offset,
        },
        kind: if soft_hyphen {
            InlineBreakKind::Hyphenation
        } else if emergency {
            InlineBreakKind::Emergency
        } else {
            InlineBreakKind::SoftWrap
        },
        priority: if soft_hyphen {
            200
        } else if emergency {
            230
        } else {
            100
        },
        trims: false,
        hangs,
        soft_hyphen,
        emergency,
        min_content,
    }
}

fn inline_break_opportunity_at_boundary(
    boundary: usize,
    runs: &[InlineParagraphRun],
    boundary_style: &ComputedStyle,
    scratch: &mut InlineBreakScratch,
) -> Option<InlineBreakOpportunity> {
    let previous = &runs[boundary - 1].item;
    let next = &runs[boundary].item;
    // A float's source marker is not in-flow text, but it is a placement
    // boundary: the preceding inline material may form a line before the
    // float is positioned and the following material is then selected against
    // that float's exclusion. This remains available inside `white-space:
    // nowrap`, where it must not be mistaken for a CSS Text soft-wrap.
    // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
    if matches!(next, InlineLineItem::Float(_)) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::AtomicBoundary,
            priority: 110,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }
    if inline_line_item_is_collapsible_space(next) && inline_line_item_allows_soft_wrap(next) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            priority: 220,
            trims: true,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }
    if inline_line_item_ends_with_collapsible_space(previous)
        && inline_line_item_is_css_atomic(next)
        && inline_line_item_allows_soft_wrap(previous)
    {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            priority: 220,
            trims: true,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }
    if matches!(
        previous,
        InlineLineItem::Fragment(fragment)
            if inline_fragment_is_pre_wrap_hanging_space(fragment)
    ) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            priority: 220,
            trims: false,
            hangs: true,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }
    // `break-spaces` creates a soft-wrap opportunity after each preserved
    // document-space character.  Test this side first: a boundary between two
    // preserved spaces is both after the former and before the latter, but
    // must retain the former's after-space opportunity.
    // <https://drafts.csswg.org/css-text-3/#valdef-white-space-break-spaces>
    if inline_line_item_is_break_spaces_preserved_space(previous) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::BreakSpaces,
            priority: 210,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }

    // A normal UAX #14 opportunity before the first preserved space would
    // transfer that space to the following line. Suppress it, except where a
    // selected policy (such as `line-break:anywhere`) itself makes this an
    // ordinary opportunity; that policy must remain observable.
    let break_spaces_next_is_already_ordinary_opportunity = matches!(
        (previous, next),
        (InlineLineItem::Fragment(previous), InlineLineItem::Fragment(next))
            if inline_fragment_boundary_allows_soft_wrap(previous, next, scratch)
    );
    if inline_line_item_is_break_spaces_preserved_space(next)
        && !break_spaces_next_is_already_ordinary_opportunity
        && matches!(
            next,
            InlineLineItem::Fragment(fragment)
                if effective_overflow_wrap(fragment.style()).is_some()
        )
    {
        // `break-spaces` supplies after-space opportunities. When such a
        // preserved run itself would overflow, `overflow-wrap:anywhere` also
        // permits the emergency boundary before its first character. Keep it
        // emergency-only when no ordinary boundary occupies that same edge,
        // so an earlier ordinary break remains preferred.
        // <https://drafts.csswg.org/css-text-3/#valdef-white-space-break-spaces>
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::Emergency,
            priority: 230,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: true,
            min_content: matches!(
                next,
                InlineLineItem::Fragment(fragment)
                    if matches!(
                        effective_overflow_wrap(fragment.style()),
                        Some(css::OverflowWrap::Anywhere)
                    )
            ),
        });
    }
    if inline_line_item_is_break_spaces_preserved_space(next)
        && !break_spaces_next_is_already_ordinary_opportunity
    {
        return None;
    }
    if matches!(
        previous,
        InlineLineItem::Fragment(fragment) if fragment.text().ends_with('\u{00ad}')
    ) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::Hyphenation,
            priority: 200,
            trims: false,
            hangs: false,
            soft_hyphen: true,
            emergency: false,
            min_content: true,
        });
    }
    if inline_line_item_is_css_atomic(previous) || inline_line_item_is_css_atomic(next) {
        return inline_atomic_boundary_opportunity(boundary, runs, boundary_style, scratch);
    }
    if let (InlineLineItem::Fragment(previous), InlineLineItem::Fragment(next)) = (previous, next) {
        return inline_fragment_boundary_opportunity(boundary, previous, next, scratch);
    }
    None
}

pub(in crate::layout) fn inline_line_item_is_css_atomic(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Atom(atom)
            if !matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(_)
                    | InlineAtomContent::Leader(_)
                    | InlineAtomContent::StaticPositionPlaceholder
            )
    )
}

/// Return whether a line item can supply an ordinary soft-wrap opportunity.
///
/// `white-space` applies to the inline that owns the text, rather than to its
/// block container. In particular, a collapsible separator inside `nowrap`
/// must not become a graph boundary merely because its parent line is
/// otherwise wrappable.
/// <https://www.w3.org/TR/css-text-3/#white-space-property>
pub(in crate::layout) fn inline_line_item_allows_soft_wrap(item: &InlineLineItem) -> bool {
    match item {
        InlineLineItem::Fragment(fragment) => fragment.style().allows_soft_wrap(),
        InlineLineItem::Atom(atom) => atom.style().allows_soft_wrap(),
        InlineLineItem::Float(_) => false,
    }
}

/// Return whether an item is a preserved `break-spaces` document-space run.
///
/// CSS Text gives each such character an after-character soft wrap, and lets
/// overflow wrapping use the preceding item boundary when needed.
/// <https://www.w3.org/TR/css-text-3/#valdef-white-space-break-spaces>
pub(in crate::layout) fn inline_line_item_is_break_spaces_preserved_space(
    item: &InlineLineItem,
) -> bool {
    matches!(
        item,
        InlineLineItem::Fragment(fragment)
            if fragment.style().white_space == WhiteSpace::BreakSpaces
                && fragment.text().chars().all(is_css_preserved_document_space)
    )
}

pub(in crate::layout) fn inline_line_item_ends_with_collapsible_space(
    item: &InlineLineItem,
) -> bool {
    matches!(
        item,
        InlineLineItem::Fragment(fragment)
            if fragment.style().white_space.collapses_spaces()
                && fragment.text().ends_with(is_css_collapsible_whitespace)
    )
}

fn inline_atomic_boundary_opportunity(
    boundary: usize,
    runs: &[InlineParagraphRun],
    boundary_style: &ComputedStyle,
    scratch: &mut InlineBreakScratch,
) -> Option<InlineBreakOpportunity> {
    let before = inline_break_context_before_boundary(runs, boundary)?;
    let after = inline_break_context_after_boundary(runs, boundary)?;
    // CSS Text assigns an opportunity at an atomic-inline boundary to the
    // nearest common ancestor of the adjacent inline-level participants. The
    // graph's paragraph context is that ancestor for atomic participants;
    // their descendant styles still control their own internal text only.
    // <https://drafts.csswg.org/css-text-3/#line-break-details>
    (inline_break_boundary_allows_soft_wrap(
        before,
        after,
        [boundary_style, boundary_style],
        scratch,
    ) || inline_atomic_boundary_has_nbsp_opportunity(before, after))
    .then_some(InlineBreakOpportunity {
        position: InlineGraphPosition::at_run_start(boundary),
        kind: InlineBreakKind::AtomicBoundary,
        priority: 120,
        trims: false,
        hangs: false,
        soft_hyphen: false,
        emergency: false,
        min_content: true,
    })
}

fn inline_break_context_before_boundary(
    runs: &[InlineParagraphRun],
    boundary: usize,
) -> Option<InlineBreakBoundaryContext<'_>> {
    for run in runs[..boundary].iter().rev() {
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                return Some(InlineBreakBoundaryContext::Text {
                    text: fragment.text(),
                });
            }
            InlineLineItem::Atom(atom) if inline_line_item_is_transparent_text_edge(atom) => {}
            InlineLineItem::Atom(atom) if inline_line_item_is_css_atomic(&run.item) => {
                return Some(InlineBreakBoundaryContext::Atomic);
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    None
}

fn inline_break_context_after_boundary(
    runs: &[InlineParagraphRun],
    boundary: usize,
) -> Option<InlineBreakBoundaryContext<'_>> {
    for run in &runs[boundary..] {
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                return Some(InlineBreakBoundaryContext::Text {
                    text: fragment.text(),
                });
            }
            InlineLineItem::Atom(atom) if inline_line_item_is_transparent_text_edge(atom) => {}
            InlineLineItem::Atom(atom) if inline_line_item_is_css_atomic(&run.item) => {
                return Some(InlineBreakBoundaryContext::Atomic);
            }
            InlineLineItem::Float(_) | InlineLineItem::Atom(_) => return None,
        }
    }
    None
}

fn append_inline_break_opportunities_across_transparent_edges(
    runs: &[InlineParagraphRun],
    opportunities: &mut Vec<InlineBreakOpportunity>,
    scratch: &mut InlineBreakScratch,
) {
    for edge_start in 1..runs.len() {
        if !inline_line_item_is_transparent_text_edge_item(&runs[edge_start].item)
            || inline_line_item_is_transparent_text_edge_item(&runs[edge_start - 1].item)
        {
            continue;
        }
        let edge_end = (edge_start + 1..runs.len())
            .find(|index| !inline_line_item_is_transparent_text_edge_item(&runs[*index].item))
            .unwrap_or(runs.len());
        let Some(previous) = previous_text_fragment_before(runs, edge_start) else {
            continue;
        };
        let Some(next) = runs.get(edge_end).and_then(|run| match &run.item {
            InlineLineItem::Fragment(fragment) => Some(fragment),
            _ => None,
        }) else {
            continue;
        };
        if let Some(opportunity) =
            inline_fragment_boundary_opportunity(edge_start, previous, next, scratch)
        {
            opportunities.push(opportunity);
        }
    }
}

pub(in crate::layout) fn previous_text_fragment_before(
    runs: &[InlineParagraphRun],
    before_run: usize,
) -> Option<&InlineFragment> {
    for run in runs[..before_run].iter().rev() {
        match &run.item {
            InlineLineItem::Fragment(fragment) => return Some(fragment),
            InlineLineItem::Atom(atom) if inline_line_item_is_transparent_text_edge(atom) => {}
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    None
}

pub(in crate::layout) fn inline_line_item_is_transparent_text_edge_item(
    item: &InlineLineItem,
) -> bool {
    matches!(item, InlineLineItem::Atom(atom) if inline_line_item_is_transparent_text_edge(atom))
}

/// A text-autospace edge contributes its advance but does not replace the
/// UAX #14/CSS Text boundary between its neighboring text fragments.
fn inline_line_item_is_transparent_text_edge(atom: &InlineAtom) -> bool {
    matches!(
        atom.content(),
        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_) | InlineEdgeRole::TextAutospace)
            | InlineAtomContent::StaticPositionPlaceholder
    )
}

fn inline_fragment_boundary_allows_soft_wrap(
    previous: &InlineFragment,
    next: &InlineFragment,
    scratch: &mut InlineBreakScratch,
) -> bool {
    inline_text_boundary_allows_soft_wrap(
        previous.text(),
        next.text(),
        [previous.style(), next.style()],
        scratch,
    )
}

/// Return whether either owning inline style permits an ordinary UAX #14
/// break between two text slices.
///
/// CSS line breaking needs a contiguous sequence at an inline boundary, so
/// the caller supplies reusable joined-text and position storage. Identical
/// relevant policies are evaluated only once.
fn inline_text_boundary_allows_soft_wrap(
    before: &str,
    after: &str,
    styles: [&ComputedStyle; 2],
    scratch: &mut InlineBreakScratch,
) -> bool {
    if before.is_empty() || after.is_empty() {
        return false;
    }
    scratch.join(before, after);
    inline_uax14_boundary_allows_soft_wrap(before.len(), styles, scratch)
}

/// Resolve UAX #14 at a typed graph boundary.
///
/// The object replacement character is appended only to the transient UAX #14
/// input required for an atomic inline object. It never becomes a source text
/// slice, extraction string, or cross-run boundary sentinel:
/// <https://www.w3.org/TR/css-text-3/#line-break-details>.
fn inline_break_boundary_allows_soft_wrap(
    before: InlineBreakBoundaryContext<'_>,
    after: InlineBreakBoundaryContext<'_>,
    styles: [&ComputedStyle; 2],
    scratch: &mut InlineBreakScratch,
) -> bool {
    if matches!(before, InlineBreakBoundaryContext::Text { text } if text.is_empty())
        || matches!(after, InlineBreakBoundaryContext::Text { text } if text.is_empty())
    {
        return false;
    }
    // UAX #14 LB11 prohibits a break before or after WORD JOINER. ICU's
    // transient U+FFFC representation for an atomic inline must not erase
    // that authored control at an object/text boundary.
    // <https://www.unicode.org/reports/tr14/#LB11>
    if before.trailing_character() == Some('\u{2060}')
        || after.leading_character() == Some('\u{2060}')
    {
        return false;
    }
    scratch.joined_text.clear();
    before.append_uax14_input(&mut scratch.joined_text);
    let boundary = scratch.joined_text.len();
    after.append_uax14_input(&mut scratch.joined_text);
    inline_uax14_boundary_allows_soft_wrap(boundary, styles, scratch)
}

fn inline_uax14_boundary_allows_soft_wrap(
    boundary: usize,
    styles: [&ComputedStyle; 2],
    scratch: &mut InlineBreakScratch,
) -> bool {
    let mut previous_policy = None;
    for (style_index, style) in styles.into_iter().enumerate() {
        if !style.allows_soft_wrap() {
            continue;
        }
        if matches!(style.word_break, css::WordBreak::BreakAll)
            && style_index == 0
            && styles[0].word_break != styles[1].word_break
        {
            // `break-all` adds letter opportunities within the element that
            // owns it. At an inline-style boundary, the following text owns
            // the boundary's first typographic unit: its `break-all` can
            // break before that unit, but a preceding `break-all` must not
            // extend an opportunity into an adjacent normal run.
            // <https://drafts.csswg.org/css-text-3/#word-break-property>
            continue;
        }
        let policy = TextBreakPolicy::from(style).without_overflow_wrap();
        if previous_policy == Some(policy) {
            continue;
        }
        collect_measured_break_opportunities(
            &scratch.joined_text,
            policy,
            &mut scratch.break_positions,
        );
        if scratch.break_positions.binary_search(&boundary).is_ok() {
            return true;
        }
        previous_policy = Some(policy);
    }
    false
}

/// CSS Text's atomic-inline tailoring retains the NBSP exception around an
/// atomic replacement object after normal Unicode line breaking.
fn inline_atomic_boundary_has_nbsp_opportunity(
    before: InlineBreakBoundaryContext<'_>,
    after: InlineBreakBoundaryContext<'_>,
) -> bool {
    (before.trailing_character() == Some('\u{00a0}') && after.is_atomic())
        || (before.is_atomic() && after.leading_character() == Some('\u{00a0}'))
}

fn inline_fragment_boundary_opportunity(
    boundary: usize,
    previous: &InlineFragment,
    next: &InlineFragment,
    scratch: &mut InlineBreakScratch,
) -> Option<InlineBreakOpportunity> {
    let ordinary_wrap = inline_fragment_boundary_allows_soft_wrap(previous, next, scratch);
    let tracking_wrap = inline_fragment_boundary_has_tracking_opportunity(previous, next);
    let overflow_wrap = inline_fragment_boundary_overflow_wrap(previous, next);
    if !ordinary_wrap && !tracking_wrap && overflow_wrap.is_none() {
        return None;
    }
    let emergency = !ordinary_wrap && !tracking_wrap && overflow_wrap.is_some();
    let min_content = if emergency {
        matches!(overflow_wrap, Some(css::OverflowWrap::Anywhere))
    } else {
        inline_fragment_boundary_is_min_content_eligible(previous, next)
    };
    Some(InlineBreakOpportunity {
        position: InlineGraphPosition::at_run_start(boundary),
        kind: if emergency {
            InlineBreakKind::Emergency
        } else {
            InlineBreakKind::SoftWrap
        },
        priority: if emergency { 230 } else { 100 },
        trims: false,
        hangs: false,
        soft_hyphen: false,
        emergency,
        min_content,
    })
}

/// Return the overflow-wrap fallback available at a text-run boundary.
///
/// An ordinary UAX #14 opportunity is represented separately, because CSS
/// Text uses overflow wrapping only when no ordinary break can fit. If the
/// participating styles differ, `anywhere` wins because it also changes
/// min-content sizing; `break-word` is otherwise the fallback.
/// <https://drafts.csswg.org/css-text-3/#overflow-wrap-property>
fn inline_fragment_boundary_overflow_wrap(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> Option<css::OverflowWrap> {
    [previous.style(), next.style()]
        .into_iter()
        .filter(|style| style.allows_soft_wrap())
        .filter_map(effective_overflow_wrap)
        .max_by_key(|overflow_wrap| match overflow_wrap {
            css::OverflowWrap::Anywhere => 2,
            css::OverflowWrap::BreakWord => 1,
            css::OverflowWrap::Normal => 0,
        })
}

/// Return the emergency wrapping behavior after resolving legacy
/// `word-break: break-word`.
///
/// CSS Text defines that value as `overflow-wrap:anywhere` behavior even when
/// the authored `overflow-wrap` value is different. Keeping the resolution at
/// graph opportunity creation makes fitting and min-content consume the same
/// typed CSS fact.
/// <https://drafts.csswg.org/css-text-3/#valdef-word-break-break-word>
fn effective_overflow_wrap(style: &ComputedStyle) -> Option<css::OverflowWrap> {
    if matches!(style.word_break, css::WordBreak::BreakWord) {
        Some(css::OverflowWrap::Anywhere)
    } else {
        match style.overflow_wrap {
            css::OverflowWrap::Anywhere => Some(css::OverflowWrap::Anywhere),
            css::OverflowWrap::BreakWord => Some(css::OverflowWrap::BreakWord),
            css::OverflowWrap::Normal => None,
        }
    }
}

/// Return whether `keep-all` may expose last-resort breaks after ordinary
/// opportunity selection fails.
///
/// This is neither `overflow-wrap:anywhere` nor `break-word`: it is a
/// fitting-only relaxation of `keep-all` and therefore never contributes to
/// min-content sizing.
/// <https://www.w3.org/TR/css-text-3/#word-break-property>
fn keep_all_allows_last_resort_breaking(style: &ComputedStyle) -> bool {
    matches!(style.word_break, css::WordBreak::KeepAll)
        && matches!(style.overflow_wrap, css::OverflowWrap::Normal)
}

pub(in crate::layout) fn inline_fragment_boundary_has_tracking_opportunity(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    !previous.text().is_empty()
        && !next.text().is_empty()
        && (previous.style().used_letter_spacing() != layout_pt(0.0)
            || next.style().used_letter_spacing() != layout_pt(0.0))
}

pub(in crate::layout) fn inline_fragment_boundary_is_min_content_eligible(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    let previous_character = previous.text().chars().next_back();
    let next_character = next.text().chars().next();
    [previous.style(), next.style()].into_iter().any(|style| {
        if !style.allows_soft_wrap() {
            return false;
        }
        text_break_is_min_content_eligible_at_fragment_boundary(
            previous_character,
            next_character,
            style,
        )
    })
}

/// Return the min-content contribution of an already-established fragment
/// boundary without materializing a joined string.
///
/// The same adjacent-character policy as intra-fragment measurement applies
/// at a transparent inline boundary, including CSS Text 4 `manual`.
fn text_break_is_min_content_eligible_at_fragment_boundary(
    previous: Option<char>,
    next: Option<char>,
    style: &ComputedStyle,
) -> bool {
    let (Some(previous), Some(next)) = (previous, next) else {
        return false;
    };
    match style.word_break {
        css::WordBreak::KeepAll => !keep_all_suppresses_break_between(previous, next),
        css::WordBreak::Manual => !manual_suppresses_break_between(previous, next),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_soft_hyphen_preserves_trailing_joining_context() {
        assert_eq!(
            normalize_materialized_fragment_text("قىل\u{00ad}", true, true, "\u{a0}\u{640}"),
            Some("قىل\u{200d}\u{a0}\u{640}".to_string())
        );
    }

    fn fragment(text: &str, style: ComputedStyle) -> InlineFragment {
        InlineFragment::new(
            text,
            style,
            0.0,
            None,
            true,
            InlineTextSource::Normal,
            false,
            InlineHangingEdges::default(),
            Vec::new(),
        )
    }

    fn text_autospace_edge(style: &ComputedStyle) -> InlineAtom {
        InlineAtom::new(
            InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace),
            style.clone(),
            None,
            InlineSize::new(0.0, style.line_height),
            style.font_size,
            0.0,
            None,
            None,
        )
    }

    fn box_edge(style: &ComputedStyle) -> InlineAtom {
        InlineAtom::new(
            InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(InlineBoxEdgeFragment {
                logical_edge: InlineLogicalEdge::End,
                physical_side: PhysicalSide::Right,
                positioning_containing_block_id: None,
                advance: 0.0,
                paint_extent: 0.0,
            })),
            style.clone(),
            None,
            InlineSize::new(0.0, style.line_height),
            style.font_size,
            0.0,
            None,
            None,
        )
    }

    #[test]
    fn transparent_edges_preserve_cjk_boundary_opportunity() {
        let style = ComputedStyle::initial();
        for edge in [text_autospace_edge(&style), box_edge(&style)] {
            let runs = vec![
                InlineParagraphRun {
                    item: InlineLineItem::Fragment(fragment("中", style.clone())),
                    width: 0.0,
                    shaped: None,
                },
                InlineParagraphRun {
                    item: InlineLineItem::Atom(edge),
                    width: 0.0,
                    shaped: None,
                },
                InlineParagraphRun {
                    item: InlineLineItem::Fragment(fragment("文", style.clone())),
                    width: 0.0,
                    shaped: None,
                },
            ];

            let opportunities = inline_break_opportunities_for_runs(&runs, &style);
            assert!(opportunities.iter().any(|opportunity| {
                opportunity.position == InlineGraphPosition::at_run_start(1)
                    && matches!(opportunity.kind, InlineBreakKind::SoftWrap)
                    && opportunity.min_content
            }));
        }
    }

    #[test]
    fn fragment_boundary_min_content_keeps_keep_all_cjk_unbreakable() {
        let normal = ComputedStyle::initial();
        let mut keep_all = normal.clone();
        keep_all.word_break = css::WordBreak::KeepAll;

        assert!(inline_fragment_boundary_is_min_content_eligible(
            &fragment("中", normal.clone()),
            &fragment("文", normal),
        ));
        assert!(!inline_fragment_boundary_is_min_content_eligible(
            &fragment("中", keep_all.clone()),
            &fragment("文", keep_all),
        ));
    }

    #[test]
    fn break_all_has_directional_inline_style_boundary_ownership() {
        let normal = ComputedStyle::initial();
        let mut break_all = normal.clone();
        break_all.word_break = css::WordBreak::BreakAll;
        let mut scratch = InlineBreakScratch::default();

        assert!(!inline_fragment_boundary_allows_soft_wrap(
            &fragment("b", break_all.clone()),
            &fragment("c", normal),
            &mut scratch,
        ));
        assert!(inline_fragment_boundary_allows_soft_wrap(
            &fragment("b", ComputedStyle::initial()),
            &fragment("c", break_all),
            &mut scratch,
        ));
    }

    #[test]
    fn atomic_boundary_uses_its_common_inline_ancestor_style() {
        let mut ancestor = ComputedStyle::initial();
        ancestor.white_space = WhiteSpace::Normal;
        let mut descendant = ancestor.clone();
        descendant.white_space = WhiteSpace::Pre;
        let atom = InlineLineItem::Atom(InlineAtom::new(
            InlineAtomContent::Canvas,
            descendant.clone(),
            None,
            InlineSize::new(1.0, descendant.line_height),
            descendant.font_size,
            0.0,
            None,
            None,
        ));
        let runs = vec![
            InlineParagraphRun {
                item: atom.clone(),
                width: 1.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: atom,
                width: 1.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &ancestor);
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position == InlineGraphPosition::at_run_start(1)
                && matches!(opportunity.kind, InlineBreakKind::AtomicBoundary)
        }));
    }

    #[test]
    fn word_joiner_blocks_soft_wraps_at_atomic_boundaries() {
        let style = ComputedStyle::initial();
        let mut scratch = InlineBreakScratch::default();

        assert!(!inline_break_boundary_allows_soft_wrap(
            InlineBreakBoundaryContext::Atomic,
            InlineBreakBoundaryContext::Text {
                text: "\u{2060}word"
            },
            [&style, &style],
            &mut scratch,
        ));
        assert!(!inline_break_boundary_allows_soft_wrap(
            InlineBreakBoundaryContext::Text {
                text: "word\u{2060}"
            },
            InlineBreakBoundaryContext::Atomic,
            [&style, &style],
            &mut scratch,
        ));
    }

    #[test]
    fn static_position_placeholder_is_transparent_to_unbroken_text() {
        let style = ComputedStyle::initial();
        let placeholder = InlineLineItem::Atom(InlineAtom::new(
            InlineAtomContent::StaticPositionPlaceholder,
            style.clone(),
            None,
            InlineSize::new(0.0, style.line_height),
            style.font_size,
            0.0,
            None,
            None,
        ));
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("un", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: placeholder,
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("broken", style.clone())),
                width: 0.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        assert!(!opportunities.iter().any(|opportunity| {
            matches!(
                opportunity.position,
                InlineGraphPosition {
                    run_index: 1 | 2,
                    ..
                }
            ) && opportunity_is_soft_wrap(*opportunity)
        }));
    }
}
