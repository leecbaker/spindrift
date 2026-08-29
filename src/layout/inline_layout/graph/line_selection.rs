use super::*;

impl InlineOpportunityGraph {
    pub(super) fn new(
        runs: Vec<InlineParagraphRun>,
        opportunities: Vec<InlineBreakOpportunity>,
    ) -> Self {
        let mut graph = Self {
            runs,
            opportunities,
            wrap_inside_avoid_depths: BTreeMap::new(),
            monotonic_source_measurement: RefCell::new(None),
        };
        for opportunity in &graph.opportunities {
            let depth = graph.lexical_wrap_inside_avoid_depth(opportunity.position);
            graph
                .wrap_inside_avoid_depths
                .insert(opportunity.position, depth);
        }
        graph
    }

    pub(in crate::layout) fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub(in crate::layout) fn start_position(&self) -> InlineGraphPosition {
        InlineGraphPosition::at_run_start(0)
    }

    pub(in crate::layout) fn end_position(&self) -> InlineGraphPosition {
        InlineGraphPosition::at_run_start(self.runs.len())
    }

    /// Return whether every graph run and CSS break boundary can use the
    /// immutable source-advance table. Line-local effects such as
    /// `::first-line` and hanging punctuation remain checked by the caller.
    pub(in crate::layout) fn supports_monotonic_source_measurement(&self) -> bool {
        if let Some(supported) = self.monotonic_source_measurement.borrow().as_ref() {
            return supported.is_some();
        }
        let runs_supported = self.runs.iter().all(|run| {
            matches!(
                &run.item,
                InlineLineItem::Fragment(fragment)
                    if matches!(fragment.style().word_break, css::WordBreak::BreakAll)
                        && matches!(
                            fragment.style().text_spacing_trim.resolved(),
                            TextSpacingTrim::SpaceAll | TextSpacingTrim::Normal
                        )
                        && !inline_run_has_nonzero_tracking(run)
                        && !fragment.text().contains('\t')
            )
        });
        let opportunities_supported = self.opportunities.iter().all(|opportunity| {
            self.wrap_inside_avoid_depth(opportunity.position) == 0
                && matches!(
                    opportunity.kind,
                    InlineBreakKind::SoftWrap
                        | InlineBreakKind::ExplicitVirtual
                        | InlineBreakKind::PreservedSpace
                        | InlineBreakKind::Forced
                )
                && !opportunity.hangs_from_fitting_measure()
                && !opportunity.is_discretionary()
                && opportunity.discretionary.is_none()
                && !opportunity.availability.is_fallback()
                && (opportunity.kind != InlineBreakKind::Forced
                    || opportunity.position == self.end_position())
        });
        let supported = runs_supported && opportunities_supported;
        if !supported {
            *self.monotonic_source_measurement.borrow_mut() = Some(None);
        }
        supported
    }

