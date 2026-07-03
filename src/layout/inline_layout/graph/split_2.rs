use super::*;
use std::rc::Rc;

pub(in crate::layout) fn normalize_materialized_fragment_text(
    text: &str,
    visible_trailing_soft_hyphen: bool,
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
        normalized.push('-');
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
) -> Vec<InlineBreakOpportunity> {
    let mut opportunities = Vec::new();
    for (run_index, run) in runs.iter().enumerate() {
        opportunities.extend(inline_break_opportunities_inside_run(run_index, run));
    }
    opportunities.extend(inline_break_opportunities_across_transparent_edges(runs));
    for boundary in 1..runs.len() {
        if let Some(opportunity) = inline_break_opportunity_at_boundary(boundary, runs) {
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

pub(in crate::layout) fn inline_break_opportunities_inside_run(
    run_index: usize,
    run: &InlineParagraphRun,
) -> Vec<InlineBreakOpportunity> {
    let InlineLineItem::Fragment(fragment) = &run.item else {
        return Vec::new();
    };
    if !fragment.style().white_space.allows_soft_wrap() {
        return Vec::new();
    }
    let text = fragment.text();
    let mut output = Vec::new();
    for position in measured_break_opportunities(text, fragment.style()) {
        if position == 0
            || position >= text.len()
            || !text.is_char_boundary(position)
            || text[position..].starts_with('\u{200b}')
        {
            continue;
        }
        output.push(inline_text_break_opportunity(
            run_index,
            text,
            fragment.style(),
            position,
            false,
            text_break_is_min_content_eligible(text, fragment.style(), position),
        ));
    }
    if matches!(fragment.style().overflow_wrap, css::OverflowWrap::BreakWord) {
        for position in grapheme_cluster_inner_boundaries(text) {
            if position == 0
                || position >= text.len()
                || output
                    .iter()
                    .any(|opportunity| opportunity.position.byte_offset == position)
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
    output
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

pub(in crate::layout) fn inline_break_opportunity_at_boundary(
    boundary: usize,
    runs: &[InlineParagraphRun],
) -> Option<InlineBreakOpportunity> {
    let previous = &runs[boundary - 1].item;
    let next = &runs[boundary].item;
    if inline_line_item_is_collapsible_space(next)
        || inline_line_item_is_pre_wrap_hanging_space(next)
    {
        return Some(InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::PreservedSpace,
            priority: 220,
            trims: true,
            hangs: inline_line_item_is_pre_wrap_hanging_space(next),
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        });
    }
    if inline_line_item_ends_with_collapsible_space(previous)
        && inline_line_item_is_css_atomic(next)
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
    if matches!(
        previous,
        InlineLineItem::Fragment(fragment)
            if fragment.style().white_space == WhiteSpace::BreakSpaces
                && fragment.text().chars().all(is_css_collapsible_whitespace)
    ) {
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
    if inline_line_item_is_float_marker(previous) || inline_line_item_is_float_marker(next) {
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
    if inline_line_item_is_css_atomic(previous) || inline_line_item_is_css_atomic(next) {
        return inline_atomic_boundary_opportunity(boundary, runs);
    }
    if let (InlineLineItem::Fragment(previous), InlineLineItem::Fragment(next)) = (previous, next) {
        return inline_fragment_boundary_opportunity(boundary, previous, next);
    }
    None
}

pub(in crate::layout) fn inline_line_item_is_css_atomic(item: &InlineLineItem) -> bool {
    matches!(
        item,
        InlineLineItem::Atom(atom)
            if !matches!(
                atom.content(),
                InlineAtomContent::InlineEdge(_) | InlineAtomContent::Leader(_)
            )
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

pub(in crate::layout) fn inline_atomic_boundary_opportunity(
    boundary: usize,
    runs: &[InlineParagraphRun],
) -> Option<InlineBreakOpportunity> {
    let (before, style) = inline_break_context_before_boundary(runs, boundary)?;
    let after = inline_break_context_after_boundary(runs, boundary)?;
    inline_atomic_boundary_allows_soft_wrap(&before, &after, style).then_some(
        InlineBreakOpportunity {
            position: InlineGraphPosition::at_run_start(boundary),
            kind: InlineBreakKind::AtomicBoundary,
            priority: 120,
            trims: false,
            hangs: false,
            soft_hyphen: false,
            emergency: false,
            min_content: true,
        },
    )
}

pub(in crate::layout) fn inline_break_context_before_boundary(
    runs: &[InlineParagraphRun],
    boundary: usize,
) -> Option<(String, &ComputedStyle)> {
    for run in runs[..boundary].iter().rev() {
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                return Some((fragment.text().to_string(), fragment.style()));
            }
            InlineLineItem::Atom(atom) if atom.content().is_box_edge() => {}
            InlineLineItem::Atom(atom) if inline_line_item_is_css_atomic(&run.item) => {
                return Some((OBJECT_REPLACEMENT_CHARACTER.to_string(), atom.style()));
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    None
}

pub(in crate::layout) fn inline_break_context_after_boundary(
    runs: &[InlineParagraphRun],
    boundary: usize,
) -> Option<String> {
    for run in &runs[boundary..] {
        match &run.item {
            InlineLineItem::Fragment(fragment) => return Some(fragment.text().to_string()),
            InlineLineItem::Atom(atom) if atom.content().is_box_edge() => {}
            InlineLineItem::Atom(_) if inline_line_item_is_css_atomic(&run.item) => {
                return Some(OBJECT_REPLACEMENT_CHARACTER.to_string());
            }
            InlineLineItem::Float(_) | InlineLineItem::Atom(_) => return None,
        }
    }
    None
}

pub(in crate::layout) fn inline_line_item_is_float_marker(item: &InlineLineItem) -> bool {
    matches!(item, InlineLineItem::Float(_))
}

pub(in crate::layout) fn inline_break_opportunities_across_transparent_edges(
    runs: &[InlineParagraphRun],
) -> Vec<InlineBreakOpportunity> {
    let mut opportunities = Vec::new();
    for edge_start in 1..runs.len() {
        if !inline_line_item_is_transparent_box_edge(&runs[edge_start].item)
            || inline_line_item_is_transparent_box_edge(&runs[edge_start - 1].item)
        {
            continue;
        }
        let edge_end = (edge_start + 1..runs.len())
            .find(|index| !inline_line_item_is_transparent_box_edge(&runs[*index].item))
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
        if let Some(opportunity) = inline_fragment_boundary_opportunity(edge_start, previous, next)
        {
            opportunities.push(opportunity);
        }
    }
    opportunities
}

pub(in crate::layout) fn previous_text_fragment_before(
    runs: &[InlineParagraphRun],
    before_run: usize,
) -> Option<&InlineFragment> {
    for run in runs[..before_run].iter().rev() {
        match &run.item {
            InlineLineItem::Fragment(fragment) => return Some(fragment),
            InlineLineItem::Atom(atom) if atom.content().is_box_edge() => {}
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    None
}

pub(in crate::layout) fn inline_line_item_is_transparent_box_edge(item: &InlineLineItem) -> bool {
    matches!(item, InlineLineItem::Atom(atom) if atom.content().is_box_edge())
}

pub(in crate::layout) fn inline_fragment_boundary_allows_soft_wrap(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    if !previous.style().white_space.allows_soft_wrap()
        || previous.text().is_empty()
        || next.text().is_empty()
    {
        return false;
    }
    let boundary = previous.text().len();
    let mut combined = String::with_capacity(previous.text().len() + next.text().len());
    combined.push_str(previous.text());
    combined.push_str(next.text());
    measured_break_opportunities(&combined, previous.style())
        .binary_search(&boundary)
        .is_ok()
}

pub(in crate::layout) fn inline_fragment_boundary_opportunity(
    boundary: usize,
    previous: &InlineFragment,
    next: &InlineFragment,
) -> Option<InlineBreakOpportunity> {
    if !inline_fragment_boundary_allows_soft_wrap(previous, next)
        && !inline_fragment_boundary_has_tracking_opportunity(previous, next)
    {
        return None;
    }
    Some(InlineBreakOpportunity {
        position: InlineGraphPosition::at_run_start(boundary),
        kind: InlineBreakKind::SoftWrap,
        priority: 100,
        trims: false,
        hangs: false,
        soft_hyphen: false,
        emergency: false,
        min_content: inline_fragment_boundary_is_min_content_eligible(previous, next),
    })
}

pub(in crate::layout) fn inline_fragment_boundary_has_tracking_opportunity(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    !previous.text().is_empty()
        && !next.text().is_empty()
        && (previous.style().used_letter_spacing() != 0.0
            || next.style().used_letter_spacing() != 0.0)
}

pub(in crate::layout) fn inline_fragment_boundary_is_min_content_eligible(
    previous: &InlineFragment,
    next: &InlineFragment,
) -> bool {
    let mut combined = String::with_capacity(previous.text().len() + next.text().len());
    combined.push_str(previous.text());
    combined.push_str(next.text());
    text_break_is_min_content_eligible(&combined, previous.style(), previous.text().len())
}
