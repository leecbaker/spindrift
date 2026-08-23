use std::rc::Rc;

use super::*;
use crate::text::{
    Uax14BoundaryProtection, character_is_css_other_space_separator,
    uax14_atomic_boundary_protection,
};
#[cfg(test)]
use crate::text::{keep_all_suppresses_break_between, manual_suppresses_break_between};

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
    phrase_relaxed_positions: Vec<usize>,
    keep_all_relaxed_positions: Vec<usize>,
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
    Text {
        text: &'a str,
        scope: Option<&'a Rc<InlineTrackingScope>>,
    },
    Atomic {
        scope: Option<&'a Rc<InlineTrackingScope>>,
    },
    /// A ruby column is measured as a coupled inline participant, but its
    /// base-side source text determines legal breaks at the boundaries between
    /// neighboring columns.
    Ruby {
        text: &'a str,
        scope: Option<&'a Rc<InlineTrackingScope>>,
    },
}

impl<'a> InlineBreakBoundaryContext<'a> {
    fn leading_character(self) -> Option<char> {
        match self {
            Self::Text { text, .. } => text.chars().next(),
            Self::Ruby { text, .. } => text.chars().next(),
            Self::Atomic { .. } => None,
        }
    }

    fn trailing_character(self) -> Option<char> {
        match self {
            Self::Text { text, .. } => text.chars().next_back(),
            Self::Ruby { text, .. } => text.chars().next_back(),
            Self::Atomic { .. } => None,
        }
    }

    fn is_atomic(self) -> bool {
        matches!(self, Self::Atomic { .. })
    }

    fn scope(self) -> Option<&'a Rc<InlineTrackingScope>> {
        match self {
            Self::Text { scope, .. } | Self::Atomic { scope, .. } | Self::Ruby { scope, .. } => {
                scope
            }
        }
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
    let materializes_trailing_soft_hyphen = visible_trailing_soft_hyphen
        && text
            .chars()
            .rev()
            .find(|character| *character != ZERO_WIDTH_SPACE)
            == Some(SOFT_HYPHEN);
    let marker_capacity = if materializes_trailing_soft_hyphen {
        hyphenate_character.len() + usize::from(preserve_joining_context) * '\u{200d}'.len_utf8()
    } else {
        0
    };
    let mut normalized = String::with_capacity(text.len().saturating_add(marker_capacity));
    for character in text.chars() {
        if character != ZERO_WIDTH_SPACE && character != SOFT_HYPHEN {
            normalized.push(character);
        }
    }
    if materializes_trailing_soft_hyphen {
        // The matching leading ZWJ is added to the following line by graph
        // materialization. Together these controls retain the source word's
        // joining context while the used hyphenate character is inserted at
        // the selected line edge.
        if preserve_joining_context {
            normalized.push('\u{200d}');
        }
        normalized.push_str(hyphenate_character);
    }
    Some(normalized)
}

pub(in crate::layout) fn remeasure_materialized_item(
    item: &mut MeasuredInlineItem,
    font_system: &mut FontSystem,
) {
    let InlineLineItem::Fragment(fragment) = &mut item.item else {
        return;
    };
    item.shaped = font_system
        .shape_untracked_inline_line(
            fragment.text(),
            fragment.style(),
            fragment.style().line_height,
        )
        .map(Rc::new);
    let base_advance = item
        .shaped
        .as_deref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    item.advance.replace_base_points(base_advance);
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
    // A `pre-wrap` sequence remains one CSS Text whitespace sequence through
    // transparent inline-box edges. The ordinary boundary builder only sees
    // adjacent graph runs, so normalize its candidates after collection and
    // then add the single legal boundary after each complete sequence.
    // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
    opportunities.retain(|opportunity| {
        !pre_wrap_preserved_space_follows_graph_position(runs, opportunity.position)
    });
    append_pre_wrap_preserved_space_sequence_break_opportunities(runs, &mut opportunities);
    if !runs.is_empty() {
        opportunities.push(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(runs.len()),
            kind: InlineBreakKind::Forced,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::None,
            discretionary: None,
        });
    }
    opportunities.sort_by_key(|opportunity| {
        (
            opportunity.position,
            opportunity.availability.fitting_stage(),
        )
    });
    opportunities.dedup_by(|left, right| {
        left.position == right.position
            && left.kind == right.kind
            && left.availability == right.availability
    });
    opportunities
}