    /// Borrow the source-advance suffix beginning at `start`.
    ///
    /// The table is built at most once for an eligible graph. All later line
    /// selections simply subtract the selected start advance from the shared
    /// paragraph-relative candidate advances.
    pub(in crate::layout) fn monotonic_source_measure_cursor_after(
        &self,
        start: InlineGraphPosition,
    ) -> Option<LineMeasureCursor<'_>> {
        let index = self.monotonic_source_measurement_index()?;
        let first = self
            .opportunities
            .partition_point(|opportunity| opportunity.position <= start);
        let start_advance = self.monotonic_source_advance_at(start, &index)?;
        Some(LineMeasureCursor {
            opportunities: &self.opportunities[first..],
            index,
            first_opportunity: first,
            start_advance,
        })
    }

    fn monotonic_source_measurement_index(&self) -> Option<Rc<InlineLineMeasureIndex>> {
        if !self.supports_monotonic_source_measurement() {
            return None;
        }
        if self.monotonic_source_measurement.borrow().is_none() {
            #[cfg(feature = "layout-profile")]
            let profile_started = Instant::now();
            let index = self.build_monotonic_source_measurement_index();
            #[cfg(feature = "layout-profile")]
            if index.is_some() {
                crate::layout::layout_profile::record_inline_line_measure_index_build(
                    profile_started.elapsed(),
                );
                crate::layout::layout_profile::record_inline_line_measure_index_scan(
                    self.opportunities.len(),
                );
            }
            *self.monotonic_source_measurement.borrow_mut() = Some(index.map(Rc::new));
        }
        let cache = self.monotonic_source_measurement.borrow();
        let index = Rc::clone(cache.as_ref()?.as_ref()?);
        Some(index)
    }

    fn build_monotonic_source_measurement_index(&self) -> Option<InlineLineMeasureIndex> {
        let mut run_starts = Vec::with_capacity(self.runs.len() + 1);
        let mut advance = 0.0;
        for run in &self.runs {
            run_starts.push(advance);
            let InlineLineItem::Fragment(fragment) = &run.item else {
                return None;
            };
            advance += run
                .shaped
                .as_deref()?
                .monotonic_source_prefix_advance(fragment.text().len())?;
        }
        run_starts.push(advance);
        let mut index = InlineLineMeasureIndex {
            run_starts,
            opportunity_advances: Vec::with_capacity(self.opportunities.len()),
        };
        for opportunity in &self.opportunities {
            index
                .opportunity_advances
                .push(self.monotonic_source_advance_at(opportunity.position, &index)?);
        }
        Some(index)
    }

    fn monotonic_source_advance_at(
        &self,
        position: InlineGraphPosition,
        index: &InlineLineMeasureIndex,
    ) -> Option<f32> {
        if position == self.end_position() {
            return index.run_starts.last().copied();
        }
        let run = self.runs.get(position.run_index)?;
        let InlineLineItem::Fragment(fragment) = &run.item else {
            return None;
        };
        let advance = run
            .shaped
            .as_deref()?
            .monotonic_source_prefix_advance(position.byte_offset)?;
        (position.byte_offset <= fragment.text().len())
            .then(|| index.run_starts[position.run_index] + advance)
    }

    /// Return the number of `wrap-inside: avoid` inline boxes split by a
    /// candidate source boundary.
    ///
    /// Collection retains zero-advance inline box edges even when there is no
    /// decoration to paint. Those lexical markers let line selection recover
    /// non-inherited inline-box containment without making `wrap-inside`
    /// behave like an inherited text property. A boundary immediately before
    /// an end edge is outside that box, matching CSS Text's margin-edge rule.
    /// <https://drafts.csswg.org/css-text-4/#wrap-inside-property>
    /// <https://drafts.csswg.org/css-text-4/#line-breaking-details>
    pub(in crate::layout) fn wrap_inside_avoid_depth(&self, position: InlineGraphPosition) -> u16 {
        self.wrap_inside_avoid_depths
            .get(&position)
            .copied()
            .unwrap_or_else(|| self.lexical_wrap_inside_avoid_depth(position))
    }

    fn lexical_wrap_inside_avoid_depth(&self, position: InlineGraphPosition) -> u16 {
        let mut depth = 0u16;
        for run in &self.runs[..position.run_index] {
            if inline_box_edge_is_wrap_inside_avoid_start(&run.item) {
                depth = depth.saturating_add(1);
            } else if inline_box_edge_is_wrap_inside_avoid_end(&run.item) {
                depth = depth.saturating_sub(1);
            }
        }

        let Some(run) = self.runs.get(position.run_index) else {
            return depth;
        };
        let mut trailing_edge_index = match &run.item {
            InlineLineItem::Fragment(fragment) if position.byte_offset == fragment.text().len() => {
                position.run_index + 1
            }
            InlineLineItem::Atom(_) if position.byte_offset == 0 => position.run_index,
            InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                return depth;
            }
        };

        // A candidate at the end of text is immediately before its following
        // box-end markers. Those markers are on the trailing margin edge and
        // therefore lie outside `wrap-inside: avoid`; source positions encode
        // it as the text's terminal offset rather than a separate run start.
        while let Some(edge_run) = self.runs.get(trailing_edge_index) {
            if inline_box_edge_is_wrap_inside_avoid_end(&edge_run.item) {
                depth = depth.saturating_sub(1);
                trailing_edge_index += 1;
                continue;
            }
            break;
        }
        depth
    }

    pub(in crate::layout) fn float_at_position(
        &self,
        position: InlineGraphPosition,
    ) -> Option<&InlineFloat> {
        if position.byte_offset != 0 {
            return None;
        }
        self.runs
            .get(position.run_index)
            .and_then(|run| match &run.item {
                InlineLineItem::Float(float) => Some(float),
                InlineLineItem::Fragment(_) | InlineLineItem::Atom(_) => None,
            })
    }

    pub(in crate::layout) fn first_float_position_in_range(
        &self,
        range: InlineGraphRange,
    ) -> Option<InlineGraphPosition> {
        let run_range = self.run_indices_for_graph_range(range)?;
        run_range
            .filter(|run_index| {
                *run_index >= range.start.run_index
                    && *run_index < range.end.run_index
                    && matches!(
                        self.runs.get(*run_index).map(|run| &run.item),
                        Some(InlineLineItem::Float(_))
                    )
            })
            .map(InlineGraphPosition::at_run_start)
            .next()
    }

    /// Find the unbreakable continuation immediately after a candidate line
    /// end when it contains an inline float after visible no-wrap source.
    ///
    /// A legal soft wrap before a `nowrap` descendant ends the preceding
    /// line, but must not let marker-only lookahead absorb the descendant's
    /// leading source into it. On the following iteration the selector takes
    /// the returned complete continuation and its float transaction defers
    /// the float below that source line when necessary. Requiring visible
    /// unbreakable source before the marker leaves the distinct marker-leading
    /// case on its existing source-order placement path.
    /// <https://www.w3.org/TR/css-text-3/#white-space-property>
    /// <https://www.w3.org/TR/CSS22/visuren.html#float-position>
    pub(in crate::layout) fn unbreakable_inline_float_continuation_after(
        &self,
        candidate: InlineGraphRange,
    ) -> Option<UnbreakableInlineFloatContinuation> {
        if !self
            .break_opportunity_at(candidate.end)
            .is_some_and(opportunity_is_soft_wrap)
        {
            return None;
        }
        let following = InlineGraphRange {
            start: candidate.end,
            end: self.end_position(),
        };
        let marker = self.first_float_position_in_range(following)?;
        let float = self.float_at_position(marker)?;
        if float.style().float != Float::Right
            || float.style().allows_soft_wrap()
            || self
                .break_opportunities_after(candidate.end)
                .take_while(|opportunity| opportunity.position <= marker)
                .any(opportunity_is_soft_wrap)
            || !self.has_visible_unbreakable_source(InlineGraphRange {
                start: candidate.end,
                end: marker,
            })
        {
            return None;
        }

        let continuation_end = self
            .break_opportunities_after(marker)
            .find(|opportunity| opportunity_is_soft_wrap(*opportunity))
            .map_or_else(|| self.end_position(), |opportunity| opportunity.position);
        Some(UnbreakableInlineFloatContinuation {
            source_range: InlineGraphRange {
                start: candidate.end,
                end: continuation_end,
            },
            marker,
        })
    }

    fn has_visible_unbreakable_source(&self, range: InlineGraphRange) -> bool {
        let Some(mut run_range) = self.run_indices_for_graph_range(range) else {
            return false;
        };
        run_range.any(|run_index| {
            let run = &self.runs[run_index];
            match &run.item {
                InlineLineItem::Fragment(fragment) if !fragment.style().allows_soft_wrap() => {
                    let start = if run_index == range.start.run_index {
                        range.start.byte_offset
                    } else {
                        0
                    }
                    .min(fragment.text().len());
                    let end = if run_index == range.end.run_index {
                        range.end.byte_offset
                    } else {
                        fragment.text().len()
                    }
                    .min(fragment.text().len());
                    start < end
                        && fragment.text()[start..end]
                            .chars()
                            .any(|character| !is_css_collapsible_whitespace(character))
                }
                InlineLineItem::Atom(atom)
                    if !atom.style().allows_soft_wrap() && !atom.content().is_box_edge() =>
                {
                    true
                }
                InlineLineItem::Fragment(_)
                | InlineLineItem::Atom(_)
                | InlineLineItem::Float(_) => false,
            }
        })
    }

    pub(in crate::layout) fn line_measured_items_for_graph_range(
        &self,
        range: InlineGraphRange,
        font_system: &mut FontSystem,
    ) -> Vec<MeasuredInlineItem> {
        let Some(run_range) = self.run_indices_for_graph_range(range) else {
            return Vec::new();
        };
        let mut items = Vec::with_capacity(run_range.len());
        for run_index in run_range {
            if let Some(item) =
                self.measured_run_slice_for_graph_range(run_index, range, font_system)
            {
                items.push(item);
            }
        }
        items
    }

    pub(in crate::layout) fn materialize_line(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        self.materialize_line_with_text_spacing_width(
            range,
            selected_break,
            self.selected_line_end_condition(range, selected_break),
            None,
            font_system,
            block_style,
        )
    }

    /// Materialize a candidate against a known line measure. This retains the
    /// CSS Text conditional end-trimming decision in the selected line rather
    /// than recomputing a different shaped run for paint.
    pub(in crate::layout) fn materialize_line_for_available_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        available_width: f32,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        self.materialize_line_with_text_spacing_width(
            range,
            selected_break,
            self.selected_line_end_condition(range, selected_break),
            Some(available_width),
            font_system,
            block_style,
        )
    }

    /// Materialize a selected line with its CSS Text line-end condition and
    /// physical available width.
    pub(in crate::layout) fn materialize_line_for_selected_end_for_available_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        line_end: SelectedLineEndCondition,
        available_width: f32,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        self.materialize_line_with_text_spacing_width(
            range,
            selected_break,
            line_end,
            Some(available_width),
            font_system,
            block_style,
        )
    }

    fn materialize_line_with_text_spacing_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        line_end: SelectedLineEndCondition,
        text_spacing_available_width: Option<f32>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> MaterializedInlineGraphLine {
        debug_assert!(selected_break.is_none_or(|opportunity| {
            !opportunity.trims_next_line_start()
                || matches!(opportunity.kind, BreakEffect::PreservedSpace)
        }));
        let mut items = self.line_measured_items_for_graph_range(range, font_system);
        self.insert_clone_continuation_edges(range, &mut items);
        let selected_manual_soft_hyphen = selected_break.is_some_and(|opportunity| {
            opportunity.is_discretionary()
                && self.source_character_before(opportunity.position) == Some('\u{00ad}')
        });
        let trailing_discretionary = selected_break.and_then(|opportunity| {
            opportunity.discretionary.or_else(|| {
                // An authored U+00AD is itself a discretionary break even
                // when no language-specific spelling rule supplies a more
                // detailed effect. Its own source fragment owns the used
                // `hyphenate-character`.
                opportunity
                    .is_discretionary()
                    .then_some(DiscretionaryBreakEffect {
                        source_boundary: opportunity.position,
                        marker_owner: DiscretionaryMarkerOwner {
                            style_position: opportunity.position,
                        },
                        left_replacement: None,
                        right_replacement: None,
                        leading_shaping_context: SelectedLineShapingContext::PreserveJoining,
                    })
            })
        });
        let leading_discretionary = self.discretionary_effect_at(range.start);
        // Do not mutate the selected source sequence for CSS Text Phase II.
        // In particular, a collapsed separator before `br` remains available
        // to bidi, extraction, and decoration ownership even though it has no
        // used advance at the selected line edge.
        let trimmed_width = trailing_collapsible_measured_width(&items);
        let authored_marker_materialized_in_source = selected_manual_soft_hyphen
            && trailing_discretionary.is_some_and(|effect| effect.left_replacement.is_none());
        normalize_materialized_control_characters(
            &mut items,
            authored_marker_materialized_in_source,
            font_system,
        );
        if authored_marker_materialized_in_source {
            if trailing_discretionary.is_some_and(|effect| {
                effect.leading_shaping_context == SelectedLineShapingContext::PreserveJoining
            }) && materialized_items_have_joining_behavior(&items)
            {
                append_materialized_line_joiner(&mut items, font_system);
            }
        } else {
            apply_selected_discretionary_break(
                &mut items,
                trailing_discretionary,
                SelectedLineEdge::Trailing,
                font_system,
                &self.runs,
            );
        }
        apply_selected_discretionary_break(
            &mut items,
            leading_discretionary,
            SelectedLineEdge::Leading,
            font_system,
            &self.runs,
        );
        apply_materialized_text_spacing_trim(
            &mut items,
            font_system,
            range.start == self.start_position(),
            text_spacing_available_width,
        );
        // Candidate fitting and intrinsic sizing use the same ownership-aware
        // tracking advances as final visual paint. In an all-LTR candidate
        // this is already visual order; bidi paint resolves the same typed
        // boundaries again after UBA reordering.
        apply_visual_tracking_boundaries(&mut items);
        resolve_materialized_line_tab_and_ruby_geometry(&mut items, font_system, block_style);
        let widths = inline_content_width_for_line_items(&items, font_system, |item| {
            item.used_advance().points()
        });
        // A `pre-wrap` run hangs at a selected soft boundary.  It also hangs
        // before an unconditionally hanging other-space separator, even when
        // the line itself ends at a forced break: that separator means the
        // preserved run is not immediately followed by the forced break.
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
        let pre_wrap_suffix_width =
            trailing_pre_wrap_hanging_width_with_unconditional_separators(&items, font_system);
        let hanging_pre_wrap_width = if widths.trailing_space_width > 0.0 {
            pre_wrap_suffix_width
        } else {
            line_end.pre_wrap_hanging_width(
                pre_wrap_suffix_width,
                widths.fitting_width,
                text_spacing_available_width,
            )
        };
        let edge_effects = InlineLineEdgeEffects {
            collapsed_end_trim_width: trimmed_width,
            pre_wrap_hanging_width: hanging_pre_wrap_width,
            hanging_space_separator_width: widths.trailing_space_width,
            retained_break_spaces_end: selected_break.is_some_and(|opportunity| {
                opportunity.whitespace_edge.retains_break_spaces_advance()
            }),
            source_effects: selected_line_edge_source_effects(
                &items,
                trimmed_width > 0.0,
                hanging_pre_wrap_width > 0.0,
                widths.trailing_space_width > 0.0,
            ),
        };
        // The materialized line retains the selected source text. Edge
        // effects change its used advance; paint materialization applies the
        // corresponding source-range suppression only when emitting the
        // formatted fragment. Keeping this summary source-faithful preserves
        // bidi, extraction, and decoration ownership.
        let fitting_width = (widths.fitting_width
            - edge_effects.collapsed_end_trim_width
            - edge_effects.pre_wrap_hanging_width)
            .max(0.0);
        MaterializedInlineGraphLine {
            items,
            fitting_width,
            content_width: fitting_width,
            edge_effects,
        }
    }

    pub(in crate::layout) fn selected_line_end_condition(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
    ) -> SelectedLineEndCondition {
        match selected_break {
            Some(opportunity) if opportunity_is_soft_wrap(opportunity) => {
                SelectedLineEndCondition::SoftWrap
            }
            Some(_) => SelectedLineEndCondition::ForcedBreak,
            None if range.end == self.end_position() => SelectedLineEndCondition::ParagraphEnd,
            None => SelectedLineEndCondition::ForcedBreak,
        }
    }

    pub(in crate::layout) fn source_character_before(
        &self,
        position: InlineGraphPosition,
    ) -> Option<char> {
        if position.byte_offset > 0 {
            return self
                .runs
                .get(position.run_index)
                .and_then(|run| match &run.item {
                    InlineLineItem::Fragment(fragment) => fragment
                        .text()
                        .get(..position.byte_offset)
                        .and_then(|text| text.chars().next_back()),
                    InlineLineItem::Atom(_) | InlineLineItem::Float(_) => None,
                });
        }
        previous_text_fragment_before(&self.runs, position.run_index)
            .and_then(|fragment| fragment.text().chars().next_back())
    }

    fn discretionary_effect_at(
        &self,
        position: InlineGraphPosition,
    ) -> Option<DiscretionaryBreakEffect> {
        self.opportunities
            .iter()
            .find(|opportunity| opportunity.position == position)
            .and_then(|opportunity| opportunity.discretionary)
    }

    pub(in crate::layout) fn break_opportunities_after(
        &self,
        start: InlineGraphPosition,
    ) -> impl Iterator<Item = InlineBreakOpportunity> + '_ {
        self.opportunities
            .iter()
            .cloned()
            .filter(move |opportunity| opportunity.position > start)
    }

    /// Borrow the source-ordered suffix of legal break opportunities.
    ///
    /// Inline line fitting commonly revisits a graph while resolving
    /// orthogonal intrinsic sizes. Keeping the suffix borrowed prevents each
    /// attempt from allocating and copying every `word-break: break-all`
    /// boundary before the line-measure cursor can inspect it.
    pub(in crate::layout) fn break_opportunity_slice_after(
        &self,
        start: InlineGraphPosition,
    ) -> &[InlineBreakOpportunity] {
        let first = self
            .opportunities
            .partition_point(|opportunity| opportunity.position <= start);
        &self.opportunities[first..]
    }

    pub(in crate::layout) fn break_opportunity_at(
        &self,
        position: InlineGraphPosition,
    ) -> Option<InlineBreakOpportunity> {
        self.opportunities
            .iter()
            .find(|opportunity| opportunity.position == position)
            .copied()
    }

    /// Whether a selected soft hyphen is immediately followed by a source
    /// hard hyphen before another candidate boundary.
    ///
    /// Some language dictionaries treat this as a discretionary replacement:
    /// the selected first line gets `hyphenate-character`, while the literal
    /// hyphen remains at the following line edge. Selecting the later UAX #14
    /// boundary would instead consume that source character and lose the
    /// replacement. The relationship is source-local and therefore belongs to
    /// the opportunity graph rather than to a line-selector string heuristic.
    /// <https://drafts.csswg.org/css-text-4/#hyphenate-character>
    pub(in crate::layout) fn soft_hyphen_precedes_literal_hyphen(
        &self,
        soft_hyphen: InlineBreakOpportunity,
        later: InlineBreakOpportunity,
    ) -> bool {
        if !soft_hyphen.is_discretionary()
            || soft_hyphen.position.run_index != later.position.run_index
            || soft_hyphen.position.byte_offset >= later.position.byte_offset
        {
            return false;
        }
        let Some(InlineLineItem::Fragment(fragment)) = self
            .runs
            .get(soft_hyphen.position.run_index)
            .map(|run| &run.item)
        else {
            return false;
        };
        let Some(between) = fragment
            .text()
            .get(soft_hyphen.position.byte_offset..later.position.byte_offset)
        else {
            return false;
        };
        between == "\u{2010}"
    }

    pub(in crate::layout) fn run_indices_for_graph_range(
        &self,
        range: InlineGraphRange,
    ) -> Option<std::ops::Range<usize>> {
        if range.end <= range.start || range.start.run_index >= self.runs.len() {
            return None;
        }
        let end_run = if range.end.byte_offset == 0 {
            range.end.run_index
        } else {
            range.end.run_index.saturating_add(1)
        }
        .min(self.runs.len());
        (range.start.run_index < end_run).then_some(range.start.run_index..end_run)
    }

    /// Count authored bytes in a graph range for layout diagnostics.
    #[cfg(feature = "layout-profile")]
    pub(in crate::layout) fn source_byte_len_for_range(&self, range: InlineGraphRange) -> usize {
        let Some(run_range) = self.run_indices_for_graph_range(range) else {
            return 0;
        };
        run_range
            .filter_map(|run_index| {
                let InlineLineItem::Fragment(fragment) = &self.runs.get(run_index)?.item else {
                    return Some(0);
                };
                let text_len = fragment.text().len();
                let start = if run_index == range.start.run_index {
                    range.start.byte_offset.min(text_len)
                } else {
                    0
                };
                let end = if run_index == range.end.run_index {
                    range.end.byte_offset.min(text_len)
                } else {
                    text_len
                };
                Some(end.saturating_sub(start))
            })
            .sum()
    }

    fn range_may_use_text_spacing_trim(&self, range: InlineGraphRange) -> bool {
        let Some(run_range) = self.run_indices_for_graph_range(range) else {
            return false;
        };
        self.runs[run_range].iter().any(|run| {
            let InlineLineItem::Fragment(fragment) = &run.item else {
                return false;
            };
            if fragment.style().text_spacing_trim.resolved() == TextSpacingTrim::SpaceAll {
                return false;
            }
            let vertical = matches!(
                fragment.style().text_layout_policy(),
                crate::css::TextLayoutPolicy::Vertical(_)
            );
            // `text-spacing-trim` changes only classified CJK punctuation.
            // Ordinary text (including preserved white space) retains its
            // source advance, so it can safely reuse the graph's borrowed
            // source measurement. Keeping this eligibility test content-aware
            // avoids unnecessarily materializing every standard text line.
            fragment.text().chars().any(|character| {
                crate::text::text_spacing_punctuation_class(
                    character,
                    fragment.style().language.as_deref(),
                    vertical,
                )
                .is_some()
            })
        })
    }

    pub(in crate::layout) fn borrowed_line_measurement_for_full_run_range(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> Option<BorrowedInlineLineMeasurement> {
        // `text-spacing-trim` selects used punctuation advances only after a
        // candidate establishes its line edges. A source-shaped graph slice
        // therefore cannot be borrowed as a final measurement when any
        // selected text can participate in that policy.
        if self.range_may_use_text_spacing_trim(range) {
            return None;
        }
        // A selected discretionary edge owns a generated marker and may own
        // spelling/shaping changes. Reuse of source-run widths would omit
        // those used-line effects, so only the full materializer may measure
        // it.
        if selected_break.is_some_and(|opportunity| opportunity.discretionary.is_some()) {
            return None;
        }
        if range.start.byte_offset != 0 || range.end.byte_offset != 0 {
            return None;
        }
        let run_range = self.run_indices_for_graph_range(range)?;
        if self.runs[run_range.clone()]
            .iter()
            .any(inline_run_has_nonzero_tracking)
        {
            return None;
        }
        // Ruby overhang depends on selected neighboring source and may reduce
        // the provisional widest-annotation advance. Intrinsic sizing must
        // therefore take the same materialized path as final line layout.
        if self.runs[run_range.clone()].iter().any(|run| {
            matches!(
                run.item,
                InlineLineItem::Atom(ref atom) if matches!(atom.content(), InlineAtomContent::Ruby { .. })
            )
        }) {
            return None;
        }
        // This fast path is used exclusively for intrinsic segments. A final
        // segment ending at the paragraph boundary must therefore use the
        // intrinsic end condition, rather than physical paragraph-end
        // conditional hanging. Otherwise preserved `pre-wrap` spaces
        // incorrectly contribute to min-content width.
        let line_end = selected_break
            .map(|opportunity| self.selected_line_end_condition(range, Some(opportunity)))
            .unwrap_or(SelectedLineEndCondition::IntrinsicSegmentEnd);
        let hanging_pre_wrap_width = if matches!(
            line_end,
            SelectedLineEndCondition::SoftWrap | SelectedLineEndCondition::IntrinsicSegmentEnd
        ) {
            trailing_pre_wrap_hanging_width_with_unconditional_separators(
                &self.runs[run_range.clone()],
                font_system,
            )
        } else {
            0.0
        };
        let runs = &self.runs[run_range];
        if runs.iter().any(|run| match &run.item {
            InlineLineItem::Fragment(fragment) => fragment_text_needs_materialized_normalization(
                fragment.text(),
                selected_break.is_some_and(InlineBreakOpportunity::is_discretionary),
            ),
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => false,
        }) {
            return None;
        }
        let widths = inline_content_width_for_line_items(runs, font_system, |run| run.width);
        let tab_advance_adjustment =
            selected_line_tab_advance_adjustment(runs, font_system, block_style, |run| run.width);
        Some(BorrowedInlineLineMeasurement {
            content_width: (widths.content_width + tab_advance_adjustment
                - trailing_collapsible_run_width(runs)
                - hanging_pre_wrap_width)
                .max(0.0),
        })
    }

    pub(in crate::layout) fn measured_run_slice_for_graph_range(
        &self,
        run_index: usize,
        range: InlineGraphRange,
        font_system: &mut FontSystem,
    ) -> Option<MeasuredInlineItem> {
        let run = self.runs.get(run_index)?;
        match &run.item {
            InlineLineItem::Fragment(fragment) => {
                let bidi_continuations = self.bidi_scope_continuations_for_range(range);
                let owns_prefix = run_index == range.start.run_index;
                let last_selected_run = if range.end.byte_offset == 0 {
                    range.end.run_index.checked_sub(1)
                } else {
                    Some(range.end.run_index)
                };
                let owns_suffix = Some(run_index) == last_selected_run;
                let mut bidi_prefix = String::new();
                if owns_prefix {
                    bidi_prefix.push_str(&bidi_continuations.prefix_parent_context);
                    bidi_prefix.push_str(&bidi_continuations.prefix);
                }
                let mut bidi_suffix = String::new();
                if owns_suffix {
                    bidi_suffix.push_str(&bidi_continuations.trailing_line_edge_context);
                    bidi_suffix.push_str(&bidi_continuations.suffix);
                    bidi_suffix.push_str(&bidi_continuations.suffix_parent_context);
                }
                let has_bidi_scope_context = !bidi_prefix.is_empty() || !bidi_suffix.is_empty();
                let text_len = fragment.text().len();
                let start = if run_index == range.start.run_index {
                    range.start.byte_offset.min(text_len)
                } else {
                    0
                };
                let end = if run_index == range.end.run_index {
                    range.end.byte_offset.min(text_len)
                } else {
                    text_len
                };
                if start >= end
                    || !fragment.text().is_char_boundary(start)
                    || !fragment.text().is_char_boundary(end)
                {
                    return None;
                }
                if start == 0 && end == text_len && !has_bidi_scope_context {
                    return Some(MeasuredInlineItem::new(
                        run.item.clone(),
                        run.width,
                        run.shaped.clone(),
                    ));
                }
                let mut fragment = fragment.clone();
                let mut hanging_edges = fragment.hanging_edges();
                let segment_text = Rc::<str>::from(&fragment.text()[start..end]);
                fragment.set_text(segment_text);
                hanging_edges.blocks_start = hanging_edges.blocks_start && start == 0;
                hanging_edges.blocks_end = hanging_edges.blocks_end && end == text_len;
                fragment = fragment.with_hanging_edges(hanging_edges);
                let mut source_selection = (!has_bidi_scope_context)
                    .then(|| {
                        // A transparent inline-boundary group may already
                        // carry a selection from a larger logical source.
                        // Derive this line fragment from that original source
                        // rather than treating the graph's intermediate
                        // slice as a new word-shaped artifact.
                        fragment
                            .source_shaped_selection()
                            .and_then(|selection| selection.subselection(start..end))
                            .or_else(|| {
                                run.shaped.as_ref().and_then(|shaped| {
                                    SourceShapedSelection::from_source(
                                        Rc::clone(shaped),
                                        start..end,
                                    )
                                })
                            })
                    })
                    .flatten();
                let shaped = source_selection
                    .as_ref()
                    .map(|selection| selection.selected().clone())
                    .or_else(|| {
                        font_system.shape_bidi_scoped_logical_line(
                            fragment.text(),
                            fragment.style(),
                            fragment.style().line_height,
                            &bidi_prefix,
                            &bidi_suffix,
                        )
                    });
                let width = shaped
                    .as_ref()
                    .map(ShapedInlineLine::advance_width)
                    .unwrap_or(0.0);
                if let (Some(selection), Some(shaped)) = (&mut source_selection, shaped.as_ref()) {
                    selection.replace_selected(shaped.clone());
                }
                let shaped = shaped.map(Rc::new);
                fragment.set_source_shaped_selection(source_selection);
                Some(MeasuredInlineItem::new(
                    InlineLineItem::Fragment(fragment),
                    width,
                    shaped,
                ))
            }
            InlineLineItem::Atom(_) | InlineLineItem::Float(_) => {
                let run_start = InlineGraphPosition::at_run_start(run_index);
                let run_end = InlineGraphPosition::at_run_start(run_index + 1);
                (range.start <= run_start && run_end <= range.end).then(|| {
                    MeasuredInlineItem::new(run.item.clone(), run.width, run.shaped.clone())
                })
            }
        }
    }

    /// Measure a plain source range without materializing selected line items.
    ///
    /// The monotonic `word-break: break-all` selector is the sole caller. Its
    /// eligibility check excludes all CSS Text effects that can make a source
    /// advance differ from the selected line's fitting advance.
    pub(in crate::layout) fn monotonic_source_range_width(
        &self,
        range: InlineGraphRange,
    ) -> Option<f32> {
        if let Some(index) = self.monotonic_source_measurement_index() {
            let start = self.monotonic_source_advance_at(range.start, &index)?;
            let end = self.monotonic_source_advance_at(range.end, &index)?;
            return Some(end - start);
        }
        let run_range = self.run_indices_for_graph_range(range)?;
        let mut width = 0.0;
        for run_index in run_range {
            let run = self.runs.get(run_index)?;
            let InlineLineItem::Fragment(fragment) = &run.item else {
                return None;
            };
            let text_len = fragment.text().len();
            let start = if run_index == range.start.run_index {
                range.start.byte_offset.min(text_len)
            } else {
                0
            };
            let end = if run_index == range.end.run_index {
                range.end.byte_offset.min(text_len)
            } else {
                text_len
            };
            if start >= end
                || !fragment.text().is_char_boundary(start)
                || !fragment.text().is_char_boundary(end)
            {
                return None;
            }
            width += if start == 0 && end == text_len {
                run.width
            } else {
                run.shaped
                    .as_deref()?
                    .source_range_advance_width(start..end)?
            };
        }
        Some(width)
    }

    pub(in crate::layout) fn intrinsic_contribution(
        &self,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> InlineIntrinsicContribution {
        if self.runs.is_empty() {
            return InlineIntrinsicContribution::default();
        }
        let materialized = self.materialize_line(
            InlineGraphRange {
                start: self.start_position(),
                end: self.end_position(),
            },
            None,
            font_system,
            block_style,
        );
        let hanging_widths = hanging_punctuation_widths_for_line_items(
            font_system,
            &materialized.items,
            block_style,
            true,
            true,
            false,
        );
        let max_content =
            (materialized.content_width - hanging_widths.start - hanging_widths.end).max(0.0);
        let mut min_content = 0.0_f32;
        let mut segment_start = self.start_position();
        for opportunity in self
            .opportunities
            .iter()
            .cloned()
            .filter(|opportunity| opportunity.availability.participates_in_min_content())
        {
            if opportunity.position <= segment_start || opportunity.position >= self.end_position()
            {
                continue;
            }
            let range = InlineGraphRange {
                start: segment_start,
                end: opportunity.position,
            };
            min_content = min_content.max(self.intrinsic_segment_width(
                range,
                Some(opportunity),
                font_system,
                block_style,
            ));
            segment_start = opportunity.position;
        }
        min_content = min_content.max(self.intrinsic_segment_width(
            InlineGraphRange {
                start: segment_start,
                end: self.end_position(),
            },
            None,
            font_system,
            block_style,
        ));
        InlineIntrinsicContribution::new(
            LogicalInlineContentSize::new(content_box_pt(min_content)),
            LogicalInlineContentSize::new(content_box_pt(max_content.max(min_content))),
        )
    }

    pub(in crate::layout) fn intrinsic_segment_width(
        &self,
        range: InlineGraphRange,
        selected_break: Option<InlineBreakOpportunity>,
        font_system: &mut FontSystem,
        block_style: &ComputedStyle,
    ) -> f32 {
        if let Some(measurement) = self.borrowed_line_measurement_for_full_run_range(
            range,
            selected_break,
            font_system,
            block_style,
        ) {
            return measurement.content_width;
        }
        // The final preserved `pre-wrap` spaces remain in the source line and
        // its max-content geometry, but are non-constraining at the end of a
        // min-content segment. Materialize that terminal candidate with the
        // same Phase II hanging rule used by the borrowed fast path above.
        // <https://drafts.csswg.org/css-text-3/#white-space-phase-2>
        let materialized = self.materialize_line_with_text_spacing_width(
            range,
            selected_break,
            if selected_break.is_none() && range.end == self.end_position() {
                SelectedLineEndCondition::IntrinsicSegmentEnd
            } else {
                self.selected_line_end_condition(range, selected_break)
            },
            None,
            font_system,
            block_style,
        );
        if materialized.items.is_empty() {
            return 0.0;
        }
        materialized.content_width
    }
}
