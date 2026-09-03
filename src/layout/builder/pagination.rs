use super::*;

/// Continuation-local containing-block state, separate from the page selected
/// for the destination fragmentainer.
///
/// The destination page may have different `@page` geometry, but a fragmented
/// nested formatting context keeps its own local insets, percentage bases,
/// writing-mode axes, and float exclusions.
/// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
#[derive(Clone)]
pub(in crate::layout) struct FragmentContinuationContext {
    pub(in crate::layout) local_offsets: FragmentOffsets,
    canvas_insets: Vec<FragmentOffsets>,
    logical_inline_sizes: Vec<f32>,
    child_available_space: Vec<ChildAvailableSpace>,
    block_percentage_context_stack: BlockPercentageContextStack,
    direction: Direction,
    writing_mode: WritingMode,
    float_contexts: Vec<FloatContext>,
    fragmentainer_kind: FragmentainerKind,
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn push_page(&mut self) {
        self.push_page_for_page_name(self.current_page_name.clone().as_deref());
        self.record_current_fragmentainer_destination();
    }

    /// Materializes a page transition while retaining the source page type for
    /// the page being committed and resolving the destination page context
    /// from `destination_page_name`.
    ///
    /// Named-page selection is a forced break with a page-context change.  A
    /// generic page break cannot perform it by mutating `current_page_name`
    /// on either side of the push without assigning one of the two page boxes
    /// the wrong type.
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>
    #[track_caller]
    pub(in crate::layout) fn push_page_for_page_name(
        &mut self,
        destination_page_name: Option<&str>,
    ) {
        if self.footnote_measurement_depth == 0 {
            self.flush_current_page_footnotes();
        }
        let next_fragmentainer_index = self.pages.len() + 1;
        let next_override_placement = self
            .fragmentainer_override
            .map(|override_| override_.placement_for_fragmentainer(next_fragmentainer_index));
        let next_override_context = next_override_placement.map(|placement| {
            debug_assert_eq!(placement.ordinal(), next_fragmentainer_index);
            placement.scratch_context()
        });
        let named_page_transition = self.current_page_name.as_deref() != destination_page_name;
        let fragment_replay_offsets = (!named_page_transition)
            .then(|| {
                self.float_fragment_parent_inline_spans
                    .last()
                    .copied()
                    .map(|parent_span| FragmentOffsets {
                        left: parent_span.left_x() - self.current_page_context.left(),
                        right: self.current_page_context.right() - parent_span.right_x(),
                        top: 0.0,
                    })
            })
            .flatten();
        if !self.current_page_has_content()
            && !self.current_page_has_named_page_flow_content
            && self.current_page_selected_name.is_none()
        {
            // CSS Fragmentation allows a box fragment to be split across
            // fragmentainers, but a carried fragment offset must not make a
            // fresh empty page permanently unfillable. If a break is requested
            // before anything painted on the current page, keep the same page
            // number and retry the fragment at the top of this page area:
            // <https://www.w3.org/TR/css-break-3/#breaking-rules>.
            let offsets = fragment_replay_offsets.unwrap_or_else(|| FragmentOffsets {
                top: 0.0,
                ..self.current_fragment_offsets()
            });
            let context = next_override_context.unwrap_or_else(|| {
                self.resolved_page_context_for_name(
                    self.destination_document_page_number(self.pages.len() + 1),
                    false,
                    destination_page_name,
                )
            });
            let advances_to_larger_fragmentainer = self.fragmentainer_override.is_some()
                && context.area_height() > self.current_page_context.area_height() + 0.01;
            if advances_to_larger_fragmentainer {
                let next_page = page_for_context(context);
                let page = std::mem::replace(&mut self.current_page, next_page);
                self.current_page_has_flow_content = false;
                self.current_page_has_named_page_flow_content = false;
                self.pages.push(page);
                self.page_names.push(self.current_page_name.clone());
                self.page_blanks.push(false);
                self.page_named_strings
                    .push(std::mem::take(&mut self.current_page_named_strings));
                self.page_running_elements
                    .push(std::mem::take(&mut self.current_page_running_elements));
                self.apply_page_context(context, offsets);
                self.current_page_selected_name = None;
                self.truncate_page_start_margins = true;
                self.apply_pending_fragments_for_current_page();
                return;
            }
            self.current_page = page_for_context(context);
            self.current_page_has_flow_content = false;
            self.current_page_has_named_page_flow_content = false;
            self.apply_page_context(context, offsets);
            self.current_page_selected_name = None;
            self.truncate_page_start_margins = true;
            self.apply_pending_fragments_for_current_page();
            return;
        }
        // A fragmented float owns the complete paint subtree of each of its
        // fragments, including positioned descendants that cross this page
        // boundary.  Let the float harvest those layers after its child
        // layout completes rather than committing them to the source page.
        // <https://www.w3.org/TR/css-break-3/#breaks-between>
        if self.float_paint_capture_depth == 0 {
            self.flush_positioned_layers();
        }
        let offsets = if named_page_transition {
            // A class-A named-page boundary is a forced page break in the
            // current block-fragmentation context. Re-enter that context with
            // the same normalized continuation origin as an explicit
            // `break-before: page`; a raw page-area offset would retain the
            // source root/body canvas rather than the destination fragment's
            // canvas translation.
            //
            // This is deliberately distinct from the empty-page replacement
            // above. Before a page has been committed there is no preceding
            // root/body fragment to continue.
            self.block_page_break_continuation_context().local_offsets
        } else {
            fragment_replay_offsets
                .unwrap_or_else(|| self.current_fragment_offsets_for_page_break())
        };
        let next_context = next_override_context.unwrap_or_else(|| {
            self.resolved_page_context_for_name(
                self.destination_document_page_number(self.pages.len() + 2),
                false,
                destination_page_name,
            )
        });
        let next_page = page_for_context(next_context);
        let page = std::mem::replace(&mut self.current_page, next_page);
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.pages.push(page);
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(false);
        self.page_named_strings
            .push(std::mem::take(&mut self.current_page_named_strings));
        self.page_running_elements
            .push(std::mem::take(&mut self.current_page_running_elements));
        self.apply_page_context(next_context, offsets);
        self.current_page_selected_name = None;
        self.truncate_page_start_margins = true;
        self.apply_pending_fragments_for_current_page();
    }

