use super::*;
use crate::layout::block::suppress_fragmented_box_edges;
use crate::layout::builder::page_for_context;

pub(in crate::layout) struct PositionedPaginationState {
    pages: Vec<Page>,
    page_names: Vec<Option<String>>,
    page_blanks: Vec<bool>,
    page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    current_page: Page,
    current_page_has_flow_content: bool,
    current_page_has_named_page_flow_content: bool,
    current_page_selected_name: Option<String>,
    pub(in crate::layout) current_page_context: PageContext,
    current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    current_page_running_elements: HashMap<String, Vec<NamedStringAssignment>>,
    cursor_y: f32,
    content_left: f32,
    content_right: f32,
    fragment_top_offsets: Vec<f32>,
    truncate_page_start_margins: bool,
    pending_paint_fragments: Vec<PendingPaintFragment>,
    pending_page_side_effects: Vec<PendingPageSideEffects>,
    absolute_positioned_page_span_target: Option<usize>,
    pending_positioned_page_span_target: Option<usize>,
}

/// Retain page fragments established by nested out-of-flow layout when its
/// scratch pagination state is restored to the enclosing formatting context.
///
/// Each nested positioned subtree can extend the final document independently,
/// so neither requirement may replace the other.
/// <https://www.w3.org/TR/css-position-3/#fragmenting-absolutely-positioned-elements>
pub(in crate::layout) fn merged_positioned_page_span_target(
    enclosing: Option<usize>,
    nested: Option<usize>,
) -> Option<usize> {
    enclosing.into_iter().chain(nested).max()
}

