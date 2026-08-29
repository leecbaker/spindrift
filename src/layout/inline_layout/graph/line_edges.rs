use super::*;

/// The complete shaping and used-text request for one selected discretionary
/// edge.  Source fragments remain unchanged in the graph; this request is
/// applied only to the short-lived line materialization used for measuring and
/// painting the selected line.
#[derive(Debug, Clone, Copy)]
struct SelectedLineShapingRequest {
    effect: DiscretionaryBreakEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectedLineEdge {
    Leading,
    Trailing,
}

/// Apply CSS Text's selected discretionary behavior without inspecting the
/// language or spelling in the materializer.  Language resources create the
/// effect; this generic boundary applies its two replacements, non-painting
/// shaping context, and separate used marker.
pub(super) fn apply_selected_discretionary_break(
    items: &mut Vec<MeasuredInlineItem>,
    effect: Option<DiscretionaryBreakEffect>,
    edge: SelectedLineEdge,
    font_system: &mut FontSystem,
    graph_runs: &[InlineParagraphRun],
) {
    let Some(effect) = effect else {
        return;
    };
    let request = SelectedLineShapingRequest { effect };
    match edge {
        SelectedLineEdge::Trailing => {
            if let Some(replacement) = request.effect.left_replacement {
                apply_trailing_line_edge_replacement(items, replacement, font_system);
            }
            if request.effect.leading_shaping_context == SelectedLineShapingContext::PreserveJoining
                && materialized_items_have_joining_behavior(items)
            {
                append_materialized_line_joiner(items, font_system);
            }
            // The marker owns the trailing shaping context. Keeping its ZWJ
            // in the generated item places it immediately before
            // `hyphenate-character` in logical text, including RTL markers
            // whose leading NBSP must not be shaped as a line-start document
            // space.
            append_discretionary_marker(items, request.effect, graph_runs, font_system);
        }
        SelectedLineEdge::Leading => {
            if let Some(replacement) = request.effect.right_replacement {
                apply_leading_line_edge_replacement(items, replacement, font_system);
            }
            if request.effect.leading_shaping_context == SelectedLineShapingContext::PreserveJoining
                && materialized_items_have_joining_behavior(items)
            {
                // This helper presently uses a ZWJ-backed shaper request. The
                // ZWJ never becomes a separate paint item; the typed request
                // above is the sole authority for its use at a selected edge.
                prepend_materialized_line_joiner(items, font_system);
            }
        }
    }
}

fn apply_trailing_line_edge_replacement(
    items: &mut [MeasuredInlineItem],
    replacement: InlineLineEdgeReplacement,
    font_system: &mut FontSystem,
) {
    let Some(item) = items.iter_mut().rev().find(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(fragment) = &mut item.item else {
        return;
    };
    let text = fragment.text();
    let Some(prefix_end) = text.len().checked_sub(replacement.source_bytes) else {
        return;
    };
    if !text.is_char_boundary(prefix_end) {
        return;
    }
    let mut used = String::with_capacity(prefix_end + replacement.text.len());
    used.push_str(&text[..prefix_end]);
    used.push_str(replacement.text);
    fragment.set_text(used);
    remeasure_materialized_item(item, font_system);
}

fn apply_leading_line_edge_replacement(
    items: &mut [MeasuredInlineItem],
    replacement: InlineLineEdgeReplacement,
    font_system: &mut FontSystem,
) {
    let Some(item) = items.iter_mut().find(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(fragment) = &mut item.item else {
        return;
    };
    let text = fragment.text();
    if replacement.source_bytes > text.len() || !text.is_char_boundary(replacement.source_bytes) {
        return;
    }
    let mut used = String::with_capacity(replacement.text.len() + text.len());
    used.push_str(replacement.text);
    used.push_str(&text[replacement.source_bytes..]);
    fragment.set_text(used);
    remeasure_materialized_item(item, font_system);
}

/// Append the selected `hyphenate-character` as a paint item distinct from
/// the source fragment that owns the break.  This keeps its style, bidi
/// behavior, and advance visible to normal line materialization rather than
/// disguising it as an edit to a source word.
fn append_discretionary_marker(
    items: &mut Vec<MeasuredInlineItem>,
    effect: DiscretionaryBreakEffect,
    graph_runs: &[InlineParagraphRun],
    font_system: &mut FontSystem,
) {
    let Some(source_index) = items.iter().rposition(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(source_fragment) = &items[source_index].item else {
        return;
    };
    let Some(InlineLineItem::Fragment(marker_owner)) = graph_runs
        .get(effect.marker_owner.style_position.run_index)
        .map(|run| &run.item)
    else {
        return;
    };
    let marker_text = used_discretionary_marker_text(marker_owner);
    let mut marker = marker_owner.clone();
    marker.set_text(marker_text);
    marker.mark_selected_discretionary_marker();
    let source_text = source_fragment.text().to_owned();
    let marker_range = source_text.len()..source_text.len() + marker.text().len();
    let mut logical_text = source_text;
    logical_text.push_str(marker.text());
    let spans = [
        StyledTextSpan {
            text: source_fragment.text(),
            style: source_fragment.style(),
        },
        StyledTextSpan {
            text: marker.text(),
            style: marker.style(),
        },
    ];
    // Shape the generated marker with the selected source edge before it is
    // separated into paint items. Script fallback and Arabic joining are
    // chosen for the complete logical request; `source_slice` then gives each
    // item only its source-owned glyph cluster range.
    // <https://www.w3.org/TR/css-text-3/#boundary-shaping>
    if let Some(shaped) = font_system.shape_untracked_styled_inline_fragments(
        &spans,
        logical_text,
        0.0,
        source_fragment.style().line_height,
        0.0,
        source_fragment.style(),
    ) {
        let source_range = 0..marker_range.start;
        if let Some(source_slice) = shaped.source_slice(source_range) {
            let source = &mut items[source_index];
            source
                .advance
                .replace_base_points(source_slice.advance_width());
            source.shaped = Some(Rc::new(source_slice));
        }
        if let Some(marker_slice) = shaped.source_slice(marker_range) {
            let width = marker_slice.advance_width();
            items.push(MeasuredInlineItem::new(
                InlineLineItem::Fragment(marker),
                width,
                Some(Rc::new(marker_slice)),
            ));
            return;
        }
    }
    let mut materialized_marker =
        MeasuredInlineItem::new(InlineLineItem::Fragment(marker), 0.0, None);
    remeasure_materialized_item(&mut materialized_marker, font_system);
    items.push(materialized_marker);
}

/// Resolve the UA-selected `hyphenate-character` at the selected line edge.
/// Default horizontal text delegates its language-specific choice to the
/// computed `hyphenate-character`; vertical layout uses U+2010, whose vertical
/// form is the interoperable conditional-hyphen presentation.
/// Explicit author strings remain unchanged in every writing mode.
/// <https://drafts.csswg.org/css-text-4/#hyphenate-character>
pub(super) fn used_discretionary_marker_text(fragment: &InlineFragment) -> &str {
    if matches!(
        fragment.style().hyphenate_character,
        crate::css::HyphenateCharacter::Auto
    ) && matches!(
        fragment.style().text_layout_policy(),
        crate::css::TextLayoutPolicy::Vertical(_)
    ) {
        "\u{2010}"
    } else {
        fragment
            .style()
            .hyphenate_character
            .used_text_for_language(fragment.style().language.as_deref())
    }
}

/// Reconcile tabs after graph materialization has rejoined adjacent text runs.
///
/// The opportunity graph intentionally splits text at legal boundaries, but a
/// preserved tab's advance depends on all preceding text in its selected line.
/// Re-shaping one all-text materialized line keeps its fitting and alignment
/// measure consistent with the boundary-shaped paint group. Atomic inline
/// participants contribute to the running logical inline cursor even though
/// they split a shaping group, so a following tab resolves from the same block
/// content edge as it does during paint:
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>.
pub(in crate::layout) fn opportunity_is_soft_wrap(opportunity: InlineBreakOpportunity) -> bool {
    !matches!(
        opportunity.kind,
        InlineBreakKind::Forced | InlineBreakKind::FloatPlacement
    )
}

/// Return the selected Phase II trim advance without deleting source items.
///
/// Regular inline box edges are transparent to CSS Text line-edge processing,
/// but remain in the item sequence for their painting ownership.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
pub(in crate::layout) fn trailing_collapsible_measured_width(items: &[MeasuredInlineItem]) -> f32 {
    let mut trimmed_width = 0.0;
    for item in items.iter().rev() {
        match &item.item {
            // Inline box edges decorate the same inline stream but do not
            // give an otherwise-empty tail textual content. Phase II trims
            // collapsed spaces through nested inline boxes, retaining their
            // borders/padding while removing the space advances on either
            // side of those edges.
            // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
            InlineLineItem::Atom(atom)
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                ) => {}
            InlineLineItem::Fragment(fragment)
                if fragment.style().white_space.collapses_spaces()
                    && fragment.text().chars().all(is_css_collapsible_whitespace) =>
            {
                trimmed_width += item.used_advance().points();
            }
            _ => break,
        }
    }
    trimmed_width
}

pub(in crate::layout) fn trailing_collapsible_run_width(runs: &[InlineParagraphRun]) -> f32 {
    let mut trimmed_width = 0.0;
    for run in runs.iter().rev() {
        match &run.item {
            InlineLineItem::Atom(atom)
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                ) => {}
            InlineLineItem::Fragment(fragment)
                if fragment.style().white_space.collapses_spaces()
                    && fragment.text().chars().all(is_css_collapsible_whitespace) =>
            {
                trimmed_width += run.width;
            }
            _ => break,
        }
    }
    trimmed_width
}

/// Return conditional `pre-wrap` hanging advance at a selected line edge,
/// including a preserved-space run immediately before a trailing sequence of
/// unconditionally hanging Unicode space separators.
///
/// Phase II first identifies the complete visual line-end whitespace sequence:
/// a `pre-wrap` run before U+3000 (or another other-space separator) is still
/// at line end and therefore hangs with it.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
pub(in crate::layout) fn trailing_pre_wrap_hanging_width_with_unconditional_separators<T>(
    items: &[T],
    font_system: &mut FontSystem,
) -> f32
where
    T: AsRef<InlineLineItem>,
{
    let mut width = 0.0;
    for item in items.iter().rev() {
        let InlineLineItem::Fragment(fragment) = item.as_ref() else {
            if matches!(
                item.as_ref(),
                InlineLineItem::Atom(atom)
                    if matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)))
            ) {
                continue;
            }
            break;
        };
        for character in fragment.text().chars().rev() {
            // Collapsed terminal document whitespace is removed before the
            // unconditional other-space-separator rule. Continue through it
            // so a preceding U+3000 (or another other separator) is still
            // recognized as the visual line edge.
            // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
            if fragment.style().white_space.collapses_spaces()
                && is_css_collapsible_whitespace(character)
            {
                continue;
            }
            if fragment.style().white_space == WhiteSpace::PreWrap
                && is_css_preserved_document_space(character)
            {
                width += font_system.measure_text(&character.to_string(), fragment.style());
                continue;
            }
            if character_is_css_other_space_separator(character)
                && fragment
                    .style()
                    .white_space
                    .hangs_trailing_space_separators()
            {
                continue;
            }
            return width;
        }
    }
    width
}