    pub(in crate::layout) fn push_blank_page(&mut self) {
        // CSS Fragmentation forced left/right/recto/verso breaks can generate
        // blank pages. Those pages are real page boxes and match `@page :blank`.
        // https://www.w3.org/TR/css-break-3/#break-between
        let page_number = self.destination_document_page_number(self.pages.len() + 1);
        let context = self.resolved_page_context(page_number, true);
        self.pages.push(page_for_context(context));
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(true);
        self.page_named_strings.push(HashMap::new());
        self.page_running_elements.push(HashMap::new());
    }

    #[track_caller]
    pub(in crate::layout) fn push_page_if_nonempty(&mut self) {
        if self.current_page_has_content() {
            self.push_page();
        }
    }

    /// Capture the nested containing block before selecting a destination
    /// page or column. Page selection itself remains the responsibility of
    /// the ordinary fragmentation transition.
    pub(in crate::layout) fn fragment_continuation_context(&self) -> FragmentContinuationContext {
        FragmentContinuationContext {
            // This context is replayed after the destination page has been
            // selected. Preserve the actual page-local containing-block
            // edges, including root/body canvas insets, rather than the
            // generic page-break offsets which intentionally subtract those
            // insets for ordinary root-flow continuation.
            local_offsets: FragmentOffsets {
                left: self.content_left - self.current_page_context.left(),
                right: self.current_page_context.right() - self.content_right,
                top: 0.0,
            },
            canvas_insets: self.document_canvas_fragment_insets.clone(),
            logical_inline_sizes: self.content_logical_inline_size_stack.clone(),
            child_available_space: self.child_available_space_stack.clone(),
            block_percentage_context_stack: self.block_percentage_context_stack.clone(),
            direction: self.containing_block_direction,
            writing_mode: self.containing_block_writing_mode,
            float_contexts: self.float_contexts.clone(),
            fragmentainer_kind: self.active_fragmentainer_kind(),
        }
    }