impl<'a> LayoutBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn extend_positioned_principal_decoration_fragments(
        &mut self,
        fragments: &mut Vec<(usize, PaintFragment)>,
        style: &ComputedStyle,
        border_box: PaintClip,
        first_page_index: usize,
        captured_last_page_index: usize,
        target_page_index: usize,
        first_page_context: PageContext,
    ) {
        if target_page_index <= captured_last_page_index || style.visibility != Visibility::Visible
        {
            return;
        }
        let fragmentainer_height = first_page_context.area_height().max(1.0);
        let box_top = border_box.y() + border_box.height();
        let box_start_distance = (first_page_context.top() - box_top).max(0.0);
        let box_end_distance = box_start_distance + border_box.height();

        for page_index in captured_last_page_index + 1..=target_page_index {
            let page_distance =
                page_index.saturating_sub(first_page_index) as f32 * fragmentainer_height;
            let slice_start = box_start_distance.max(page_distance);
            let slice_end = box_end_distance.min(page_distance + fragmentainer_height);
            if slice_end <= slice_start + 0.01 {
                continue;
            }
            let slice_top = first_page_context.top() - (slice_start - page_distance);
            let slice_height = slice_end - slice_start;
            let owns_block_start = slice_start <= box_start_distance + 0.01;
            let owns_block_end = slice_end >= box_end_distance - 0.01;
            let mut fragment_style = style.clone();
            suppress_fragmented_box_edges(&mut fragment_style, owns_block_start, owns_block_end);
            let background = self.box_background_primitives(
                paint_space_rect(
                    border_box.x(),
                    slice_top - slice_height,
                    border_box.width(),
                    slice_height,
                ),
                &fragment_style,
            );
            let outline = self.box_outline_primitives(
                paint_space_rect(
                    border_box.x(),
                    slice_top - slice_height,
                    border_box.width(),
                    slice_height,
                ),
                &fragment_style,
            );
            if background.is_empty() && outline.is_empty() {
                continue;
            }
            if let Some((_, fragment)) = fragments
                .iter_mut()
                .find(|(fragment_page_index, _)| *fragment_page_index == page_index)
            {
                fragment.prepend_primitives_in_band(PaintBand::BackgroundBorder, background);
                fragment.append_primitives_in_band(PaintBand::Outline, outline);
            } else {
                let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
                fragment.prepend_primitives_in_band(PaintBand::BackgroundBorder, background);
                fragment.append_primitives_in_band(PaintBand::Outline, outline);
                fragments.push((page_index, fragment));
            }
        }
        fragments.sort_by_key(|(page_index, _)| *page_index);
    }

    pub(in crate::layout) fn layout_positioned_block_with_block_static_y_offset(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        static_y_offset: f32,
    ) {
        let previous = self.block_static_position_y_offset;
        self.block_static_position_y_offset = Some(static_y_offset);
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
        self.block_static_position_y_offset = previous;
    }

    /// Returns the last page index occupied by an absolutely positioned box.
    ///
    /// The margin-box span determines which page fragments an absolute box
    /// occupies. Its principal paint may be transparent, but the used box
    /// still establishes destination fragmentainers: fixed-position
    /// descendants must replay on them and later positioned descendants use
    /// their page-local containing blocks.
    ///
    /// CSS Positioned Layout makes absolutely positioned boxes out-of-flow;
    /// CSS Fragmentation permits their rendered fragments to cross
    /// fragmentainer boundaries:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn absolute_positioned_page_span_target(
        &self,
        style: &ComputedStyle,
        containing_block: ContainingBlock,
        positioned_y: PositionedAxis,
        vertical_border_width: f32,
        containing_block_origin_page_index: usize,
    ) -> Option<usize> {
        if style.position != Position::Absolute {
            return None;
        }
        let page_height = self.page_area_height().max(1.0);
        let margin_box_top = containing_block.top_y() - positioned_y.start;
        let margin_box_height = positioned_y.margin_start
            + positioned_y.size
            + style.padding.top
            + style.padding.bottom
            + vertical_border_width
            + positioned_y.margin_end;
        if margin_box_height <= 0.0 {
            return None;
        }
        // Size containment makes the principal box monolithic, but it does
        // not confine an oversized box's graphical representation to its
        // start fragmentainer. Its continuous margin-box extent therefore
        // still bounds every potential decoration slice.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        // <https://www.w3.org/TR/css-break-3/#monolithic>
        let margin_box_bottom = margin_box_top - margin_box_height.max(0.0);
        let distance_from_page_top = (self.page_top() - margin_box_bottom).max(0.0);
        if distance_from_page_top <= 0.0 {
            return None;
        }
        Some(
            containing_block_origin_page_index
                + ((distance_from_page_top - 0.01).max(0.0) / page_height).floor() as usize,
        )
    }

    pub(in crate::layout) fn absolute_positioned_page_start_offset(
        &self,
        containing_block: ContainingBlock,
        positioned_y: PositionedAxis,
    ) -> (usize, f32) {
        let page_height = self.page_area_height().max(1.0);
        let margin_box_top = containing_block.top_y() - positioned_y.start;
        let start_distance = (self.page_top() - margin_box_top).max(0.0);
        let page_offset = (start_distance / page_height).floor() as usize;
        (
            page_offset,
            (start_distance - page_offset as f32 * page_height).max(0.0),
        )
    }

    /// Records final document pages required by positioned paint or descendant layers.
    ///
    /// The positioned subtree is first laid out against scratch page state so
    /// descendant fragmentation can be harvested without advancing normal flow.
    /// Only non-empty paint fragments and positioned descendant layers extend
    /// the real page sequence; an empty absolute margin-box span does not:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn ensure_positioned_page_span(
        &mut self,
        target_page_index: Option<usize>,
    ) {
        let Some(target_page_index) = target_page_index else {
            return;
        };
        self.pending_positioned_page_span_target = Some(
            self.pending_positioned_page_span_target
                .map_or(target_page_index, |existing| {
                    existing.max(target_page_index)
                }),
        );
        // A positioned descendant is measured while its source inline run is
        // still selecting normal-flow fragment breaks. It may need a later
        // fragmentainer for its paint, but must not advance that source flow
        // or make its widow/orphan decision from the provisional destination.
        // The enclosing formatter materializes this retained span once it
        // has committed the in-flow break sequence.
        // <https://www.w3.org/TR/css-position-3/#absolute-positioning>
        // <https://www.w3.org/TR/css-break-3/#widows-orphans>
    }

    /// Retains an absolute box's logical fragmentainer span independently
    /// from its current paint. Viewport-fixed descendants replay against the
    /// final document sequence, so their retention cannot depend on whether
    /// the fixed layer appeared before or during this subtree.
    ///
    /// <https://www.w3.org/TR/css-position-3/#fixed-pos>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn retain_absolute_positioned_page_span(
        &mut self,
        target_page_index: Option<usize>,
    ) {
        let Some(target_page_index) = target_page_index else {
            return;
        };
        self.absolute_positioned_page_span_target = Some(
            self.absolute_positioned_page_span_target
                .map_or(target_page_index, |existing| {
                    existing.max(target_page_index)
                }),
        );
    }

    pub(in crate::layout) fn materialize_pending_positioned_page_span(&mut self) {
        if self.out_of_flow_prebreak_suppression_depth == 0 {
            let target_page_index = self
                .pending_positioned_page_span_target
                .take()
                .into_iter()
                .chain(self.positioned_layers.iter().map(|layer| layer.page_index))
                .chain(
                    (!self.fixed_layers.is_empty())
                        .then_some(self.absolute_positioned_page_span_target)
                        .flatten(),
                )
                .max();
            let Some(target_page_index) = target_page_index else {
                return;
            };
            while self.pages.len() < target_page_index {
                if !self.current_page_has_content() {
                    self.mark_current_page_flow_content();
                }
                self.push_page_without_flushing_positioned_layers();
            }
            if self.pages.len() == target_page_index {
                self.mark_current_page_flow_content();
            }
        }
    }

    pub(in crate::layout) fn push_page_without_flushing_positioned_layers(&mut self) {
        if !self.current_page_has_content() {
            self.mark_current_page_flow_content();
        }
        let offsets = self.current_fragment_offsets_for_page_break();
        // Positioned overflow must advance through the active fragmentainer
        // sequence without flushing layers that still belong to its containing
        // stacking context. In a multicol probe the next fragment is another
        // anonymous column box, not a document page.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
        // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>.
        let next_context = self
            .fragmentainer_override
            .map(|override_| override_.context_for_fragmentainer(self.pages.len() + 1))
            .unwrap_or_else(|| self.resolved_page_context(self.pages.len() + 2, false));
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

    pub(in crate::layout) fn remap_absolute_positioned_fragments(
        &self,
        fragments: &mut [(usize, PaintFragment)],
        scratch_start_page_index: usize,
        destination_start_page_index: usize,
    ) {
        // Positioned layout has already entered destination-page-local
        // coordinates before painting. Remapping therefore changes ownership
        // only, retaining each fragment's local geometry.
        // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
        for (page_index, _) in fragments {
            let relative_page_index = page_index.saturating_sub(scratch_start_page_index);
            *page_index = destination_start_page_index + relative_page_index;
        }
    }

    pub(in crate::layout) fn positioned_pagination_state(&self) -> PositionedPaginationState {
        PositionedPaginationState {
            pages: self.pages.clone(),
            page_names: self.page_names.clone(),
            page_blanks: self.page_blanks.clone(),
            page_named_strings: self.page_named_strings.clone(),
            page_running_elements: self.page_running_elements.clone(),
            current_page: self.current_page.clone(),
            current_page_has_flow_content: self.current_page_has_flow_content,
            current_page_has_named_page_flow_content: self.current_page_has_named_page_flow_content,
            current_page_selected_name: self.current_page_selected_name.clone(),
            current_page_context: self.current_page_context,
            current_page_named_strings: self.current_page_named_strings.clone(),
            current_page_running_elements: self.current_page_running_elements.clone(),
            cursor_y: self.cursor_y,
            content_left: self.content_left,
            content_right: self.content_right,
            fragment_top_offsets: self.fragment_top_offsets.clone(),
            truncate_page_start_margins: self.truncate_page_start_margins,
            pending_paint_fragments: self.pending_paint_fragments.clone(),
            pending_page_side_effects: self.pending_page_side_effects.clone(),
            absolute_positioned_page_span_target: self.absolute_positioned_page_span_target,
            pending_positioned_page_span_target: self.pending_positioned_page_span_target,
        }
    }

    pub(in crate::layout) fn restore_positioned_pagination_state(
        &mut self,
        state: PositionedPaginationState,
    ) {
        self.pages = state.pages;
        self.page_names = state.page_names;
        self.page_blanks = state.page_blanks;
        self.page_named_strings = state.page_named_strings;
        self.page_running_elements = state.page_running_elements;
        self.current_page = state.current_page;
        self.current_page_has_flow_content = state.current_page_has_flow_content;
        self.current_page_has_named_page_flow_content =
            state.current_page_has_named_page_flow_content;
        self.current_page_selected_name = state.current_page_selected_name;
        self.current_page_context = state.current_page_context;
        self.current_page_named_strings = state.current_page_named_strings;
        self.current_page_running_elements = state.current_page_running_elements;
        self.cursor_y = state.cursor_y;
        self.content_left = state.content_left;
        self.content_right = state.content_right;
        self.fragment_top_offsets = state.fragment_top_offsets;
        self.truncate_page_start_margins = state.truncate_page_start_margins;
        self.pending_paint_fragments = state.pending_paint_fragments;
        self.pending_page_side_effects = state.pending_page_side_effects;
        self.absolute_positioned_page_span_target = state.absolute_positioned_page_span_target;
        self.pending_positioned_page_span_target = state.pending_positioned_page_span_target;
    }

    /// Captures out-of-flow positioned paint fragments from every page touched by layout.
    ///
    /// CSS Positioned Layout takes absolutely positioned boxes out of normal
    /// flow, while CSS Fragmentation still allows their contents to split
    /// across page fragmentainers. Each produced page fragment must therefore
    /// be replayed in the positioned stacking level for that page, not left as
    /// normal-flow paint and not replayed as one page-local fragment:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn take_positioned_fragments_since(
        &mut self,
        paint_page_index: usize,
        paint_checkpoint: PaintCheckpoint,
    ) -> Vec<(usize, PaintFragment)> {
        if self.pages.len() == paint_page_index {
            return vec![(
                paint_page_index,
                self.current_page
                    .take_paint_fragment_since(paint_checkpoint),
            )];
        }

        let mut fragments = Vec::new();
        if let Some(page) = self.pages.get_mut(paint_page_index) {
            fragments.push((
                paint_page_index,
                page.take_paint_fragment_since(paint_checkpoint),
            ));
        }
        for page_index in paint_page_index + 1..self.pages.len() {
            let fragment = self.pages[page_index].take_paint_fragment();
            fragments.push((page_index, fragment));
        }
        fragments.push((self.pages.len(), self.current_page.take_paint_fragment()));
        fragments
    }
}