/// Collect the selected source ranges that own Phase II end-edge behavior.
///
/// The width helpers intentionally remain geometry-only, but painting cannot
/// infer source ownership from a scalar advance when spaces cross inline
/// boxes. Record the selected fragment ranges in the same reverse visual-edge
/// traversal used by CSS Text's Phase II whitespace rules.
/// <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
pub(super) fn selected_line_edge_source_effects(
    items: &[MeasuredInlineItem],
    has_collapsed_trim: bool,
    has_pre_wrap_hang: bool,
    has_unconditional_separator_hang: bool,
) -> Rc<[InlineLineEdgeEffect]> {
    let mut effects = Vec::new();

    if has_collapsed_trim {
        for (item_index, item) in items.iter().enumerate().rev() {
            match &item.item {
                InlineLineItem::Atom(atom)
                    if matches!(
                        atom.content(),
                        InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_))
                    ) => {}
                InlineLineItem::Fragment(fragment)
                    if fragment.style().white_space.collapses_spaces()
                        && fragment.text().chars().all(is_css_collapsible_whitespace) =>
                {
                    effects.push(InlineLineEdgeEffect {
                        kind: InlineLineEdgeEffectKind::CollapsedEndTrim,
                        item_index,
                        source_range: 0..fragment.text().len(),
                    });
                }
                _ => break,
            }
        }
    }

    if has_pre_wrap_hang {
        for (item_index, item) in items.iter().enumerate().rev() {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                if matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)))
                ) {
                    continue;
                }
                break;
            };
            let mut start = fragment.text().len();
            let mut saw_pre_wrap_space = false;
            for (offset, character) in fragment.text().char_indices().rev() {
                if fragment.style().white_space.collapses_spaces()
                    && is_css_collapsible_whitespace(character)
                {
                    continue;
                }
                if fragment.style().white_space == WhiteSpace::PreWrap
                    && is_css_preserved_document_space(character)
                {
                    start = offset;
                    saw_pre_wrap_space = true;
                    continue;
                }
                if character_is_css_other_space_separator(character)
                    && fragment
                        .style()
                        .white_space
                        .hangs_trailing_space_separators()
                {
                    continue;
                }
                break;
            }
            if saw_pre_wrap_space {
                effects.push(InlineLineEdgeEffect {
                    kind: InlineLineEdgeEffectKind::PreWrapHang,
                    item_index,
                    source_range: start..fragment.text().len(),
                });
                continue;
            }
            if fragment
                .text()
                .chars()
                .all(character_is_css_other_space_separator)
                && fragment
                    .style()
                    .white_space
                    .hangs_trailing_space_separators()
            {
                continue;
            }
            break;
        }
    }

    if has_unconditional_separator_hang {
        let mut follows_hanging_separator = false;
        for (item_index, item) in items.iter().enumerate().rev() {
            let InlineLineItem::Fragment(fragment) = &item.item else {
                if matches!(
                    &item.item,
                    InlineLineItem::Atom(atom)
                        if matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(_)))
                ) {
                    continue;
                }
                break;
            };
            let mut start = fragment.text().len();
            let mut saw_separator = false;
            for (offset, character) in fragment.text().char_indices().rev() {
                if fragment.style().white_space.collapses_spaces()
                    && is_css_collapsible_whitespace(character)
                    && !follows_hanging_separator
                {
                    continue;
                }
                if character_is_css_other_space_separator(character)
                    && fragment
                        .style()
                        .white_space
                        .hangs_trailing_space_separators()
                {
                    start = offset;
                    saw_separator = true;
                    follows_hanging_separator = true;
                    continue;
                }
                if fragment.style().white_space != WhiteSpace::PreWrap
                    && fragment
                        .style()
                        .white_space
                        .hangs_trailing_space_separators()
                    && follows_hanging_separator
                    && is_css_preserved_document_space(character)
                {
                    start = offset;
                    continue;
                }
                break;
            }
            if saw_separator {
                effects.push(InlineLineEdgeEffect {
                    kind: InlineLineEdgeEffectKind::UnconditionalSeparatorHang,
                    item_index,
                    source_range: start..fragment.text().len(),
                });
                continue;
            }
            if fragment.style().white_space != WhiteSpace::PreWrap
                && fragment
                    .style()
                    .white_space
                    .hangs_trailing_space_separators()
                && follows_hanging_separator
                && fragment.text().chars().all(is_css_preserved_document_space)
            {
                effects.push(InlineLineEdgeEffect {
                    kind: InlineLineEdgeEffectKind::UnconditionalSeparatorHang,
                    item_index,
                    source_range: 0..fragment.text().len(),
                });
                continue;
            }
            break;
        }
    }

    effects.sort_by_key(|effect| effect.item_index);
    Rc::from(effects.into_boxed_slice())
}