/// Return whether a logical `pre-wrap` preserved-space sequence begins at a
/// graph position, looking through transparent inline edges.
fn pre_wrap_preserved_space_follows_graph_position(
    runs: &[InlineParagraphRun],
    position: InlineGraphPosition,
) -> bool {
    let mut run_index = position.run_index;
    if let Some(InlineParagraphRun {
        item: InlineLineItem::Fragment(fragment),
        ..
    }) = runs.get(run_index)
        && position.byte_offset > 0
        && position.byte_offset < fragment.text().len()
    {
        return fragment.style().white_space == WhiteSpace::PreWrap
            && fragment
                .text()
                .get(position.byte_offset..)
                .and_then(|suffix| suffix.chars().next())
                .is_some_and(is_css_preserved_document_space);
    }
    if let Some(InlineParagraphRun {
        item: InlineLineItem::Fragment(fragment),
        ..
    }) = runs.get(run_index)
        && position.byte_offset == fragment.text().len()
    {
        run_index += 1;
    }
    while let Some(run) = runs.get(run_index) {
        match &run.item {
            InlineLineItem::Atom(_)
                if inline_line_item_is_transparent_to_text_continuity(&run.item) =>
            {
                run_index += 1;
            }
            InlineLineItem::Fragment(fragment) => {
                return fragment.style().white_space == WhiteSpace::PreWrap
                    && fragment
                        .text()
                        .chars()
                        .next()
                        .is_some_and(is_css_preserved_document_space);
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return false,
        }
    }
    false
}

/// Add the sole post-sequence break for each logical `pre-wrap` space run.
///
/// Graph box-edge atoms have no CSS Text content, so a run such as
/// `text <span>   </span> next` must expose the break after the spaces even
/// though its source position is before the span's end-edge marker.
fn append_pre_wrap_preserved_space_sequence_break_opportunities(
    runs: &[InlineParagraphRun],
    opportunities: &mut Vec<InlineBreakOpportunity>,
) {
    for (run_index, run) in runs.iter().enumerate() {
        let InlineLineItem::Fragment(fragment) = &run.item else {
            continue;
        };
        if !inline_fragment_is_pre_wrap_hanging_space(fragment) {
            continue;
        }
        let mut next = run_index + 1;
        while runs
            .get(next)
            .is_some_and(|run| inline_line_item_is_transparent_to_text_continuity(&run.item))
        {
            next += 1;
        }
        if runs.get(next).is_some_and(|run| {
            matches!(
                &run.item,
                InlineLineItem::Fragment(next_fragment)
                    if inline_fragment_is_pre_wrap_hanging_space(next_fragment)
            )
        }) {
            continue;
        }
        if next == runs.len() {
            continue;
        }
        opportunities.push(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(run_index + 1),
            kind: InlineBreakKind::PreservedSpace,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::PreWrapHang,
            discretionary: None,
        });
    }
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
    output.extend(scratch.break_positions.iter().copied().map(|position| {
        inline_text_break_opportunity(
            run_index,
            text,
            fragment.style(),
            position,
            text_discretionary_break_availability(fragment.style(), text, position),
        )
    }));
    collect_auto_phrase_relaxed_wrap_opportunities(
        text,
        line_break_policy,
        &mut scratch.phrase_relaxed_positions,
    );
    output.extend(
        scratch
            .phrase_relaxed_positions
            .iter()
            .copied()
            .filter(|position| {
                *position > 0
                    && *position < text.len()
                    && text.is_char_boundary(*position)
                    && !text[*position..].starts_with('\u{200b}')
            })
            .map(|position| {
                inline_text_break_opportunity(
                    run_index,
                    text,
                    fragment.style(),
                    position,
                    BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::AutoPhraseWrap),
                )
            }),
    );
    // A soft hyphen at the end of a styled inline fragment is a break inside
    // that source fragment, even though its selected graph position coincides
    // with the following fragment's boundary.  UAX #14's interior iterator
    // deliberately excludes terminal offsets; retain this CSS Text
    // discretionary edge separately so the used hyphenate character is not
    // lost when an inline style changes immediately after U+00AD.
    // <https://www.w3.org/TR/css-text-3/#valdef-hyphens-manual>
    if text.ends_with('\u{00ad}') {
        output.push(inline_text_break_opportunity(
            run_index,
            text,
            fragment.style(),
            text.len(),
            text_discretionary_break_availability(fragment.style(), text, text.len()),
        ));
    }
    if matches!(fragment.style().word_break, css::WordBreak::KeepAll) {
        collect_keep_all_relaxed_wrap_opportunities(
            text,
            line_break_policy,
            &mut scratch.keep_all_relaxed_positions,
        );
        for &position in &scratch.keep_all_relaxed_positions {
            if position == 0 || position >= text.len() || !text.is_char_boundary(position) {
                continue;
            }
            output.push(inline_text_break_opportunity(
                run_index,
                text,
                fragment.style(),
                position,
                BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::KeepAll),
            ));
        }
    }
    if let Some(overflow_wrap) = effective_overflow_wrap(fragment.style()) {
        // `overflow-wrap` supplies grapheme-cluster fallbacks independently
        // of `keep-all`. In particular, a literal hyphen's ordinary UAX #14
        // boundary must never be replaced by an earlier `keep-all` fallback.
        // <https://drafts.csswg.org/css-text-3/#overflow-wrap-property>
        collect_grapheme_cluster_inner_boundaries(text, &mut scratch.grapheme_positions);
        for &position in &scratch.grapheme_positions {
            if position == 0 || position >= text.len() {
                continue;
            }
            output.push(inline_text_break_opportunity(
                run_index,
                text,
                fragment.style(),
                position,
                overflow_wrap_fallback(overflow_wrap),
            ));
        }
    }
}

/// `auto-phrase` may defer a discretionary hyphen, but must retain it in the
/// graph for the later hyphenation-relaxation fitting stage. This applies to
/// authored and automatic candidates alike, independent of whether the user
/// agent has a phrase analyzer for the declared language.
/// <https://drafts.csswg.org/css-text-4/#word-break-auto-phrase>
fn text_discretionary_break_availability(
    style: &ComputedStyle,
    text: &str,
    byte_offset: usize,
) -> BreakAvailability {
    if !text[..byte_offset].ends_with('\u{00ad}') {
        return BreakAvailability::Ordinary;
    }
    discretionary_hyphenation_availability(style.authored_discretionary_hyphenation_policy())
        .expect("disabled discretionary hyphens must be removed before graph construction")
}

