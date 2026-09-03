use super::*;

/// Shape source runs across transparent inline element edges.
///
/// CSS Text establishes shaping before it selects a line break. Keeping the
/// source-shaped slices on graph runs means a later selected soft-hyphen edge
/// cannot turn an Arabic medial glyph into a final glyph, or lose kerning at a
/// color-only typographic pseudo boundary, merely because an otherwise
/// transparent inline element owns that boundary:
/// <https://www.w3.org/TR/css-text-3/#boundary-shaping>.
pub(super) fn shape_logical_joining_graph_runs(
    runs: &mut [InlineParagraphRun],
    font_system: &mut FontSystem,
    tab_metric_style: &ComputedStyle,
) {
    let mut index = 0;
    while index < runs.len() {
        let InlineLineItem::Fragment(_) = &runs[index].item else {
            index += 1;
            continue;
        };
        let mut fragment_indices = vec![index];
        index += 1;
        while let Some(run) = runs.get(index) {
            match &run.item {
                InlineLineItem::Fragment(right) => {
                    let InlineLineItem::Fragment(left) =
                        &runs[*fragment_indices.last().expect("first graph fragment")].item
                    else {
                        unreachable!("graph fragment indices name fragments");
                    };
                    // A resolved nonzero tracking boundary must not be
                    // reshaped as one run: that could restore an optional
                    // ligature or contextual substitution across the visual
                    // gap. The sole exception is a cursive join, which has no
                    // tracking boundary in the first place and must retain
                    // its shaping context through transparent inline boxes.
                    if graph_fragments_have_nonjoining_tracking_boundary(left, right) {
                        break;
                    }
                    if !can_shape_inline_fragments_together(left, right) {
                        break;
                    }
                    fragment_indices.push(index);
                    index += 1;
                }
                InlineLineItem::Atom(atom) if graph_atom_is_transparent_to_shaping(atom) => {
                    index += 1;
                }
                InlineLineItem::Float(_) => {
                    // Floats alter line-box geometry but not the shaping
                    // context of their neighboring in-flow text.
                    // <https://www.w3.org/TR/css-text-3/#boundary-shaping>
                    index += 1;
                }
                InlineLineItem::Atom(_) => break,
            }
        }
        let needs_source_shape = fragment_indices.len() == 1
            && matches!(
                &runs[fragment_indices[0]].item,
                InlineLineItem::Fragment(fragment) if {
                    let mut opportunities = Vec::new();
                    font_system.collect_cached_measured_break_opportunities(
                        fragment.text(),
                        TextBreakPolicy::from(fragment.style()),
                        &mut opportunities,
                    );
                    opportunities.into_iter().any(|position| {
                        position > 0 && position < fragment.text().len()
                    })
                }
            );
        if fragment_indices.len() < 2 && !needs_source_shape {
            continue;
        }
        if fragment_indices.len() == 1 {
            let fragment_index = fragment_indices[0];
            let Some(source) = runs[fragment_index].shaped.clone() else {
                continue;
            };
            let text_len = match &runs[fragment_index].item {
                InlineLineItem::Fragment(fragment) => fragment.text().len(),
                InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                    unreachable!("graph fragment indices name fragments")
                }
            };
            let Some(selection) = SourceShapedSelection::from_source(source, 0..text_len) else {
                continue;
            };
            let selected = selection.selected_rc();
            let InlineLineItem::Fragment(fragment) = &mut runs[fragment_index].item else {
                unreachable!("graph fragment indices name fragments")
            };
            runs[fragment_index].width = selected.advance_width();
            runs[fragment_index].shaped = Some(selected);
            fragment.set_source_shaped_selection(Some(selection));
            continue;
        }
        let mut spans = Vec::with_capacity(fragment_indices.len());
        let mut span_styles = Vec::with_capacity(fragment_indices.len());
        let mut text = String::new();
        let mut ranges = Vec::with_capacity(fragment_indices.len());
        let mut line_height = None;
        for &fragment_index in &fragment_indices {
            let InlineLineItem::Fragment(fragment) = &runs[fragment_index].item else {
                unreachable!("graph fragment indices name fragments");
            };
            line_height.get_or_insert(fragment.style().line_height);
            let start = text.len();
            text.push_str(fragment.text());
            ranges.push(start..text.len());
            spans.push(StyledTextSpan {
                text: fragment.text(),
                // Keep every eligible CSS span in one canonical shaping
                // stream. The styled shaper retains paint/style ranges
                // independently and adds virtual U+200D only for a genuine
                // glyph-affecting font transition.
                // <https://drafts.csswg.org/css-text-3/#boundary-shaping>
                style: fragment.style(),
            });
            span_styles.push(Rc::clone(&fragment.data.style));
        }
        let Some(shaped) = font_system
            .shape_untracked_styled_inline_fragments_with_style_identities(
                &spans,
                text,
                line_height.expect("graph shaping group has a fragment"),
                tab_metric_style,
                &span_styles,
            )
        else {
            continue;
        };
        let shaped = Rc::new(shaped);
        // Retain the complete canonical source shape for every fragment.
        // Selections, visual ordering, and paint then consume exactly the
        // glyph geometry selected across transparent CSS boundaries instead
        // of independently reshaping paint-only fragments.
        // <https://drafts.csswg.org/css-text-3/#boundary-shaping>
        let boundary_source = Rc::new(BoundaryShapedSource {
            shaped: Rc::clone(&shaped),
        });
        for (&fragment_index, range) in fragment_indices.iter().zip(ranges) {
            let InlineLineItem::Fragment(fragment) = &mut runs[fragment_index].item else {
                unreachable!("graph fragment indices name fragments")
            };
            fragment.set_boundary_shaped_source(Rc::clone(&boundary_source), range.clone());
            let Some(mut selection) =
                SourceShapedSelection::from_source(Rc::clone(&shaped), range.clone())
            else {
                continue;
            };
            let slice = Some(selection.selected().clone());
            let width = slice
                .as_ref()
                .map(ShapedInlineLine::advance_width)
                .unwrap_or(0.0);
            if let Some(slice) = slice.as_ref() {
                selection.replace_selected(slice.clone());
            }
            runs[fragment_index].width = width;
            runs[fragment_index].shaped = slice.map(Rc::new);
            fragment.set_source_shaped_selection(Some(selection));
        }
    }
}