    /// Capture an in-flow block retry's page continuation.
    ///
    /// Unlike table-row slices, a nested block retry re-enters each fragment's
    /// root/body canvas. Its local offsets must therefore retain the complete
    /// ordinary page-break continuation origin, including the canvas's inline
    /// insets, so replay starts at the same position as normal in-flow page
    /// continuation.
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>
    pub(in crate::layout) fn block_page_break_continuation_context(
        &self,
    ) -> FragmentContinuationContext {
        let mut continuation = self.fragment_continuation_context();
        // `current_fragment_offsets_for_page_break` already restores the
        // document-canvas inline insets needed by a real page continuation.
        // Subtracting them here a second time shifts retried avoided blocks
        // outside the propagated root/body canvas on their destination page.
        continuation.local_offsets = self.current_fragment_offsets_for_page_break();
        continuation
    }

    /// Capture a float's destination context.
    ///
    /// A root-flow float crosses the same canvas boundary as ordinary in-flow
    /// page content. A nested float instead keeps its narrower local
    /// containing block (for example an overflow-clipped formatting context).
    pub(in crate::layout) fn float_page_break_continuation_context(
        &self,
    ) -> FragmentContinuationContext {
        let canvas = self.document_canvas_fragment_insets.iter().fold(
            FragmentOffsets::ZERO,
            |total, inset| FragmentOffsets {
                left: total.left + inset.left,
                right: total.right + inset.right,
                top: total.top + inset.top,
            },
        );
        let root_flow_width =
            (self.content_left - self.current_page_context.left() - canvas.left).abs() <= 0.01
                && (self.current_page_context.right() - self.content_right - canvas.right).abs()
                    <= 0.01;
        if root_flow_width {
            // The deferred float is replayed in isolation after its parent
            // flow has remained on the source page. Unlike an ordinary
            // in-flow page break, no root/body layout pass will re-enter the
            // document canvas before the float is placed. Preserve the
            // actual root-flow insets here so the destination margin box has
            // the same containing block as an equivalent forced-break block.
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            let mut continuation = self.fragment_continuation_context();
            // Float exclusion rectangles are page-local. A root-flow float
            // moved to a fresh page must not be placed beside a float from
            // the preceding page.
            // The context vector also encodes active float-formatting scopes;
            // retain those frames so subsequent placement always has a root
            // context. Only the exclusion shapes themselves belong to the
            // preceding page.
            for context in &mut continuation.float_contexts {
                context.shapes.clear();
            }
            continuation
        } else {
            self.fragment_continuation_context()
        }
    }

    /// Apply a captured continuation to an already-selected destination page.
    ///
    /// `push_page` carries mutable layout stacks until replay. Reinstall the
    /// captured continuation state before applying the destination page so
    /// page-area rebasing starts from the source formatting context rather
    /// than from a partially advanced sibling or scratch fragment. The page
    /// context then recalculates only entries that genuinely represent the
    /// destination page area.
    pub(in crate::layout) fn replay_fragment_continuation_on_page(
        &mut self,
        continuation: &FragmentContinuationContext,
        destination: PageContext,
    ) {
        debug_assert_eq!(continuation.fragmentainer_kind, FragmentainerKind::Page);
        debug_assert_eq!(self.active_fragmentainer_kind(), FragmentainerKind::Page);
        self.document_canvas_fragment_insets = continuation.canvas_insets.clone();
        self.content_logical_inline_size_stack = continuation.logical_inline_sizes.clone();
        self.child_available_space_stack = continuation.child_available_space.clone();
        self.block_percentage_context_stack = continuation.block_percentage_context_stack.clone();
        self.containing_block_direction = continuation.direction;
        self.containing_block_writing_mode = continuation.writing_mode;
        self.float_contexts = continuation.float_contexts.clone();

        self.current_page = page_for_context(destination);
        self.apply_page_context(destination, continuation.local_offsets);
        self.current_page_selected_name = None;
    }