pub(in crate::layout) fn inline_text_break_opportunity(
    run_index: usize,
    text: &str,
    style: &ComputedStyle,
    byte_offset: usize,
    availability: BreakAvailability,
) -> InlineBreakOpportunity {
    let soft_hyphen = text[..byte_offset].ends_with('\u{00ad}');
    let explicit_virtual = text[..byte_offset].ends_with('\u{200b}');
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
        } else if explicit_virtual {
            InlineBreakKind::ExplicitVirtual
        } else if availability.is_fallback() {
            InlineBreakKind::Emergency
        } else {
            InlineBreakKind::SoftWrap
        },
        availability,
        whitespace_edge: if hangs {
            SelectedWhitespaceEdge::PreWrapHang
        } else {
            SelectedWhitespaceEdge::None
        },
        discretionary: soft_hyphen.then_some(DiscretionaryBreakEffect {
            source_boundary: InlineGraphPosition {
                run_index,
                byte_offset,
            },
            marker_owner: DiscretionaryMarkerOwner {
                style_position: InlineGraphPosition {
                    run_index,
                    byte_offset,
                },
            },
            left_replacement: None,
            right_replacement: None,
            leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
        }),
    }
}

fn overflow_wrap_fallback(overflow_wrap: css::OverflowWrap) -> BreakAvailability {
    BreakAvailability::OverflowWrap(match overflow_wrap {
        css::OverflowWrap::Anywhere => OverflowWrapFallback::Anywhere,
        css::OverflowWrap::BreakWord => OverflowWrapFallback::BreakWord,
        css::OverflowWrap::Normal => unreachable!("normal does not create an overflow fallback"),
    })
}

fn inline_break_opportunity_at_boundary(
    boundary: usize,
    runs: &[InlineParagraphRun],
    boundary_style: &ComputedStyle,
    scratch: &mut InlineBreakScratch,
) -> Option<InlineBreakOpportunity> {
    let previous = &runs[boundary - 1].item;
    let next = &runs[boundary].item;
    // U+200B and `<wbr>` own precisely the boundary *after* the control.
    // The preceding text/run edge is not a second UAX #14 opportunity, and
    // the control itself has zero advance.
    if inline_line_item_ends_with_explicit_virtual_separator(previous) {
        return inline_line_item_allows_soft_wrap(previous).then_some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::ExplicitVirtual,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::None,
            discretionary: None,
        });
    }
    if inline_line_item_starts_with_explicit_virtual_separator(next) {
        return None;
    }
    // A float's source marker is not in-flow text, but it is a placement
    // boundary: the preceding inline material may form a line before the
    // float is positioned and the following material is then selected against
    // that float's exclusion. This remains available inside `white-space:
    // nowrap`, where it must not be mistaken for a CSS Text soft-wrap.
    // <https://www.w3.org/TR/CSS22/visuren.html#float-position>
    if matches!(next, InlineLineItem::Float(_)) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::FloatPlacement,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::None,
            discretionary: None,
        });
    }
    if inline_line_item_is_collapsible_space(next) && inline_line_item_allows_soft_wrap(next) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::CollapseAtNextLineStart,
            discretionary: None,
        });
    }
    if inline_line_item_ends_with_collapsible_space(previous)
        && inline_line_item_is_css_atomic(next)
        && inline_line_item_allows_soft_wrap(previous)
    {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::CollapseAtNextLineStart,
            discretionary: None,
        });
    }
    // CSS Text collapses a document-space sequence to one space, and that
    // retained separator supplies a soft-wrap opportunity *after* itself.
    // Keep this boundary explicit rather than relying on the surrounding
    // fragments' UAX #14 input: whitespace normalization may isolate the
    // separator in its own run, in which case there is no single text run for
    // ICU to inspect. The selected-line Phase II path owns its trimming.
    // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
    if inline_line_item_ends_with_collapsible_space(previous)
        && inline_line_item_allows_soft_wrap(previous)
    {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::CollapseAtNextLineStart,
            discretionary: None,
        });
    }
    // A preserved `pre-wrap` space sequence owns its soft-wrap opportunity at
    // its end. Suppress the ordinary UAX #14 boundary before the sequence so
    // selection retains it on the preceding line, where CSS Text Phase II can
    // apply its conditional hanging advance.
    // <https://www.w3.org/TR/css-text-3/#white-space-phase-2>
    if matches!(
        next,
        InlineLineItem::Fragment(fragment)
            if inline_fragment_is_pre_wrap_hanging_space(fragment)
    ) {
        return None;
    }
    if matches!(
        previous,
        InlineLineItem::Fragment(fragment)
            if inline_fragment_is_pre_wrap_hanging_space(fragment)
    ) {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::PreWrapHang,
            discretionary: None,
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
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::BreakSpacesRetained,
            discretionary: None,
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
            availability: match next {
                InlineLineItem::Fragment(fragment) => overflow_wrap_fallback(
                    effective_overflow_wrap(fragment.style())
                        .expect("break-spaces fallback requires overflow wrapping"),
                ),
                InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                    unreachable!("break-spaces fallback must have text")
                }
            },
            whitespace_edge: SelectedWhitespaceEdge::None,
            discretionary: None,
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
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::None,
            discretionary: Some(DiscretionaryBreakEffect {
                source_boundary: InlineGraphPosition::at_run_start(boundary),
                marker_owner: DiscretionaryMarkerOwner {
                    style_position: InlineGraphPosition {
                        run_index: boundary - 1,
                        byte_offset: match previous {
                            InlineLineItem::Fragment(fragment) => fragment.text().len(),
                            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                                unreachable!("soft hyphen boundary has preceding text")
                            }
                        },
                    },
                },
                left_replacement: None,
                right_replacement: None,
                leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
            }),
        });
    }
    if inline_line_item_is_css_atomic(previous) || inline_line_item_is_css_atomic(next) {
        return inline_atomic_boundary_opportunity(boundary, runs, boundary_style);
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

/// Return whether an item is a preserved `break-spaces` space-separator run.
///
/// CSS Text gives every preserved document space and other Unicode space
/// separator an after-character soft wrap, and lets overflow wrapping use the
/// preceding item boundary when needed.
/// <https://www.w3.org/TR/css-text-3/#valdef-white-space-break-spaces>
pub(in crate::layout) fn inline_line_item_is_break_spaces_preserved_space(
    item: &InlineLineItem,
) -> bool {
    matches!(
        item,
        InlineLineItem::Fragment(fragment)
            if fragment.style().white_space == WhiteSpace::BreakSpaces
                && fragment.text().chars().all(|character| {
                    is_css_preserved_document_space(character)
                        || character_is_css_other_space_separator(character)
                })
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
) -> Option<InlineBreakOpportunity> {
    let before = inline_break_context_before_boundary(runs, boundary)?;
    let after = inline_break_context_after_boundary(runs, boundary)?;
    // CSS Text assigns an opportunity at an atomic-inline boundary to the
    // nearest common ancestor of the adjacent inline-level participants. The
    // graph's paragraph context is that ancestor for atomic participants;
    // their descendant styles still control their own internal text only.
    // <https://drafts.csswg.org/css-text-3/#line-break-details>
    inline_atomic_boundary_allows_soft_wrap(before, after, boundary_style).then_some(
        InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::AtomicBoundary,
            availability: BreakAvailability::Ordinary,
            whitespace_edge: SelectedWhitespaceEdge::None,
            discretionary: None,
        },
    )
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
                    scope: fragment.tracking_scope(),
                });
            }
            InlineLineItem::Atom(atom) if inline_line_item_is_transparent_text_edge(atom) => {}
            InlineLineItem::Atom(atom @ InlineAtom { .. })
                if matches!(atom.content(), InlineAtomContent::Ruby { .. }) =>
            {
                let InlineAtomContent::Ruby { base_text, .. } = atom.content() else {
                    unreachable!("ruby atom matched above")
                };
                return Some(InlineBreakBoundaryContext::Ruby {
                    text: base_text,
                    scope: atom.tracking_scope(),
                });
            }
            InlineLineItem::Atom(atom) if inline_line_item_is_css_atomic(&run.item) => {
                return Some(InlineBreakBoundaryContext::Atomic {
                    scope: atom.tracking_scope(),
                });
            }
            InlineLineItem::Float(_) => {}
            InlineLineItem::Atom(_) => return None,
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
                    scope: fragment.tracking_scope(),
                });
            }
            InlineLineItem::Atom(atom) if inline_line_item_is_transparent_text_edge(atom) => {}
            InlineLineItem::Atom(atom @ InlineAtom { .. })
                if matches!(atom.content(), InlineAtomContent::Ruby { .. }) =>
            {
                let InlineAtomContent::Ruby { base_text, .. } = atom.content() else {
                    unreachable!("ruby atom matched above")
                };
                return Some(InlineBreakBoundaryContext::Ruby {
                    text: base_text,
                    scope: atom.tracking_scope(),
                });
            }
            InlineLineItem::Atom(atom) if inline_line_item_is_css_atomic(&run.item) => {
                return Some(InlineBreakBoundaryContext::Atomic {
                    scope: atom.tracking_scope(),
                });
            }
            InlineLineItem::Float(_) => {}
            InlineLineItem::Atom(_) => return None,
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
        if !inline_line_item_is_transparent_to_text_continuity(&runs[edge_start].item)
            || inline_line_item_is_transparent_to_text_continuity(&runs[edge_start - 1].item)
        {
            continue;
        }
        let edge_end = (edge_start + 1..runs.len())
            .find(|index| !inline_line_item_is_transparent_to_text_continuity(&runs[*index].item))
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
            InlineLineItem::Float(_) => {}
            InlineLineItem::Atom(_) => return None,
        }
    }
    None
}