fn graph_fragments_have_nonjoining_tracking_boundary(
    left: &InlineFragment,
    right: &InlineFragment,
) -> bool {
    let (Some(left_scope), Some(right_scope)) = (left.tracking_scope(), right.tracking_scope())
    else {
        return false;
    };
    InlineBoundaryAdvance::between(
        UsedLetterSpacing::new(left_scope.letter_spacing()),
        UsedLetterSpacing::new(right_scope.letter_spacing()),
    )
    .points()
        != 0.0
        && crate::text::inter_character_gap_allowed_between_text(left.text(), right.text())
}

pub(super) fn graph_atom_is_transparent_to_shaping(atom: &InlineAtom) -> bool {
    matches!(atom.content(), InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge))
        if edge.advance == 0.0
            && edge.paint_extent == 0.0
            && !inline_box_edge_fragment_breaks_shaping(atom.style(), *edge)
            && !inline_box_bidi_isolation_breaks_shaping(atom.style()))
}

pub(in crate::layout) fn push_text_graph_runs(
    font_system: &mut FontSystem,
    runs: &mut Vec<InlineParagraphRun>,
    word: &InlineWord,
    text: &str,
    tracking_scope: Rc<InlineTrackingScope>,
) {
    if text.is_empty() {
        return;
    }
    let break_text = if word.style.hyphens == Hyphens::Auto
        && matches!(
            word.style.automatic_discretionary_hyphenation_policy(),
            DiscretionaryHyphenationPolicy::Ordinary
        ) {
        Cow::Borrowed(text)
    } else {
        text_with_hyphenation_controls(text, &word.style)
    };
    let text = break_text.as_ref();
    if text.is_empty() {
        return;
    }
    let source_run = Rc::new(());
    if word.source == InlineTextSource::BidiControl {
        push_text_graph_run_segment(
            font_system,
            runs,
            word,
            text,
            word.hanging_edges,
            tracking_scope,
            source_run,
        );
        return;
    }

    // A nonzero `letter-spacing` value is resolved between CSS Text
    // typographic units, not between source fragments. Keep the fast,
    // source-fragment path for the overwhelmingly common untracked case, but
    // make every tracked unit explicit so a joining word remains one unit
    // while its adjacent space or non-joining content can own a boundary.
    // This also prevents optional ligatures from crossing a nonzero tracking
    // boundary.
    if tracking_scope.letter_spacing().points() == 0.0 {
        push_text_graph_run_segment(
            font_system,
            runs,
            word,
            text,
            word.hanging_edges,
            tracking_scope,
            source_run,
        );
        return;
    }
    for range in CursiveProtectedUnitRanges::new(text) {
        let mut hanging_edges = word.hanging_edges;
        hanging_edges.blocks_start &= range.start == 0;
        hanging_edges.blocks_end &= range.end == text.len();
        push_text_graph_run_segment(
            font_system,
            runs,
            word,
            &text[range],
            hanging_edges,
            Rc::clone(&tracking_scope),
            Rc::clone(&source_run),
        );
    }
}