    /// Captures the active formatting-context insets from the current page area.
    ///
    /// A page break fragments boxes without leaving their containing block, while
    /// a named-page transition can select a different page area. Keeping these
    /// offsets preserves ancestor margins and padding on the new page fragment:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn current_fragment_offsets(&self) -> FragmentOffsets {
        let raw = FragmentOffsets {
            left: self.content_left - self.current_page_context.left(),
            right: self.current_page_context.right() - self.content_right,
            top: self
                .fragment_top_offsets
                .last()
                .map(|offset| offset.first_fragment_start())
                .unwrap_or_else(|| self.current_page_context.top() - self.cursor_y),
        };
        let canvas = self.document_canvas_fragment_insets.iter().fold(
            FragmentOffsets::ZERO,
            |total, inset| FragmentOffsets {
                left: total.left + inset.left,
                right: total.right + inset.right,
                top: total.top + inset.top,
            },
        );
        FragmentOffsets {
            left: raw.left - canvas.left,
            right: raw.right - canvas.right,
            top: raw.top - canvas.top,
        }
    }

    /// Captures fragment insets for an actual page break.
    ///
    /// The next fragment keeps inline containing-block insets, including the
    /// root/body canvas's inline margins, but starts at the block-start edge
    /// of the new fragmentainer. CSS Fragmentation's initial
    /// `box-decoration-break: slice` behavior does not clone ancestor
    /// block-start margin, border, or padding into continuation fragments:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>.
    pub(in crate::layout) fn current_fragment_offsets_for_page_break(&self) -> FragmentOffsets {
        // A multicolumn implementation uses temporary page contexts as
        // fragmentainers. Their contexts already encode the real page's
        // canvas margins, so subtracting document-canvas insets again shifts
        // each continuation column horizontally. Preserve only the local
        // containing-block inset when advancing a synthetic column page.
        // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        if self
            .fragmentainer_override
            .is_some_and(|override_| override_.kind == FragmentainerKind::Column)
        {
            return FragmentOffsets {
                left: self.content_left - self.current_page_context.left(),
                right: self.current_page_context.right() - self.content_right,
                // Synthetic column fragmentainers retain their local inline
                // containing block, but `clone` still restarts every active
                // block below its cloned block-start border and padding.
                // Returning a raw zero here made definite blocks consume
                // that start edge as source content, leaving the following
                // sibling fifteen CSS pixels too high in clone-004.
                top: self
                    .fragment_top_offsets
                    .iter()
                    .map(|offset| offset.continuation_start())
                    .sum(),
            };
        }
        let mut offsets = self.current_fragment_offsets();
        // `current_fragment_offsets` removes active document-canvas insets
        // so an isolated fragment replay can reconstruct its own canvas. A
        // real root-flow page continuation instead re-enters that canvas on
        // the destination page, so retain its logical-inline insets here.
        // Otherwise an ordinary body margin disappears after the first page
        // even though the page area itself changes correctly.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        // <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds>
        let canvas = self.document_canvas_fragment_insets.iter().fold(
            FragmentOffsets::ZERO,
            |total, inset| FragmentOffsets {
                left: total.left + inset.left,
                right: total.right + inset.right,
                top: total.top + inset.top,
            },
        );
        offsets.left += canvas.left;
        offsets.right += canvas.right;
        // Reset the exhausted source *block-start* coordinate only. In a
        // vertical principal flow, clearing both physical horizontal insets
        // incorrectly widens the continuation and loses the root/body
        // block-end inset; `vertical-rl` restarts from the right and
        // `vertical-lr` from the left.
        // <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
        // <https://www.w3.org/TR/css-break-4/#box-splitting>
        offsets.clear_fragmentainer_block_start(FlowAxes::new(
            self.principal_flow.writing_mode,
            self.principal_flow.used_direction(),
        ));
        // A cloned ancestor creates a fresh border/padding inset in every
        // continuation.  The regular fragment-offset reset above implements
        // `slice`; reapply only the explicitly recorded clone start edges.
        // These are physical top insets because this page-flow path is the
        // horizontal principal-flow continuation boundary. Vertical roots
        // use their dedicated logical page-fragmentation projection instead.
        if self.principal_flow.writing_mode == WritingMode::HorizontalTb {
            offsets.top += self
                .fragment_top_offsets
                .iter()
                .map(|offset| offset.continuation_start())
                .sum::<f32>();
        }
        offsets
    }

    /// Applies a new page context while preserving active fragment insets.
    ///
    /// CSS Paged Media changes the page area's size and margins per page, but
    /// CSS Fragmentation keeps content in the same containing block across page
    /// fragments:
    /// <https://www.w3.org/TR/css-page-3/#page-model> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn apply_page_context(
        &mut self,
        context: PageContext,
        offsets: FragmentOffsets,
    ) {
        let previous_context = self.current_page_context;
        self.current_page_context = context;
        self.current_page.rotation = context.rotation;
        self.cursor_y = context.top() - offsets.top;
        self.content_left = context.left() + offsets.left;
        self.content_right = (context.right() - offsets.right).max(self.content_left);
        self.rebase_page_area_context_caches(previous_context, context);
    }

    /// Selects the first destination fragmentainer for an out-of-flow scratch
    /// layout without changing the already-resolved physical box geometry.
    ///
    /// Absolutely positioned boxes resolve their insets in their continuous
    /// containing block, then fragment their contents through destination
    /// page areas.  When the static-position rectangle begins on a later
    /// page, the scratch layout must use that page's percentage bases and
    /// continuation dimensions from its first fragment.  Its physical cursor
    /// and containing-block coordinates have already been resolved, however,
    /// and must not be reset to the page-area origin.
    /// <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
    /// <https://www.w3.org/TR/css-page-3/#page-model>
    pub(in crate::layout) fn rebase_positioned_scratch_page_context(
        &mut self,
        context: PageContext,
    ) {
        let previous_context = self.current_page_context;
        self.current_page_context = context;
        self.current_page = page_for_context(context);
        self.rebase_page_area_context_caches(previous_context, context);
    }

    /// Updates active parent caches that directly represent the page area.
    ///
    /// A page transition can select a different page size. Root-level
    /// auto-sized formatting contexts use cached page-area dimensions while
    /// their descendants are being laid out, so those exact page-area entries
    /// must change with the context before the descendant's used percentages
    /// are resolved.
    /// <https://www.w3.org/TR/css-page-3/#page-model>
    fn rebase_page_area_context_caches(
        &mut self,
        previous_context: PageContext,
        next_context: PageContext,
    ) {
        const EPSILON: f32 = 0.01;
        if previous_context == next_context {
            return;
        }
        let active_page_writing_mode = self
            .child_available_space_stack
            .last()
            .map(|space| space.writing_mode)
            .unwrap_or(WritingMode::HorizontalTb);
        if self
            .content_logical_inline_size_stack
            .last()
            .is_some_and(|size| {
                (*size - previous_context.logical_inline_size(active_page_writing_mode)).abs()
                    <= EPSILON
            })
            && let Some(size) = self.content_logical_inline_size_stack.last_mut()
        {
            *size = next_context.logical_inline_size(active_page_writing_mode);
        }
        let page_available_space = ChildAvailableSpace::new(
            active_page_writing_mode,
            PhysicalContentWidth::new(content_box_pt(next_context.area_width())),
            true,
            Some(PhysicalContentHeight::new(content_box_pt(
                next_context.area_height(),
            ))),
            self.initial_containing_block_physical_height(),
        );
        if self
            .child_available_space_stack
            .last()
            .is_some_and(|space| {
                space.writing_mode == active_page_writing_mode
                    && (space.physical_content_width.points() - previous_context.area_width()).abs()
                        <= EPSILON
                    && (space.available_physical_height().points() - previous_context.area_height())
                        .abs()
                        <= EPSILON
            })
            && let Some(space) = self.child_available_space_stack.last_mut()
        {
            *space = page_available_space;
        }
    }

    /// Restores an enclosing page-area formatting context after a child caused
    /// a page transition with a different used page size.
    ///
    /// CSS Paged Media resolves each page's containing block from that page's
    /// used page area. An `html`/`body`-like auto-sized block that filled the
    /// preceding page area must therefore fill the new page area after a child
    /// fragments; restoring its old physical rectangle would retain the prior
    /// page width for all following siblings.
    /// <https://www.w3.org/TR/css-page-3/#page-model>
    pub(in crate::layout) fn restore_page_area_parent_context_after_page_transition(
        &mut self,
        previous_left: f32,
        previous_right: f32,
        page_context_at_entry: PageContext,
        page_index_at_entry: usize,
    ) {
        const EPSILON: f32 = 0.01;
        let filled_previous_page_area = (previous_left - page_context_at_entry.left()).abs()
            <= EPSILON
            && (previous_right - page_context_at_entry.right()).abs() <= EPSILON;
        if self.pages.len() != page_index_at_entry {
            // The child committed a page boundary. `apply_page_context` has
            // already installed the destination's continuation origin; in
            // particular, it has removed the source root/body canvas inset.
            // Restoring `previous_left` here would reintroduce that source
            // offset and make later siblings start at a different position
            // from the equivalent explicit forced break.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            return;
        }
        if self.current_page_context != page_context_at_entry && filled_previous_page_area {
            // The outer root/body canvas re-enters each page fragment, but a
            // named-page transition must not rebuild the destination from a
            // source fragment's generic offsets. Reapply only that stable
            // canvas inset here, after the new page area's geometry is known.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            let canvas = self.document_canvas_fragment_insets.iter().fold(
                FragmentOffsets::ZERO,
                |total, inset| FragmentOffsets {
                    left: total.left + inset.left,
                    right: total.right + inset.right,
                    top: total.top,
                },
            );
            self.content_left = self.current_page_context.left() + canvas.left;
            self.content_right =
                (self.current_page_context.right() - canvas.right).max(self.content_left);
        } else {
            self.content_left = previous_left;
            self.content_right = previous_right;
        }
    }

    pub(in crate::layout) fn apply_forced_break(&mut self, forced_break: PageBreak) {
        if !FragmentainerKind::Page.is_forced_break(forced_break) {
            return;
        }
        let current_empty_named_destination = !self.current_page_has_content()
            && self.page_names.last().map(Option::as_deref)
                != Some(self.current_page_name.as_deref());
        if self.current_page_has_content() {
            self.push_page();
        }
        while !forced_break_satisfied(
            forced_break,
            self.destination_document_page_number(self.pages.len() + 1),
            self.page_progression_direction,
        ) {
            self.push_blank_page();
        }
        if !self.current_page_has_content() && !current_empty_named_destination {
            let offsets = self.current_fragment_offsets_for_page_break();
            let page_number = self.destination_document_page_number(self.pages.len() + 1);
            let context = self.resolved_page_context(page_number, false);
            self.current_page = page_for_context(context);
            self.apply_page_context(context, offsets);
            self.current_page_selected_name = None;
        }
        // At a forced break, adjoining margins before the break are
        // truncated, but margins after the break are preserved. The box that
        // follows this boundary is therefore unlike a continuation placed at
        // an unforced fragmentainer break, whose block-start margin is
        // truncated.
        // <https://www.w3.org/TR/css-break-3/#break-margins>
        self.truncate_page_start_margins = false;
    }

    pub(in crate::layout) fn apply_forced_break_in(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        forced_break: PageBreak,
    ) {
        // Callers pass the resolved outgoing break value so that a flex, grid,
        // or table item can leave `auto` for its following sibling. Only a
        // value forced in the active fragmentainer kind may materialize a
        // continuation; treating `auto` as a column transition manufactures
        // an empty anonymous column after every such box.
        // <https://www.w3.org/TR/css-break-3/#forced-breaks>
        if !fragmentainer_kind.is_forced_break(forced_break) {
            return;
        }
        if self.fragmentation_suppression_depth > 0 {
            return;
        }
        if fragmentainer_kind == FragmentainerKind::Column
            && self
                .fragmentainer_override
                .is_some_and(|override_| override_.kind == FragmentainerKind::Column)
        {
            self.materialize_column_continuation();
            return;
        }
        if !fragmentainer_kind.materializes_page_cursor() {
            return;
        }
        self.apply_forced_break(forced_break);
    }

    /// Apply this generated box's `break-before` in the active fragmentainer.
    ///
    /// CSS Fragmentation defines `break-before` generically across
    /// fragmentainer types. Spindrift currently materializes only page transitions
    /// at this builder layer, but the break value is still resolved through the
    /// shared target-aware break context so column-specific values remain
    /// ignored here rather than accidentally treated as page breaks:
    /// <https://www.w3.org/TR/css-break-3/#break-between>.
    pub(in crate::layout) fn apply_forced_break_before_box_in(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        style: &ComputedStyle,
    ) {
        if let Some(forced_break) = FragmentBreakContext::for_standalone_box(style)
            .forced_break_before_in(fragmentainer_kind)
        {
            self.apply_forced_break_in(fragmentainer_kind, forced_break);
        }
    }

    /// Apply this generated box's `break-after` in the active fragmentainer.
    ///
    /// This is the exit-boundary counterpart to
    /// `apply_forced_break_before_box_in`; layout modes that carry descendant
    /// forced breaks should resolve the fallback through
    /// `FragmentBreakContext::forced_break_after_or_in` before calling the
    /// page transition primitive:
    /// <https://www.w3.org/TR/css-break-3/#forced-breaks>.
    pub(in crate::layout) fn apply_forced_break_after_box_in(
        &mut self,
        fragmentainer_kind: FragmentainerKind,
        style: &ComputedStyle,
    ) {
        if let Some(forced_break) = FragmentBreakContext::for_standalone_box(style)
            .forced_break_after_in(fragmentainer_kind)
        {
            // A forced break establishes a page boundary after this box even
            // when the box has no paint (for example, an empty `min-height`
            // block). Retain that completed fragmentainer during document
            // finalization; generic trailing geometry without a forced
            // boundary remains eligible for omission.
            // <https://www.w3.org/TR/css-break-3/#forced-breaks>
            if self.current_page_has_flow_content {
                self.current_page.mark_fragmentation_content();
            }
            // An out-of-flow-only source box can have no normal-flow cursor
            // effect, while its positioned paint is still owned by this page.
            // Commit that paint before deciding whether a forced side break
            // needs a new page; otherwise the break is treated as if it
            // occurred at document start and the positioned content is
            // replayed into a later sibling's page.
            // <https://www.w3.org/TR/css-break-3/#breaks-between>
            if fragmentainer_kind.materializes_page_cursor() && !self.current_page_has_content() {
                self.flush_positioned_layers();
            }
            self.apply_forced_break_in(fragmentainer_kind, forced_break);
        }
    }

    pub(in crate::layout) fn current_page_has_content(&self) -> bool {
        self.current_page.has_paint_content() || self.current_page_has_flow_content
    }

    pub(in crate::layout) fn active_fragmentainer_kind(&self) -> FragmentainerKind {
        self.fragmentainer_override
            .map(|override_| override_.kind)
            .unwrap_or(FragmentainerKind::Page)
    }

    /// Return whether a transition for `kind` has a concrete cursor-backed
    /// fragmentainer in the active layout scope.
    ///
    /// Page layout is always cursor-backed. The multicol engine also installs
    /// page-shaped anonymous column canvases, so column transitions inside
    /// that scope must materialize just like page transitions.
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn fragmentainer_materializes_cursor(
        &self,
        kind: FragmentainerKind,
    ) -> bool {
        kind.materializes_page_cursor()
            || self
                .fragmentainer_override
                .is_some_and(|override_| override_.kind == kind)
    }

    /// Marks the current page as carrying source-owned normal-flow content.
    ///
    /// CSS Fragmentation fragments boxes into page fragmentainers even when a
    /// particular fragment has no visible paint. A used border box with
    /// positive area, or a zero-size box placed after clearance in a new
    /// fragmentainer, therefore participates in forced breaks independently
    /// from PDF paint primitives. At document
    /// finalization, a trailing run with no paint or page-owning side effect
    /// can be omitted from static PDF output:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/css-box-3/#box-model>.
    pub(in crate::layout) fn mark_current_page_flow_content(&mut self) {
        self.current_page_has_flow_content = true;
        self.current_page_has_named_page_flow_content = true;
        // An explicit named-page assignment is observable even when its
        // normal-flow box contributes geometry but no paint: it selects the
        // page box, including its size and page rules. Preserve every
        // fragmentainer actually occupied by that named flow rather than
        // discarding it as an unpainted trailing geometry page.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        if self.current_page_name.is_some() {
            self.current_page.mark_fragmentation_content();
        }
    }
}