pub(in crate::layout) fn normalize_materialized_control_characters(
    items: &mut Vec<MeasuredInlineItem>,
    visible_trailing_soft_hyphen: bool,
    font_system: &mut FontSystem,
) {
    let trailing_soft_hyphen_index = visible_trailing_soft_hyphen
        .then(|| {
            items.iter().rposition(|item| {
            matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
        })
        })
        .flatten();
    let mut index = 0;
    while index < items.len() {
        let mut remove = false;
        if let InlineLineItem::Fragment(fragment) = &mut items[index].item
            && fragment_text_needs_materialized_normalization(
                fragment.text(),
                Some(index) == trailing_soft_hyphen_index,
            )
            && let Some(text) = normalize_materialized_fragment_text(
                fragment.text(),
                Some(index) == trailing_soft_hyphen_index,
                false,
                used_discretionary_marker_text(fragment),
            )
        {
            fragment.set_text(text);
            remove = fragment.text().is_empty();
            if !remove {
                remeasure_materialized_item(&mut items[index], font_system);
            }
        }
        if remove {
            items.remove(index);
        } else {
            index += 1;
        }
    }
}

pub(super) fn materialized_items_have_joining_behavior(items: &[MeasuredInlineItem]) -> bool {
    items.iter().any(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment)
        if fragment.text().chars().any(|character| {
            character_has_cursive_shaping_behavior(character)
                && !character_is_join_control(character)
        }))
    })
}