pub(in crate::layout) fn inline_line_item_is_transparent_text_edge_item(
    item: &InlineLineItem,
) -> bool {
    matches!(item, InlineLineItem::Atom(atom) if inline_line_item_is_transparent_text_edge(atom))
}

/// Return whether an inline graph item is absent from the CSS Text stream.
///
/// A float retains its graph marker so source-order layout can position it,
/// but CSS Text determines boundaries as though out-of-flow boxes were not
/// present. Inline box edges have the same text-stream behavior while still
/// owning their paint and box geometry.
/// <https://www.w3.org/TR/css-text-3/#line-break-details>
fn inline_line_item_is_transparent_to_text_continuity(item: &InlineLineItem) -> bool {
    inline_line_item_is_transparent_text_edge_item(item) || matches!(item, InlineLineItem::Float(_))
}

/// A text-autospace edge contributes its advance but does not replace the
/// UAX #14/CSS Text boundary between its neighboring text fragments.
fn inline_line_item_is_transparent_text_edge(atom: &InlineAtom) -> bool {
    matches!(
        atom.content(),
        InlineAtomContent::InlineEdge(
            InlineEdgeRole::BoxEdge(_) | InlineEdgeRole::TextAutospace(_)
        ) | InlineAtomContent::StaticPositionPlaceholder
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
        inline_fragment_boundary_owner_allows_soft_wrap(previous, next),
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
    boundary_owner_allows_soft_wrap: bool,
    scratch: &mut InlineBreakScratch,
) -> bool {
    if before.is_empty() || after.is_empty() {
        return false;
    }
    scratch.join(before, after);
    inline_uax14_boundary_allows_soft_wrap(
        before.len(),
        styles,
        boundary_owner_allows_soft_wrap,
        scratch,
    )
}

fn inline_uax14_boundary_allows_soft_wrap(
    boundary: usize,
    styles: [&ComputedStyle; 2],
    boundary_owner_allows_soft_wrap: bool,
    scratch: &mut InlineBreakScratch,
) -> bool {
    if !boundary_owner_allows_soft_wrap {
        return false;
    }
    let mut previous_policy = None;
    for (style_index, style) in styles.into_iter().enumerate() {
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

/// Return whether the nearest common inline ancestor permits a cross-element
/// soft wrap.
///
/// CSS Text makes `white-space` ownership explicit at a boundary between two
/// characters or atomic inlines. The remaining line-breaking properties are
/// intentionally still taken from the adjacent source styles, as their
/// cross-boundary ownership is undefined at this level of the specification:
/// <https://drafts.csswg.org/css-text-3/#line-break-details>.
fn inline_boundary_owner_allows_soft_wrap(
    left: Option<&Rc<InlineTrackingScope>>,
    right: Option<&Rc<InlineTrackingScope>>,
    fallback: &ComputedStyle,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            InlineTrackingScope::common_boundary_policy(left.as_ref(), right.as_ref())
                .allows_soft_wrap()
        }
        _ => fallback.allows_soft_wrap(),
    }
}

fn inline_fragment_boundary_owner_allows_soft_wrap(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    match (previous.tracking_scope(), next.tracking_scope()) {
        (Some(left), Some(right)) => {
            InlineTrackingScope::common_boundary_policy(left.as_ref(), right.as_ref())
                .allows_soft_wrap()
        }
        // Unit tests and synthetic consumers can build fragments directly,
        // without graph-owned lexical scopes. Preserve the historical local
        // behavior there; normal graph construction always retains scopes.
        _ => previous.style().allows_soft_wrap() || next.style().allows_soft_wrap(),
    }
}

/// Resolve CSS Text's atomic-inline soft-wrap override.
///
/// The nearest common inline ancestor controls whether wrapping is enabled.
/// When it is, CSS Text requires a break before and after each atomic inline,
/// overriding ordinary UAX #14 punctuation and word-breaking restrictions.
/// Only non-NBSP `GL`, `WJ`, and `ZWJ` characters retain their prohibition.
/// <https://www.w3.org/TR/css-text-3/#line-break-details>
fn inline_atomic_boundary_allows_soft_wrap(
    before: InlineBreakBoundaryContext<'_>,
    after: InlineBreakBoundaryContext<'_>,
    fallback_boundary_style: &ComputedStyle,
) -> bool {
    inline_boundary_owner_allows_soft_wrap(before.scope(), after.scope(), fallback_boundary_style)
        && !((before.is_atomic()
            && after.leading_character().is_some_and(|character| {
                !matches!(
                    uax14_atomic_boundary_protection(character),
                    Uax14BoundaryProtection::None
                )
            }))
            || (after.is_atomic()
                && before.trailing_character().is_some_and(|character| {
                    !matches!(
                        uax14_atomic_boundary_protection(character),
                        Uax14BoundaryProtection::None
                    )
                })))
}

fn inline_fragment_boundary_opportunity(
    boundary: usize,
    previous: &InlineFragment,
    next: &InlineFragment,
    scratch: &mut InlineBreakScratch,
) -> Option<InlineBreakOpportunity> {
    let boundary_owner_allows_soft_wrap =
        inline_fragment_boundary_owner_allows_soft_wrap(previous, next);
    let ordinary_wrap = inline_fragment_boundary_allows_soft_wrap(previous, next, scratch);
    let overflow_wrap =
        inline_fragment_boundary_overflow_wrap(previous, next, boundary_owner_allows_soft_wrap);
    if !ordinary_wrap && overflow_wrap.is_none() {
        return None;
    }
    let availability = if ordinary_wrap {
        BreakAvailability::Ordinary
    } else {
        overflow_wrap_fallback(overflow_wrap.expect("fallback was checked above"))
    };
    Some(InlineBreakOpportunity {
        position: InlineGraphPosition::at_run_start(boundary),
        kind: if availability.is_fallback() {
            InlineBreakKind::Emergency
        } else {
            InlineBreakKind::SoftWrap
        },
        availability,
        whitespace_edge: SelectedWhitespaceEdge::None,
        discretionary: None,
    })
}

fn inline_line_item_ends_with_explicit_virtual_separator(item: &InlineLineItem) -> bool {
    matches!(item, InlineLineItem::Fragment(fragment) if fragment.text().ends_with('\u{200b}'))
}

fn inline_line_item_starts_with_explicit_virtual_separator(item: &InlineLineItem) -> bool {
    matches!(item, InlineLineItem::Fragment(fragment) if fragment.text().starts_with('\u{200b}'))
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
    boundary_owner_allows_soft_wrap: bool,
) -> Option<css::OverflowWrap> {
    if !boundary_owner_allows_soft_wrap {
        return None;
    }
    [previous.style(), next.style()]
        .into_iter()
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
#[cfg(test)]
fn inline_fragment_boundary_is_min_content_eligible(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    inline_fragment_boundary_is_min_content_eligible_with_owner(
        previous,
        next,
        inline_fragment_boundary_owner_allows_soft_wrap(previous, next),
    )
}

#[cfg(test)]
fn inline_fragment_boundary_is_min_content_eligible_with_owner(
    previous: &InlineFragment,
    next: &InlineFragment,
    boundary_owner_allows_soft_wrap: bool,
) -> bool {
    if !boundary_owner_allows_soft_wrap {
        return false;
    }
    let previous_character = previous.text().chars().next_back();
    let next_character = next.text().chars().next();
    [previous.style(), next.style()].into_iter().any(|style| {
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
#[cfg(test)]
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
    use crate::css::ContentLanguage;

    #[test]
    fn selected_soft_hyphen_preserves_trailing_joining_context() {
        assert_eq!(
            normalize_materialized_fragment_text("قىل\u{00ad}", true, true, "\u{a0}\u{640}"),
            Some("قىل\u{200d}\u{a0}\u{640}".to_string())
        );
    }

    #[test]
    fn normalizing_controls_uses_the_last_non_zero_width_space_soft_hyphen() {
        assert_eq!(
            normalize_materialized_fragment_text("A\u{00ad}\u{200b}", true, false, "="),
            Some("A=".to_string())
        );
        assert_eq!(
            normalize_materialized_fragment_text("A\u{00ad}\u{200b}B", true, false, "="),
            Some("AB".to_string())
        );
        assert_eq!(
            normalize_materialized_fragment_text("A\u{200b}B", false, false, "="),
            Some("AB".to_string())
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

    fn scoped_fragment(
        text: &str,
        style: ComputedStyle,
        scope: Rc<InlineTrackingScope>,
    ) -> InlineFragment {
        fragment(text, style).with_tracking_scope(scope)
    }

    fn scoped_atomic(style: ComputedStyle, scope: Rc<InlineTrackingScope>) -> InlineAtom {
        InlineAtom::new(
            InlineAtomContent::Canvas,
            style.clone(),
            None,
            InlineSize::new(1.0, style.line_height),
            style.font_size,
            0.0,
            None,
            None,
        )
        .with_tracking_scope(scope)
    }

    fn atomic_text_boundary_allows_soft_wrap(style: &ComputedStyle, text: &str) -> bool {
        let before = InlineBreakBoundaryContext::Atomic { scope: None };
        let after = InlineBreakBoundaryContext::Text { text, scope: None };
        inline_atomic_boundary_allows_soft_wrap(before, after, style)
    }

    fn text_autospace_edge(style: &ComputedStyle) -> InlineAtom {
        InlineAtom::new(
            InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(
                InlineTextBoundarySpacing::new(layout_pt(0.0)),
            )),
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
                    && opportunity.availability.participates_in_min_content()
            }));
        }
    }

    #[test]
    fn sibling_pre_spans_use_their_normal_common_ancestor_for_cjk_breaking() {
        let ancestor = ComputedStyle::initial();
        let mut descendant = ancestor.clone();
        descendant.white_space = WhiteSpace::Pre;
        let root = InlineTrackingScope::root(&ancestor);
        let left_scope = InlineTrackingScope::child(Rc::clone(&root), &descendant);
        let right_scope = InlineTrackingScope::child(root, &descendant);
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(scoped_fragment(
                    "口",
                    descendant.clone(),
                    left_scope,
                )),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(scoped_fragment("口", descendant, right_scope)),
                width: 0.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &ancestor);
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position == InlineGraphPosition::at_run_start(1)
                && matches!(opportunity.kind, InlineBreakKind::SoftWrap)
        }));
    }

    #[test]
    fn sibling_pre_spans_use_their_normal_common_ancestor_for_atomic_breaking() {
        let ancestor = ComputedStyle::initial();
        let mut descendant = ancestor.clone();
        descendant.white_space = WhiteSpace::Pre;
        let root = InlineTrackingScope::root(&ancestor);
        let left_scope = InlineTrackingScope::child(Rc::clone(&root), &descendant);
        let right_scope = InlineTrackingScope::child(root, &descendant);
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Atom(scoped_atomic(descendant.clone(), left_scope)),
                width: 1.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Atom(scoped_atomic(descendant, right_scope)),
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
    fn keep_all_excludes_thai_internal_candidates_from_min_content() {
        let mut style = ComputedStyle::initial();
        style.word_break = css::WordBreak::KeepAll;
        let runs = vec![InlineParagraphRun {
            item: InlineLineItem::Fragment(fragment("กรุงเทพ", style.clone())),
            width: 0.0,
            shaped: None,
        }];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        assert!(!opportunities.iter().any(|opportunity| {
            opportunity.position.run_index == 0
                && opportunity.position.byte_offset > 0
                && opportunity.position.byte_offset < "กรุงเทพ".len()
                && opportunity.availability.participates_in_min_content()
        }));
    }

    #[test]
    fn keep_all_retains_post_hyphen_as_an_ordinary_soft_wrap() {
        let mut style = ComputedStyle::initial();
        style.word_break = css::WordBreak::KeepAll;
        let text = "AB-CD-EF";
        let runs = vec![InlineParagraphRun {
            item: InlineLineItem::Fragment(fragment(text, style.clone())),
            width: 0.0,
            shaped: None,
        }];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        let post_hyphen = opportunities
            .iter()
            .find(|opportunity| opportunity.position.byte_offset == "AB-".len())
            .expect("keep-all retains the UAX #14 post-hyphen boundary");
        assert_eq!(post_hyphen.kind, BreakEffect::SoftWrap);
        assert_eq!(post_hyphen.availability, BreakAvailability::Ordinary);
        assert!(
            !opportunities.iter().any(|opportunity| {
                opportunity.position.byte_offset == "AB-CD".len()
                    && matches!(
                        opportunity.availability,
                        BreakAvailability::RelaxedWordBreak(WordBreakRelaxation::KeepAll)
                    )
            }),
            "Latin text before a literal hyphen is not a keep-all relaxation"
        );
    }

    #[test]
    fn explicit_virtual_separator_owns_one_ordinary_graph_boundary() {
        let mut style = ComputedStyle::initial();
        style.language = ContentLanguage::from_html_attribute("th");
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("กรุงเทพ", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("\u{200b}", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("คือ", style.clone())),
                width: 0.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        let separators = opportunities
            .iter()
            .filter(|opportunity| {
                matches!(opportunity.kind, InlineBreakKind::ExplicitVirtual)
                    && opportunity.position == InlineGraphPosition::at_run_start(2)
            })
            .collect::<Vec<_>>();
        assert_eq!(separators.len(), 1);
        assert_eq!(separators[0].availability, BreakAvailability::Ordinary);
        assert!(
            !opportunities.iter().any(|opportunity| {
                opportunity.position == InlineGraphPosition::at_run_start(1)
            }),
            "there is no competing boundary before U+200B"
        );
        assert!(
            !opportunities.iter().any(|opportunity| {
                opportunity.position.run_index == 0
                    && opportunity.position.byte_offset > 0
                    && opportunity.position.byte_offset < "กรุงเทพ".len()
                    && opportunity.availability.participates_in_min_content()
            }),
            "Thai named entities remain indivisible in ordinary min-content sizing"
        );
    }

    #[test]
    fn auto_phrase_uses_icu_boundaries_across_transparent_text_edges() {
        let mut style = ComputedStyle::initial();
        style.word_break = css::WordBreak::AutoPhrase;
        style.language = ContentLanguage::from_html_attribute("ja");
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("東京へ", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Atom(box_edge(&style)),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("行きましょう。", style.clone())),
                width: 0.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position == InlineGraphPosition::at_run_start(1)
                && matches!(opportunity.kind, InlineBreakKind::SoftWrap)
                && opportunity.availability.participates_in_min_content()
        }));
    }

    #[test]
    fn pre_wrap_space_sequence_owns_the_break_after_itself() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::PreWrap;
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("one", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment(" ", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("two", style.clone())),
                width: 0.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        assert!(
            !opportunities.iter().any(|opportunity| {
                opportunity.position == InlineGraphPosition::at_run_start(1)
            }),
            "the preserved space must remain with the preceding text"
        );
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position == InlineGraphPosition::at_run_start(2)
                && matches!(opportunity.kind, InlineBreakKind::PreservedSpace)
                && opportunity.whitespace_edge == SelectedWhitespaceEdge::PreWrapHang
        }));
    }

    #[test]
    fn pre_wrap_space_sequence_crosses_transparent_inline_edges() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::PreWrap;
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("one", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Atom(box_edge(&style)),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("  ", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Atom(box_edge(&style)),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("two", style.clone())),
                width: 0.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        for boundary in [1, 2] {
            assert!(
                !opportunities.iter().any(|opportunity| opportunity.position
                    == InlineGraphPosition::at_run_start(boundary)),
                "pre-wrap spaces must stay with the preceding source through boundary {boundary}: {opportunities:?}"
            );
        }
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position == InlineGraphPosition::at_run_start(3)
                && matches!(opportunity.kind, InlineBreakKind::PreservedSpace)
                && opportunity.whitespace_edge == SelectedWhitespaceEdge::PreWrapHang
        }));
    }

    #[test]
    fn collapsible_space_break_marks_the_next_line_start_for_collapse() {
        let style = ComputedStyle::initial();
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("one", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment(" ", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("two", style.clone())),
                width: 0.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position == InlineGraphPosition::at_run_start(1)
                && opportunity.trims_next_line_start()
        }));
    }

    #[test]
    fn break_spaces_other_space_separator_owns_the_break_after_itself() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::BreakSpaces;
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("xx", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("\u{1680}", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("あ", style.clone())),
                width: 0.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        assert!(
            !opportunities.iter().any(|opportunity| {
                opportunity.position == InlineGraphPosition::at_run_start(1)
            }),
            "break-spaces must not add an ordinary break before an other-space separator"
        );
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position == InlineGraphPosition::at_run_start(2)
                && matches!(opportunity.kind, InlineBreakKind::BreakSpaces)
                && opportunity.whitespace_edge == SelectedWhitespaceEdge::BreakSpacesRetained
        }));

        let combined_runs = vec![InlineParagraphRun {
            item: InlineLineItem::Fragment(fragment("xx\u{1680}あ", style.clone())),
            width: 0.0,
            shaped: None,
        }];
        let opportunities = inline_break_opportunities_for_runs(&combined_runs, &style);
        assert!(
            !opportunities.iter().any(|opportunity| {
                opportunity.position
                    == InlineGraphPosition {
                        run_index: 0,
                        byte_offset: "xx".len(),
                    }
            }),
            "break-spaces must not add an ordinary break before an other-space separator"
        );
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position
                == InlineGraphPosition {
                    run_index: 0,
                    byte_offset: "xx\u{1680}".len(),
                }
                && matches!(opportunity.kind, InlineBreakKind::SoftWrap)
                && opportunity.whitespace_edge == SelectedWhitespaceEdge::None
        }));

        let narrow_no_break_space_runs = vec![InlineParagraphRun {
            item: InlineLineItem::Fragment(fragment("xx\u{202f}あ", style)),
            width: 0.0,
            shaped: None,
        }];
        let opportunities = inline_break_opportunities_for_runs(
            &narrow_no_break_space_runs,
            &ComputedStyle::initial(),
        );
        assert!(
            !opportunities.iter().any(|opportunity| {
                opportunity.position
                    == InlineGraphPosition {
                        run_index: 0,
                        byte_offset: "xx".len(),
                    }
            }),
            "U+202F retains its ordinary UAX #14 GL protection before itself"
        );
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position
                == InlineGraphPosition {
                    run_index: 0,
                    byte_offset: "xx\u{202f}".len(),
                }
                && matches!(opportunity.kind, InlineBreakKind::SoftWrap)
                && opportunity.whitespace_edge == SelectedWhitespaceEdge::None
        }));
    }

    #[test]
    fn line_break_anywhere_can_break_before_other_space_separator() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::BreakSpaces;
        style.line_break = css::LineBreak::Anywhere;
        let runs = vec![InlineParagraphRun {
            item: InlineLineItem::Fragment(fragment("xx\u{1680}あ", style)),
            width: 0.0,
            shaped: None,
        }];

        let opportunities = inline_break_opportunities_for_runs(&runs, &ComputedStyle::initial());
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position
                == InlineGraphPosition {
                    run_index: 0,
                    byte_offset: "xx".len(),
                }
                && matches!(opportunity.kind, InlineBreakKind::SoftWrap)
                && !opportunity.availability.is_fallback()
        }));
    }

    #[test]
    fn line_break_anywhere_attaches_a_zwj_to_the_following_typographic_unit() {
        let mut style = ComputedStyle::initial();
        style.line_break = css::LineBreak::Anywhere;
        let text = "a\u{200d}a";
        let runs = vec![InlineParagraphRun {
            item: InlineLineItem::Fragment(fragment(text, style)),
            width: 0.0,
            shaped: None,
        }];

        let opportunities = inline_break_opportunities_for_runs(&runs, &ComputedStyle::initial());
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position
                == InlineGraphPosition {
                    run_index: 0,
                    byte_offset: "a".len(),
                }
                && matches!(opportunity.kind, InlineBreakKind::SoftWrap)
                && !opportunity.availability.is_fallback()
        }));
    }

    #[test]
    fn line_break_anywhere_attaches_a_word_joiner_to_the_following_typographic_unit() {
        let mut style = ComputedStyle::initial();
        style.line_break = css::LineBreak::Anywhere;
        let text = "a\u{2060}a";
        let runs = vec![InlineParagraphRun {
            item: InlineLineItem::Fragment(fragment(text, style)),
            width: 0.0,
            shaped: None,
        }];

        let opportunities = inline_break_opportunities_for_runs(&runs, &ComputedStyle::initial());
        assert!(opportunities.iter().any(|opportunity| {
            opportunity.position
                == InlineGraphPosition {
                    run_index: 0,
                    byte_offset: "a".len(),
                }
                && matches!(opportunity.kind, InlineBreakKind::SoftWrap)
                && !opportunity.availability.is_fallback()
        }));
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
    fn atomic_inline_overrides_punctuation_and_keep_all_on_both_sides() {
        let mut style = ComputedStyle::initial();
        style.word_break = css::WordBreak::KeepAll;
        let atom = InlineLineItem::Atom(InlineAtom::new(
            InlineAtomContent::Canvas,
            style.clone(),
            None,
            InlineSize::new(1.0, style.line_height),
            style.font_size,
            0.0,
            None,
            None,
        ));
        let runs = vec![
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment("A", style.clone())),
                width: 0.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: atom,
                width: 1.0,
                shaped: None,
            },
            InlineParagraphRun {
                item: InlineLineItem::Fragment(fragment(":", style.clone())),
                width: 0.0,
                shaped: None,
            },
        ];

        let opportunities = inline_break_opportunities_for_runs(&runs, &style);
        for position in [
            InlineGraphPosition::at_run_start(1),
            InlineGraphPosition::at_run_start(2),
        ] {
            assert!(opportunities.iter().any(|opportunity| {
                opportunity.position == position
                    && matches!(opportunity.kind, InlineBreakKind::AtomicBoundary)
            }));
        }
    }

    #[test]
    fn atomic_boundaries_keep_nbsp_compatibility_and_gl_wj_zwj_suppression() {
        let style = ComputedStyle::initial();

        assert!(atomic_text_boundary_allows_soft_wrap(
            &style,
            "\u{00a0}word"
        ));
        for text in [
            "\u{2007}word",
            "\u{202f}word",
            "\u{2060}word",
            "\u{200d}word",
        ] {
            assert!(
                !atomic_text_boundary_allows_soft_wrap(&style, text),
                "{text:?} must suppress an atomic-inline boundary break"
            );
        }
        assert!(!inline_atomic_boundary_allows_soft_wrap(
            InlineBreakBoundaryContext::Text {
                text: "word\u{202f}",
                scope: None,
            },
            InlineBreakBoundaryContext::Atomic { scope: None },
            &style,
        ));
    }

    #[test]
    fn atomic_boundaries_respect_a_non_wrapping_common_ancestor() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::NoWrap;
        assert!(
            !atomic_text_boundary_allows_soft_wrap(&style, ":"),
            "atomic-inline compatibility breaks still require wrapping to be enabled"
        );
    }

    #[test]
    fn word_joiner_blocks_soft_wraps_at_atomic_boundaries() {
        let style = ComputedStyle::initial();
        assert!(!inline_atomic_boundary_allows_soft_wrap(
            InlineBreakBoundaryContext::Atomic { scope: None },
            InlineBreakBoundaryContext::Text {
                text: "\u{2060}word",
                scope: None,
            },
            &style,
        ));
        assert!(!inline_atomic_boundary_allows_soft_wrap(
            InlineBreakBoundaryContext::Text {
                text: "word\u{2060}",
                scope: None,
            },
            InlineBreakBoundaryContext::Atomic { scope: None },
            &style,
        ));
    }

    #[test]
    fn opening_cjk_bracket_breaks_before_an_atomic_inline() {
        let style = ComputedStyle::initial();
        assert!(inline_atomic_boundary_allows_soft_wrap(
            InlineBreakBoundaryContext::Text {
                text: "「",
                scope: None,
            },
            InlineBreakBoundaryContext::Atomic { scope: None },
            &style,
        ));
    }

    #[test]
    fn static_position_placeholder_is_transparent_to_unbroken_text() {
        let mut style = ComputedStyle::initial();
        style.white_space = WhiteSpace::NoWrap;
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