/// Returns whether a forced break target is satisfied by the next page number.
///
/// CSS Fragmentation defines `left`/`right` as spread sides and `recto`/`verso`
/// as first/opposite page sides in the current page progression:
/// <https://www.w3.org/TR/css-break-3/#valdef-break-before-recto> and
/// <https://www.w3.org/TR/css-page-3/#spread-pseudos>.
pub(in crate::layout) fn forced_break_satisfied(
    forced_break: PageBreak,
    next_page_number: usize,
    page_progression_direction: Direction,
) -> bool {
    let is_left = page_is_left(next_page_number, page_progression_direction);
    match forced_break {
        PageBreak::Auto
        | PageBreak::Avoid
        | PageBreak::AvoidPage
        | PageBreak::AvoidColumn
        | PageBreak::Page
        | PageBreak::Column => true,
        PageBreak::Left => is_left,
        PageBreak::Right => !is_left,
        PageBreak::Recto => is_recto_page(next_page_number, page_progression_direction),
        PageBreak::Verso => !is_recto_page(next_page_number, page_progression_direction),
    }
}

/// Returns whether a page is on the left side of the spread.
///
/// CSS Paged Media spread pseudo-classes follow the page progression direction:
/// <https://www.w3.org/TR/css-page-3/#spread-pseudos>.
pub(in crate::layout) fn page_is_left(
    page_number: usize,
    page_progression_direction: Direction,
) -> bool {
    match page_progression_direction {
        Direction::Ltr => page_number.is_multiple_of(2),
        Direction::Rtl => !page_number.is_multiple_of(2),
    }
}

/// Returns whether a page is the recto side for forced recto/verso breaks.
///
/// CSS Fragmentation maps `recto` to the first side of a spread in the current
/// page progression and `verso` to the opposite side:
/// <https://www.w3.org/TR/css-break-3/#valdef-break-before-recto>.
pub(in crate::layout) fn is_recto_page(
    page_number: usize,
    page_progression_direction: Direction,
) -> bool {
    match page_progression_direction {
        Direction::Ltr => !page_is_left(page_number, page_progression_direction),
        Direction::Rtl => page_is_left(page_number, page_progression_direction),
    }
}
