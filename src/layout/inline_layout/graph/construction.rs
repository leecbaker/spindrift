use super::*;

pub(in crate::layout) fn build_inline_opportunity_graph<I>(
    font_system: &mut FontSystem,
    items: I,
    block_style: &ComputedStyle,
) -> InlineOpportunityGraph
where
    I: IntoIterator,
    I::Item: AsRef<InlineItem>,
{
    let mut runs = Vec::new();
    let mut transform_state = TextTransformState::default();
    let root_tracking_scope = InlineTrackingScope::root(block_style);
    let mut tracking_scopes = vec![root_tracking_scope];
    for item in items {
        match item.as_ref() {
            InlineItem::Word(word) => {
                let text = transform_text_with_state(&word.text, &word.style, &mut transform_state);
                let text = synthesize_missing_font_caps_text(font_system, &text, &word.style);
                push_text_graph_runs(
                    font_system,
                    &mut runs,
                    word,
                    &text,
                    Rc::clone(
                        tracking_scopes
                            .last()
                            .expect("tracking scope stack is rooted"),
                    ),
                );
            }
            InlineItem::Atom(atom) => {
                // Inline box edges are transparent to CSS `capitalize` word
                // boundaries; only replaced/atomic inline content separates
                // adjacent text words.
                if !matches!(atom.content(), InlineAtomContent::InlineEdge(_)) {
                    transform_state.force_word_boundary();
                }
                let scope = Rc::clone(
                    tracking_scopes
                        .last()
                        .expect("tracking scope stack is rooted"),
                );
                let atom = (**atom).clone().with_tracking_scope(Rc::clone(&scope));
                runs.push(InlineParagraphRun {
                    item: InlineLineItem::Atom(atom.clone()),
                    width: inline_atom_logical_inline_size(&atom, block_style),
                    shaped: None,
                });
                if let InlineAtomContent::InlineEdge(InlineEdgeRole::BoxEdge(edge)) = atom.content()
                {
                    match edge.logical_edge {
                        InlineLogicalEdge::Start => {
                            tracking_scopes.push(InlineTrackingScope::child(scope, atom.style()))
                        }
                        InlineLogicalEdge::End => {
                            if tracking_scopes.len() > 1 {
                                tracking_scopes.pop();
                            }
                        }
                    }
                }
            }
            InlineItem::StaticPositionSourceMarker(_) => {
                // Source markers are structural ownership tokens. They are
                // absent from CSS Text adjacency and line geometry; a
                // hypothetical replay replaces its selected marker with an
                // ordinary atomic inline before graph construction.
            }
            InlineItem::Float(float) => {
                // Floats participate in source-order placement but are out
                // of flow for CSS Text. They therefore cannot split the word
                // context used by `text-transform: capitalize`.
                // <https://www.w3.org/TR/css-text-3/#text-transform-property>
                runs.push(InlineParagraphRun {
                    item: InlineLineItem::Float((**float).clone()),
                    width: 0.0,
                    shaped: None,
                });
            }
            InlineItem::Break(_) | InlineItem::PageScopeStart(_) | InlineItem::PageScopeEnd => {
                transform_state.force_word_boundary();
            }
        }
    }
    resolve_text_autospace_owner_styles(&mut runs, font_system);
    coalesce_trailing_tracking_controls(&mut runs, font_system);
    let automatic_discretionary_breaks =
        apply_auto_hyphenation_across_transparent_inline_edges(&runs);
    let manual_discretionary_effects =
        manual_hyphenation_effects_across_transparent_inline_edges(&runs);
    shape_logical_joining_graph_runs(&mut runs, font_system, block_style);
    let mut opportunities =
        inline_break_opportunities_for_runs_with_font_system(&runs, block_style, Some(font_system));
    merge_automatic_discretionary_breaks(&mut opportunities, automatic_discretionary_breaks);
    merge_manual_discretionary_effects(&mut opportunities, manual_discretionary_effects);
    InlineOpportunityGraph::new(runs, opportunities)
}

