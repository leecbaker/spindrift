use super::*;

/// Float visibility captured with a selected inline line.
///
/// Selection and replay are separate transactions.  Re-querying a mutable
/// float stack is correct only for a normal containing-block replay; an
/// inline-float or initial-letter transaction that already chose its band
/// carries that fact explicitly.  This prevents a later replay from applying
/// the same exclusion once while selecting and again while painting.
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>
/// <https://drafts.csswg.org/css-inline-3/#line-boxes>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum InlineFloatReplay {
    /// Resolve the current containing block's float band at the replay site.
    RequeryContainingBlock { selected_float_page_index: usize },
    /// Reuse selection geometry on its source fragmentainer; resolve a new
    /// band only after fragmentation moves the line.
    FrozenSelectedBand { selected_float_page_index: usize },
}

impl InlineFloatReplay {
    pub(in crate::layout) fn selected_float_page_index(self) -> usize {
        match self {
            Self::RequeryContainingBlock {
                selected_float_page_index,
            }
            | Self::FrozenSelectedBand {
                selected_float_page_index,
            } => selected_float_page_index,
        }
    }

    pub(in crate::layout) fn reuses_selected_band_on(self, page_index: usize) -> bool {
        matches!(self, Self::FrozenSelectedBand { .. })
            && self.selected_float_page_index() == page_index
    }

    pub(in crate::layout) fn freeze_selected_band(self) -> Self {
        Self::FrozenSelectedBand {
            selected_float_page_index: self.selected_float_page_index(),
        }
    }