/// Add the trailing half of a selected joining boundary before the generated
/// marker. The marker remains a separate paint item, but the source-side ZWJ
/// keeps the combined paint shaping request faithful to the selected logical
/// edge sequence.
pub(super) fn append_materialized_line_joiner(
    items: &mut [MeasuredInlineItem],
    font_system: &mut FontSystem,
) {
    let Some(item) = items.iter_mut().rev().find(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(fragment) = &mut item.item else {
        return;
    };
    let mut text = String::with_capacity(fragment.text().len() + '\u{200d}'.len_utf8());
    text.push_str(fragment.text());
    text.push('\u{200d}');
    fragment.set_text(text);
    remeasure_materialized_item(item, font_system);
}

/// Add the leading half of a selected soft-hyphen shaping boundary.
///
/// The source soft hyphen is removed during CSS Text's line-edge processing,
/// so the following physical line otherwise begins without the joining context
/// that the source word had. The control has no advance and is not added at
/// arbitrary visual-order boundaries.
fn prepend_materialized_line_joiner(
    items: &mut [MeasuredInlineItem],
    font_system: &mut FontSystem,
) {
    let Some(item) = items.iter_mut().find(|item| {
        matches!(&item.item, InlineLineItem::Fragment(fragment) if !fragment.text().is_empty())
    }) else {
        return;
    };
    let InlineLineItem::Fragment(fragment) = &mut item.item else {
        return;
    };
    let mut text = String::with_capacity(fragment.text().len() + '\u{200d}'.len_utf8());
    text.push('\u{200d}');
    text.push_str(fragment.text());
    fragment.set_text(text);
    remeasure_materialized_item(item, font_system);
}

pub(in crate::layout) fn fragment_text_needs_materialized_normalization(
    text: &str,
    visible_trailing_soft_hyphen: bool,
) -> bool {
    const SOFT_HYPHEN: char = '\u{00ad}';
    const ZERO_WIDTH_SPACE: char = '\u{200b}';
    text.contains(ZERO_WIDTH_SPACE)
        || text.contains(SOFT_HYPHEN)
        || (visible_trailing_soft_hyphen && text.ends_with(SOFT_HYPHEN))
}