/// Resolve each source autospace marker against the innermost lexical inline
/// scope shared by its adjacent typographic units.
///
/// Inline collection must create the marker before the graph has assigned
/// immutable tracking scopes. Once the graph has done so, this pass is the
/// single point where CSS Text's boundary owner selects both the applicable
/// property value and the `1/8ic` used advance.
/// <https://drafts.csswg.org/css-text-4/#text-autospace-property>
fn resolve_text_autospace_owner_styles(
    runs: &mut Vec<InlineParagraphRun>,
    font_system: &mut FontSystem,
) {
    let mut index = 0;
    while index < runs.len() {
        if !matches!(
            runs[index].item,
            InlineLineItem::Atom(ref atom)
                if matches!(
                    atom.content(),
                    InlineAtomContent::InlineEdge(InlineEdgeRole::TextAutospace(_))
                )
        ) {
            index += 1;
            continue;
        }
        let Some((left, right)) = autospace_adjacent_fragments(runs, index) else {
            index += 1;
            continue;
        };
        let (Some(left_scope), Some(right_scope)) = (left.tracking_scope(), right.tracking_scope())
        else {
            index += 1;
            continue;
        };
        let owner_style =
            InlineTrackingScope::common_autospace_style(left_scope.as_ref(), right_scope.as_ref())
                .clone();
        let (Some(first), Some(second)) = (
            autospace_boundary_character_at_end(left.text()),
            autospace_boundary_character_at_start(right.text()),
        ) else {
            runs.remove(index);
            continue;
        };
        if !text_autospace_boundary_needs_spacing(
            &owner_style.text_autospace,
            first,
            left.style(),
            second,
            right.style(),
        ) {
            runs.remove(index);
            continue;
        }
        let advance = font_system.ic_advance_for_style(&owner_style) / 8.0;
        let InlineLineItem::Atom(atom) = &mut runs[index].item else {
            unreachable!("autospace marker is an inline atom");
        };
        atom.set_text_autospace_advance(&owner_style, advance);
        runs[index].width = advance.points();
        index += 1;
    }
}

/// Return direct textual neighbors of an already collected autospace marker.
/// Box-edge atoms are lexical scope markers, while any other atom or float
/// would have prevented collection from inserting the autospace marker.
fn autospace_adjacent_fragments(
    runs: &[InlineParagraphRun],
    index: usize,
) -> Option<(&InlineFragment, &InlineFragment)> {
    let mut left = None;
    for run in runs[..index].iter().rev() {
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                left = Some(fragment);
                break;
            }
            InlineLineItem::Atom(atom)
                if matches!(atom.content(), InlineAtomContent::InlineEdge(_)) => {}
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    let mut right = None;
    for run in &runs[index + 1..] {
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                right = Some(fragment);
                break;
            }
            InlineLineItem::Atom(atom)
                if matches!(atom.content(), InlineAtomContent::InlineEdge(_)) => {}
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => return None,
        }
    }
    left.zip(right)
}

/// Attach a separately collected trailing control run to its preceding text.
///
/// HTML comments and inline DOM boundaries can split a sequence such as
/// `x U+200D` into distinct collector words. CSS Text treats the control as
/// part of the adjacent typographic unit: it must not own a gap itself, while
/// the boundary from that preceding `x` to the following visible character
/// remains eligible for tracking. Coalescing before graph shaping also keeps
/// fitting, intrinsic sizing, and paint on the same source representation.
/// <https://www.w3.org/TR/css-text-3/#letter-spacing>
fn coalesce_trailing_tracking_controls(
    runs: &mut Vec<InlineParagraphRun>,
    font_system: &mut FontSystem,
) {
    let mut index = 1;
    while index < runs.len() {
        let control_text = match &runs[index].item {
            InlineLineItem::Fragment(fragment)
                if crate::text::text_is_inter_character_control_only(fragment.text()) =>
            {
                Some(fragment.text().to_string())
            }
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                None
            }
        };
        let Some(control_text) = control_text else {
            index += 1;
            continue;
        };
        let can_attach = matches!(
            (&runs[index - 1].item, &runs[index].item),
            (InlineLineItem::Fragment(previous), InlineLineItem::Fragment(control))
                if previous.tracking_scope().zip(control.tracking_scope())
                    .is_some_and(|(left, right)| Rc::ptr_eq(left, right))
        );
        if !can_attach {
            index += 1;
            continue;
        }
        let InlineLineItem::Fragment(previous) = &mut runs[index - 1].item else {
            unreachable!("control run can only attach to preceding text")
        };
        let mut text = String::with_capacity(previous.text().len() + control_text.len());
        text.push_str(previous.text());
        text.push_str(&control_text);
        previous.set_text(text);
        let shaped = font_system.shape_untracked_inline_line_with_style_identity(
            previous.text(),
            &previous.data.style,
            previous.style().line_height,
        );
        let width = shaped
            .as_ref()
            .map(ShapedInlineLine::advance_width)
            .unwrap_or(0.0);
        runs[index - 1].width = width;
        runs[index - 1].shaped = shaped.map(Rc::new);
        runs.remove(index);
    }
}