pub(in crate::layout) fn push_text_graph_run_segment(
    font_system: &mut FontSystem,
    runs: &mut Vec<InlineParagraphRun>,
    word: &InlineWord,
    text: &str,
    hanging_edges: InlineHangingEdges,
    tracking_scope: Rc<InlineTrackingScope>,
    source_run: Rc<()>,
) {
    if text.is_empty() {
        return;
    }
    let mut fragment = InlineFragment::new_shared_style(
        text,
        Rc::clone(&word.style),
        word.baseline_shift,
        word.link_target.clone(),
        word.mergeable,
        word.source,
        false,
        hanging_edges,
        Rc::clone(&word.ancestor_inline_decorations),
    )
    .with_visual_offset(word.visual_offset)
    .with_excluded_positioning_geometry_source(word.excluded_positioning_geometry_source)
    .with_source_run(source_run)
    .with_tracking_scope(tracking_scope);
    if word.style.content.is_generated() && word.style.display.is_flex() {
        // Generated content can use a block-level principal display (for
        // example, flex) while its fallback text remains in the enclosing
        // inline stream. Its principal decoration belongs to that generated
        // line fragment and must not be rejected as non-inline paint.
        // <https://drafts.csswg.org/css-content-3/#content-property>
        fragment.set_force_inline_background_paint(true);
    }
    // CSS-generated bidi controls are UAX #9 input only. Their fallback
    // glyph records must not contribute an inline advance or line metrics;
    // they remain as source fragments so visual ordering can still consume
    // the controls after line selection.
    // <https://www.w3.org/TR/css-writing-modes-4/#unicode-bidi>
    let shaped = (word.source != InlineTextSource::BidiControl)
        .then(|| {
            font_system.shape_untracked_inline_line_with_style_identity(
                text,
                &word.style,
                word.style.line_height,
            )
        })
        .flatten();
    let width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    let shaped = shaped.map(Rc::new);
    runs.push(InlineParagraphRun {
        item: InlineLineItem::Fragment(fragment),
        width,
        shaped,
    });
}

pub(super) fn run_tracking_scope<'a>(
    run: &'a InlineParagraphRun,
    fallback: &ComputedStyle,
) -> &'a Rc<InlineTrackingScope> {
    match &run.item {
        InlineLineItem::Fragment(fragment) => fragment
            .tracking_scope()
            .expect("graph fragments retain inline tracking scope"),
        InlineLineItem::Atom(atom) => atom
            .tracking_scope()
            .expect("graph atoms retain inline tracking scope"),
        InlineLineItem::Float(_) => {
            let _ = fallback;
            unreachable!("first-letter graph fragments do not inherit float scope")
        }
    }
}

pub(in crate::layout) fn inline_run_has_nonzero_tracking(run: &InlineParagraphRun) -> bool {
    match &run.item {
        InlineLineItem::Fragment(fragment) => fragment
            .tracking_scope()
            .is_some_and(|scope| scope.letter_spacing().points() != 0.0),
        InlineLineItem::Atom(atom) => atom
            .tracking_scope()
            .is_some_and(|scope| scope.letter_spacing().points() != 0.0),
        InlineLineItem::Float(_) => false,
    }
}