    pub(super) fn requery_containing_block(self) -> Self {
        Self::RequeryContainingBlock {
            selected_float_page_index: self.selected_float_page_index(),
        }
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Build the inline opportunity graph for one mixed inline paragraph.
    ///
    /// Text transform is applied exactly once while normalizing `InlineItem`s
    /// into graph runs. Unicode break opportunities come from the existing
    /// ICU/Parley-backed text helpers; Spindrift records CSS policy metadata
    /// on the resulting boundaries so later line selection does not repeat
    /// whitespace, hyphenation, and atomic-inline decisions:
    /// <https://www.w3.org/TR/css-text-3/#text-transform-property>,
    /// <https://www.w3.org/TR/css-text-3/#line-breaking>, and
    /// <https://www.w3.org/TR/css-inline-3/#atomic-inline>.
    pub(in crate::layout) fn build_inline_opportunity_graph<I>(
        &mut self,
        items: I,
        block_style: &ComputedStyle,
    ) -> InlineOpportunityGraph
    where
        I: IntoIterator,
        I::Item: AsRef<InlineItem>,
    {
        #[cfg(feature = "layout-profile")]
        let _profile_scope = crate::layout::layout_profile::inline_opportunity_graph_build_scope();
        build_inline_opportunity_graph(&mut self.font_system, items, block_style)
    }

    /// Return a graph whose first typographic letter has its `::first-letter`
    /// style applied before line fitting.
    ///
    /// CSS Inline's `initial-letter` changes the shaped advance and exclusion
    /// geometry of the first letter, so it must be materialized before the
    /// line-break graph selects the first line:
    /// <https://drafts.csswg.org/css-inline-3/#initial-letter-property> and
    /// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>.
    pub(in crate::layout) fn graph_with_first_letter_pseudo(
        &mut self,
        graph: &InlineOpportunityGraph,
        block_style: &ComputedStyle,
    ) -> InlineOpportunityGraph {
        let Some(first_letter_style) = block_style.first_letter_style.as_deref() else {
            return graph.clone();
        };
        let selection = first_letter_stream_selection(graph);
        if selection.is_empty() {
            return graph.clone();
        }
        let used_style = used_first_letter_style_for_graph(
            first_letter_style,
            block_style,
            &mut self.font_system,
        );
        let pseudo_group_id = FirstLetterPseudoGroupId::allocate();
        let paint_scope_id = (used_style.opacity.value() < 1.0).then(InlinePaintScopeId::allocate);
        let mut runs = Vec::with_capacity(graph.runs.len() + selection.len() + 2);
        let mut selection_index = 0;
        let mut marked_leading_whitespace = false;
        for (run_index, run) in graph.runs.iter().enumerate() {
            let InlineLineItem::Fragment(fragment) = &run.item else {
                runs.push(run.clone());
                continue;
            };
            let selection_start = selection_index;
            while selection
                .get(selection_index)
                .is_some_and(|slice| slice.run_index == run_index)
            {
                selection_index += 1;
            }
            let selected_slices = &selection[selection_start..selection_index];
            if selected_slices.is_empty() {
                runs.push(run.clone());
                continue;
            }
            if !marked_leading_whitespace {
                mark_leading_preserved_whitespace_as_first_letter_pseudo(
                    &mut runs,
                    first_letter_style,
                    pseudo_group_id,
                );
                marked_leading_whitespace = true;
            }
            for fragment in split_fragment_for_first_letter_stream_selection(
                fragment,
                selected_slices,
                first_letter_style,
                &used_style,
                block_style,
                paint_scope_id,
                pseudo_group_id,
            ) {
                runs.push(measured_fragment_run(
                    fragment,
                    Rc::clone(run_tracking_scope(run, block_style)),
                    &mut self.font_system,
                ));
            }
        }
        if used_style.float != Float::None {
            if matches!(block_style.position, Position::Absolute | Position::Fixed) {
                // The out-of-flow collector traverses the originating
                // positioned source before its final containing block exists.
                // Its graph must not retain the anonymous float marker (or
                // selected text) at that provisional root position. The
                // positioned flow surrogate rebuilds this graph with its
                // final used `position: static` style and owns the one real
                // float transaction.
                remove_positioned_first_letter_float_source(&mut runs, pseudo_group_id);
            } else {
                materialize_first_letter_float(&mut runs, pseudo_group_id, &used_style);
            }
        }
        // `::first-letter` splits an already shaped source fragment. Its
        // pseudo-element boundary is transparent to cursive shaping unless
        // the used style introduces a real shaping boundary, so restore
        // logical source shaping before deriving line-break opportunities.
        // <https://www.w3.org/TR/css-text-3/#boundary-shaping> and
        // <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>.
        shape_logical_joining_graph_runs(&mut runs, &mut self.font_system, block_style);
        let opportunities = inline_break_opportunities_for_runs_with_font_system(
            &runs,
            block_style,
            Some(&mut self.font_system),
        );
        InlineOpportunityGraph::new(runs, opportunities)
    }
}

/// Discard a floated first-letter group from an out-of-flow source traversal.
///
/// Absolutely and fixed-positioned elements are collected before their final
/// containing block is known. Their eventual positioned flow surrogate builds
/// the same inline graph at the resolved coordinates, so keeping either the
/// selected text or its synthetic marker here would duplicate the pseudo at
/// the provisional root origin.
/// <https://drafts.csswg.org/css-position-3/#abspos-layout>
fn remove_positioned_first_letter_float_source(
    runs: &mut Vec<InlineParagraphRun>,
    group_id: FirstLetterPseudoGroupId,
) {
    runs.retain(|run| {
        !matches!(
            &run.item,
            InlineLineItem::Fragment(fragment)
                if fragment.first_letter_pseudo_group_id() == Some(group_id)
        )
    });
}

/// Apply `::first-letter` paint ownership to preserved whitespace that the
/// inline collector emitted as runs before the typographic initial.
///
/// Tokenization may split a leading tab from its following letter before the
/// first-letter graph pass runs. The pseudo nevertheless owns that whitespace
/// for paint, while `initial-letter` sizing remains attached to the letter.
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>
fn mark_leading_preserved_whitespace_as_first_letter_pseudo(
    runs: &mut [InlineParagraphRun],
    first_letter_style: &ComputedStyle,
    pseudo_group_id: FirstLetterPseudoGroupId,
) {
    for run in runs.iter_mut().rev() {
        let InlineLineItem::Fragment(fragment) = &mut run.item else {
            break;
        };
        if fragment.text().is_empty()
            || !fragment.text().chars().all(char::is_whitespace)
            || fragment.style().white_space.collapses_spaces()
        {
            break;
        }
        apply_first_letter_style_to_leading_preserved_whitespace(fragment, first_letter_style);
        fragment.set_first_letter_pseudo_group_id(pseudo_group_id);
    }
}

/// Replace one stream-selected first-letter group with a source-order CSS
/// float marker.
///
/// CSS 2 treats a floated first letter like a floated element.  Keep the
/// text fragments as payload rather than fabricating a DOM node: the selected
/// group may contain generated punctuation or cross transparent inline edges.
/// <https://www.w3.org/TR/CSS21/selector.html#first-letter>
pub(super) fn materialize_first_letter_float(
    runs: &mut Vec<InlineParagraphRun>,
    group_id: FirstLetterPseudoGroupId,
    used_style: &ComputedStyle,
) {
    let mut fragments = Vec::new();
    for run in runs.iter() {
        let InlineLineItem::Fragment(fragment) = &run.item else {
            continue;
        };
        if fragment.first_letter_pseudo_group_id() == Some(group_id) {
            let mut fragment = fragment.clone();
            // The anonymous float wrapper owns the out-of-flow behavior and
            // the pseudo can no longer be an in-flow initial letter.
            fragment.style_mut().float = Float::None;
            fragment.style_mut().initial_letter = css::InitialLetter::Normal;
            fragments.push(fragment);
        }
    }
    if fragments.is_empty() {
        return;
    }
    let mut float_style = used_style.clone();
    float_style.initial_letter = css::InitialLetter::Normal;
    let float = InlineFloat::first_letter(fragments, group_id, float_style);
    let mut materialized = Vec::with_capacity(runs.len() + 1);
    let mut inserted = false;
    for run in std::mem::take(runs) {
        let selected = matches!(
            &run.item,
            InlineLineItem::Fragment(fragment)
                if fragment.first_letter_pseudo_group_id() == Some(group_id)
        );
        if selected {
            if !inserted {
                materialized.push(InlineParagraphRun {
                    item: InlineLineItem::Float(float.clone()),
                    ..run.clone()
                });
                inserted = true;
            }
        } else {
            materialized.push(run);
        }
    }
    *runs = materialized;
}

fn apply_first_letter_style_to_leading_preserved_whitespace(
    fragment: &mut InlineFragment,
    first_letter_style: &ComputedStyle,
) {
    let source_style = fragment.style().clone();
    let mut prefix_style = first_letter_style.clone();
    prefix_style.font_size = source_style.font_size;
    prefix_style.line_height = source_style.line_height;
    prefix_style.white_space = source_style.white_space;
    prefix_style.tab_size = source_style.tab_size;
    prefix_style.initial_letter = css::InitialLetter::Normal;
    *fragment.style_mut() = prefix_style;
    fragment
        .set_first_letter_pseudo_role(FirstLetterPseudoFragmentRole::LeadingPreservedWhitespace);
    fragment.set_mergeable(false);
}

/// One source range selected as part of the originating block's first-letter
/// text, kept separate because the graph preserves lexical inline fragments.
#[derive(Debug, Clone)]
pub(super) struct FirstLetterStreamSlice {
    pub(super) run_index: usize,
    pub(super) range: std::ops::Range<usize>,
    pub(super) role: FirstLetterPseudoFragmentRole,
}

/// Select first-letter text across source-order graph fragments.
///
/// Marker and CSS-generated bidi-control text do not create first-letter
/// content. Inline box edges are transparent, whereas another in-flow atomic
/// participant prevents a later text run from becoming the first letter.
/// <https://www.w3.org/TR/css-pseudo-4/#first-letter-pseudo>
pub(super) fn first_letter_stream_selection(
    graph: &InlineOpportunityGraph,
) -> Vec<FirstLetterStreamSlice> {
    let mut prefix = Vec::new();
    let mut prefix_origin = None;
    let mut selection = Vec::new();
    let mut pending_suffix_space = Vec::new();
    let mut selected_base = false;

    for (run_index, run) in graph.runs.iter().enumerate() {
        match &run.item {
            InlineLineItem::Fragment(fragment)
                if matches!(
                    fragment.source(),
                    InlineTextSource::BidiControl | InlineTextSource::Marker
                ) =>
            {
                continue;
            }
            InlineLineItem::Fragment(fragment) => {
                // A run split from one lexical source remains continuous for
                // first-letter selection, even when inline box edges occur
                // between its pieces. Generated content and a nested inline's
                // independent text establish their own first-letter scope;
                // an opening quote in either therefore remains the selected
                // pseudo content instead of attaching to later author text.
                if !prefix.is_empty() {
                    let origin = prefix_origin.expect("a prefix has an origin");
                    if !first_letter_prefix_continues_into(origin, fragment) {
                        return prefix;
                    }
                }
                for range in CursiveProtectedUnitRanges::new(fragment.text()) {
                    let Some(base) = first_letter_unit_base(&fragment.text()[range.clone()]) else {
                        continue;
                    };
                    if !selected_base {
                        if character_is_first_letter_associated_space(base) && !prefix.is_empty() {
                            push_first_letter_stream_slice(
                                &mut prefix,
                                run_index,
                                range,
                                FirstLetterPseudoFragmentRole::AssociatedPrefix,
                            );
                        } else if base.is_whitespace() && prefix.is_empty() {
                            // Preserved leading whitespace is styled through
                            // the existing pseudo-prefix path once a base is
                            // found.
                        } else if character_is_unicode_punctuation(base) {
                            if prefix.is_empty() {
                                prefix_origin = Some(fragment);
                            }
                            push_first_letter_stream_slice(
                                &mut prefix,
                                run_index,
                                range,
                                FirstLetterPseudoFragmentRole::AssociatedPrefix,
                            );
                        } else if character_is_unicode_first_letter_base(base) {
                            selection.append(&mut prefix);
                            push_first_letter_stream_slice(
                                &mut selection,
                                run_index,
                                range,
                                FirstLetterPseudoFragmentRole::TypographicInitial,
                            );
                            selected_base = true;
                        } else if !prefix.is_empty() {
                            return Vec::new();
                        }
                    } else if character_is_first_letter_associated_space(base) {
                        push_first_letter_stream_slice(
                            &mut pending_suffix_space,
                            run_index,
                            range,
                            FirstLetterPseudoFragmentRole::AssociatedSuffix,
                        );
                    } else if character_is_first_letter_suffix_punctuation(base) {
                        selection.append(&mut pending_suffix_space);
                        push_first_letter_stream_slice(
                            &mut selection,
                            run_index,
                            range,
                            FirstLetterPseudoFragmentRole::AssociatedSuffix,
                        );
                    } else {
                        return selection;
                    }
                }
            }
            InlineLineItem::Atom(atom) if atom.content().is_inline_edge() => {}
            InlineLineItem::Float(_) => {}
            InlineLineItem::Atom(_) => return Vec::new(),
        }
    }
    selection
}

fn first_letter_prefix_continues_into(origin: &InlineFragment, next: &InlineFragment) -> bool {
    origin.source() == next.source()
        && match (origin.tracking_scope(), next.tracking_scope()) {
            (Some(origin), Some(next)) => Rc::ptr_eq(origin, next),
            (None, None) => true,
            _ => false,
        }
}

fn first_letter_unit_base(unit: &str) -> Option<char> {
    unit.chars().find(|character| {
        !character_is_unicode_mark(*character)
            && !character_is_default_ignorable_code_point(*character)
    })
}

fn push_first_letter_stream_slice(
    slices: &mut Vec<FirstLetterStreamSlice>,
    run_index: usize,
    range: std::ops::Range<usize>,
    role: FirstLetterPseudoFragmentRole,
) {
    if let Some(previous) = slices.last_mut()
        && previous.run_index == run_index
        && previous.role == role
        && previous.range.end == range.start
    {
        previous.range.end = range.end;
        return;
    }
    slices.push(FirstLetterStreamSlice {
        run_index,
        range,
        role,
    });
}

fn split_fragment_for_first_letter_stream_selection(
    fragment: &InlineFragment,
    selected_slices: &[FirstLetterStreamSlice],
    first_letter_style: &ComputedStyle,
    used_style: &ComputedStyle,
    block_style: &ComputedStyle,
    paint_scope_id: Option<InlinePaintScopeId>,
    pseudo_group_id: FirstLetterPseudoGroupId,
) -> Vec<InlineFragment> {
    let mut pieces = Vec::new();
    let mut cursor = 0;
    for slice in selected_slices {
        debug_assert!(cursor <= slice.range.start);
        if cursor < slice.range.start {
            let mut before = fragment.clone();
            before.set_text(Rc::<str>::from(&fragment.text()[cursor..slice.range.start]));
            apply_first_letter_style_to_leading_preserved_whitespace(
                &mut before,
                first_letter_style,
            );
            pieces.push(before);
        }
        let mut selected = fragment.clone();
        selected.set_text(Rc::<str>::from(&fragment.text()[slice.range.clone()]));
        apply_first_letter_style_to_stream_selection(
            &mut selected,
            first_letter_style,
            used_style,
            block_style,
            slice.role,
            paint_scope_id,
            pseudo_group_id,
        );
        pieces.push(selected);
        cursor = slice.range.end;
    }
    if cursor < fragment.text().len() {
        let mut after = fragment.clone();
        after.set_text(Rc::<str>::from(&fragment.text()[cursor..]));
        if let Some(first_line_style) = block_style.first_line_style.as_deref() {
            after.style_mut().color = first_line_style.color;
        }
        pieces.push(after);
    }
    pieces
}

fn apply_first_letter_style_to_stream_selection(
    fragment: &mut InlineFragment,
    first_letter_style: &ComputedStyle,
    used_style: &ComputedStyle,
    block_style: &ComputedStyle,
    role: FirstLetterPseudoFragmentRole,
    paint_scope_id: Option<InlinePaintScopeId>,
    pseudo_group_id: FirstLetterPseudoGroupId,
) {
    let mut used_style = used_style.clone();
    if first_letter_style.color == block_style.color && block_style.first_line_style.is_none() {
        used_style.color = fragment.style().color;
    }
    // Prefixes and suffixes remain within `::first-letter`, but only the
    // L/N/S typographic unit participates in initial-letter geometry.
    if role != FirstLetterPseudoFragmentRole::TypographicInitial {
        used_style.initial_letter = css::InitialLetter::Normal;
    }
    *fragment.style_mut() = used_style;
    fragment.set_first_letter_pseudo_role(role);
    fragment.set_first_letter_pseudo_group_id(pseudo_group_id);
    if fragment.style().opacity.value() < 1.0 {
        let mut marker_style = fragment.style().clone();
        marker_style.opacity = css::Opacity::ONE;
        fragment.push_ancestor_inline_decoration(InlineAncestorDecoration {
            style: marker_style,
            hanging_edges: InlineHangingEdges::default(),
            paints_background_or_border: false,
            positioning_containing_block_id: None,
            paint_effect_scope_id: paint_scope_id,
        });
    }
    fragment.set_mergeable(false);
}

fn used_first_letter_style_for_graph(
    first_letter_style: &ComputedStyle,
    block_style: &ComputedStyle,
    font_system: &mut FontSystem,
) -> ComputedStyle {
    let mut style = first_letter_style.clone();
    // `::first-letter` inherits from the originating `::first-line`. The
    // cascade has already resolved an authored `color: inherit` against the
    // block style, so carry the first-line color into that otherwise
    // indistinguishable inherited value before the initial-letter run is
    // shaped and painted.
    // <https://www.w3.org/TR/css-pseudo-4/#first-line-pseudo>
    if style.color == block_style.color
        && let Some(first_line_style) = block_style.first_line_style.as_deref()
    {
        style.color = first_line_style.color;
    }
    let Some((size, _sink)) = style.initial_letter.specified() else {
        return style;
    };
    let surrounding_cap_height = font_system.used_cap_height_for_style(block_style).points();
    let initial_cap_height = font_system.used_cap_height_for_style(&style).points();
    let cap_ratio = (initial_cap_height / style.font_size.max(0.01)).max(0.01);
    let target_cap_height =
        ((size - 1.0).max(0.0) * block_style.line_height) + surrounding_cap_height.max(0.0);
    // `initial-letter` establishes the used font size from the surrounding
    // line geometry.  The computed `font-size` on `::first-letter` remains
    // observable in the cascade, but does not impose a minimum on the used
    // initial-letter size: authors commonly set an intentionally huge value
    // here to assert that it is ignored.
    // <https://drafts.csswg.org/css-inline-3/#initial-letter-sizing>
    let used_font_size = (target_cap_height / cap_ratio).max(0.01);
    style.font_size = used_font_size;
    style.line_height = used_font_size;
    style.line_height_value = css::ComputedLineHeight::from_points(used_font_size);
    style
}

fn measured_fragment_run(
    fragment: InlineFragment,
    tracking_scope: Rc<InlineTrackingScope>,
    font_system: &mut FontSystem,
) -> InlineParagraphRun {
    let shaped = font_system.shape_untracked_inline_line_with_style_identity(
        fragment.text(),
        &fragment.data.style,
        fragment.style().line_height,
    );
    let width = shaped
        .as_ref()
        .map(ShapedInlineLine::advance_width)
        .unwrap_or(0.0);
    InlineParagraphRun {
        item: InlineLineItem::Fragment(fragment.with_tracking_scope(tracking_scope)),
        width,
        shaped: shaped.map(Rc::new),
    }
}