/// Apply dictionary hyphenation after joining source fragments that CSS Text
/// treats as one word.
///
/// An ordinary inline element is transparent to word formation: `high<span>
/// way</span>` must be offered to the language dictionary as `highway`, while
/// its resulting soft hyphen remains owned by the source fragment before the
/// selected break. Atomic boxes, used inline-axis decoration, bidi isolation,
/// and differing hyphenation policies terminate the word.
/// <https://www.w3.org/TR/css-text-3/#hyphenation>
pub(super) fn apply_auto_hyphenation_across_transparent_inline_edges(
    runs: &[InlineParagraphRun],
) -> Vec<InlineBreakOpportunity> {
    let mut automatic_breaks = Vec::new();
    let mut index = 0;
    while index < runs.len() {
        let Some((fragment_indices, next_index)) = hyphenation_fragment_group(runs, index) else {
            index += 1;
            continue;
        };
        index = next_index;
        let InlineLineItem::Fragment(first) = &runs[fragment_indices[0]].item else {
            unreachable!("hyphenation group has a first fragment");
        };
        if first.style().hyphens != Hyphens::Auto
            || matches!(
                first.style().automatic_discretionary_hyphenation_policy(),
                DiscretionaryHyphenationPolicy::Disabled
            )
            || matches!(first.style().line_break, css::LineBreak::Anywhere)
        {
            continue;
        }
        let Some(language) = first.style().language.as_deref() else {
            continue;
        };
        let hyphenator = hyphenator_for_language(language);
        let (source, source_ends) = hyphenation_source_for_fragments(runs, &fragment_indices);
        let opportunities = automatic_hyphenation_opportunities(
            &source,
            hyphenator.as_deref(),
            first.style().hyphenate_limit_chars,
            language,
        );
        automatic_breaks.extend(opportunities.into_iter().filter_map(|opportunity| {
            automatic_opportunity_for_source_offset(
                opportunity,
                &fragment_indices,
                &source_ends,
                runs,
            )
        }));
    }
    automatic_breaks
}

/// Attach language-resource spelling changes to authored U+00AD boundaries.
///
/// The source word is gathered through transparent inline edges before the
/// text-layer resolver removes U+00AD for its lookup. The resolver then maps
/// a matching rule back to the authored source boundary, allowing selected
/// line materialization to use the same discretionary effect as `auto`.
/// <https://www.w3.org/TR/css-text-3/#hyphenation>
fn manual_hyphenation_effects_across_transparent_inline_edges(
    runs: &[InlineParagraphRun],
) -> Vec<(InlineGraphPosition, DiscretionaryBreakEffect)> {
    let mut effects = Vec::new();
    let mut index = 0;
    while index < runs.len() {
        let Some((fragment_indices, next_index)) = hyphenation_fragment_group(runs, index) else {
            index += 1;
            continue;
        };
        index = next_index;
        let InlineLineItem::Fragment(first) = &runs[fragment_indices[0]].item else {
            unreachable!("hyphenation group has a first fragment");
        };
        if matches!(
            first.style().authored_discretionary_hyphenation_policy(),
            DiscretionaryHyphenationPolicy::Disabled
        ) {
            continue;
        }
        let Some(language) = first.style().language.as_deref() else {
            continue;
        };
        let (source, source_ends) = hyphenation_source_for_fragments(runs, &fragment_indices);
        for opportunity in manual_hyphenation_opportunities(&source, language) {
            let position = source_position_for_offset(
                opportunity.byte_offset,
                &fragment_indices,
                &source_ends,
                runs,
            );
            let InlineLineItem::Fragment(fragment) = &runs[position.run_index].item else {
                unreachable!("manual hyphen source position names a text fragment");
            };
            if !fragment.style().allows_soft_wrap() {
                continue;
            }
            effects.push((
                position,
                DiscretionaryBreakEffect {
                    source_boundary: position,
                    marker_owner: DiscretionaryMarkerOwner {
                        style_position: position,
                    },
                    left_replacement: language_replacement_to_line_edge(opportunity.left),
                    right_replacement: language_replacement_to_line_edge(opportunity.right),
                    leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
                },
            ));
        }
    }
    effects
}

fn hyphenation_fragment_group(
    runs: &[InlineParagraphRun],
    start: usize,
) -> Option<(Vec<usize>, usize)> {
    let InlineLineItem::Fragment(first) = &runs.get(start)?.item else {
        return None;
    };
    if first.style().hyphens == Hyphens::None {
        return None;
    }
    let mut fragment_indices = vec![start];
    let mut index = start + 1;
    while let Some(run) = runs.get(index) {
        match &run.item {
            InlineLineItem::Fragment(next)
                if graph_fragments_share_hyphenation_policy(
                    match &runs[*fragment_indices.last().expect("first hyphenation fragment")].item
                    {
                        InlineLineItem::Fragment(fragment) => fragment,
                        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                            unreachable!("hyphenation fragment index names a fragment")
                        }
                    },
                    next,
                ) =>
            {
                fragment_indices.push(index);
                index += 1;
            }
            InlineLineItem::Atom(atom) if graph_atom_is_transparent_to_shaping(atom) => {
                index += 1;
            }
            InlineLineItem::Float(_) => {
                // An out-of-flow float is transparent to the in-flow word
                // that may continue on either side of its source marker.
                // <https://www.w3.org/TR/css-text-3/#line-break-details>
                index += 1;
            }
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) => {
                break;
            }
        }
    }
    Some((fragment_indices, index))
}

fn hyphenation_source_for_fragments(
    runs: &[InlineParagraphRun],
    fragment_indices: &[usize],
) -> (String, Vec<usize>) {
    let mut source = String::new();
    let mut source_ends = Vec::with_capacity(fragment_indices.len());
    for &fragment_index in fragment_indices {
        let InlineLineItem::Fragment(fragment) = &runs[fragment_index].item else {
            unreachable!("hyphenation source index names a text fragment");
        };
        source.push_str(fragment.text());
        source_ends.push(source.len());
    }
    (source, source_ends)
}

fn automatic_opportunity_for_source_offset(
    opportunity: DiscretionaryOpportunity,
    fragment_indices: &[usize],
    source_ends: &[usize],
    runs: &[InlineParagraphRun],
) -> Option<InlineBreakOpportunity> {
    let position =
        source_position_for_offset(opportunity.byte_offset, fragment_indices, source_ends, runs);
    let availability = match &runs[position.run_index].item {
        InlineLineItem::Fragment(fragment) => discretionary_hyphenation_availability(
            fragment
                .style()
                .automatic_discretionary_hyphenation_policy(),
        )?,
        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
            unreachable!("hyphenation source position is text")
        }
    };
    Some(InlineBreakOpportunity {
        position,
        kind: BreakEffect::Hyphenation,
        availability,
        whitespace_edge: SelectedWhitespaceEdge::None,
        discretionary: Some(DiscretionaryBreakEffect {
            source_boundary: position,
            marker_owner: DiscretionaryMarkerOwner {
                style_position: position,
            },
            left_replacement: language_replacement_to_line_edge(opportunity.left),
            right_replacement: language_replacement_to_line_edge(opportunity.right),
            leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
        }),
    })
}

fn source_position_for_offset(
    source_byte_offset: usize,
    fragment_indices: &[usize],
    source_ends: &[usize],
    runs: &[InlineParagraphRun],
) -> InlineGraphPosition {
    let fragment_offset = source_ends
        .iter()
        .position(|&end| source_byte_offset <= end)
        .unwrap_or_else(|| source_ends.len().saturating_sub(1));
    let previous_end = fragment_offset
        .checked_sub(1)
        .and_then(|index| source_ends.get(index))
        .copied()
        .unwrap_or(0);
    let run_index = fragment_indices[fragment_offset];
    let byte_offset = source_byte_offset - previous_end;
    let fragment_text_len = match &runs[run_index].item {
        InlineLineItem::Fragment(fragment) => fragment.text().len(),
        InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
            unreachable!("hyphenation source position is text")
        }
    };
    debug_assert!(byte_offset <= fragment_text_len);
    InlineGraphPosition {
        run_index,
        byte_offset,
    }
}

fn language_replacement_to_line_edge(
    replacement: Option<LanguageDiscretionaryReplacement>,
) -> Option<InlineLineEdgeReplacement> {
    replacement.map(|replacement| InlineLineEdgeReplacement {
        source_bytes: replacement.source_bytes,
        text: replacement.replacement,
    })
}

fn merge_automatic_discretionary_breaks(
    opportunities: &mut Vec<InlineBreakOpportunity>,
    automatic_breaks: Vec<InlineBreakOpportunity>,
) {
    for automatic in automatic_breaks {
        // A language resource's explicit discretionary behavior owns this
        // source edge.  Remove any generic UAX candidate at the same edge so
        // selection cannot silently choose a marker-less interpretation.
        opportunities.retain(|opportunity| opportunity.position != automatic.position);
        opportunities.push(automatic);
    }
    opportunities.sort_by_key(|opportunity| {
        (
            opportunity.position,
            opportunity.availability.fitting_stage(),
        )
    });
}

fn merge_manual_discretionary_effects(
    opportunities: &mut [InlineBreakOpportunity],
    effects: Vec<(InlineGraphPosition, DiscretionaryBreakEffect)>,
) {
    for (position, effect) in effects {
        let Some(opportunity) = opportunities
            .iter_mut()
            .find(|opportunity| opportunity.position == position && opportunity.is_discretionary())
        else {
            continue;
        };
        opportunity.discretionary = Some(effect);
    }
}

fn graph_fragments_share_hyphenation_policy(left: &InlineFragment, right: &InlineFragment) -> bool {
    left.style().hyphens == right.style().hyphens
        && left.style().hyphens != Hyphens::None
        && left.style().language == right.style().language
        && (left.style().hyphens != Hyphens::Auto
            || left.style().hyphenate_limit_chars == right.style().hyphenate_limit_chars)
}
