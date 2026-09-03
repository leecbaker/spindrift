use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PageContext {
    pub(in crate::layout) size: PageSize,
    pub(in crate::layout) margins: PageMargins,
    pub(in crate::layout) edges: PageBoxEdges,
    pub(in crate::layout) rotation: i32,
}

/// Used page-box border and padding edges for the document page area.
///
/// CSS Paged Media makes page boxes follow the CSS box model: page margins
/// surround the page border, page padding is inside that border, and document
/// content is laid out in the page area/content box:
/// <https://www.w3.org/TR/css-page-3/#page-model> and
/// <https://www.w3.org/TR/css-box-3/#box-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PageBoxEdges {
    pub(in crate::layout) border: css::Edges,
    pub(in crate::layout) padding: css::Edges,
}

impl PageBoxEdges {
    pub(in crate::layout) const ZERO: Self = Self {
        border: css::Edges::ZERO,
        padding: css::Edges::ZERO,
    };

    pub(in crate::layout) fn left(self) -> f32 {
        self.border.left + self.padding.left
    }

    pub(in crate::layout) fn right(self) -> f32 {
        self.border.right + self.padding.right
    }

    pub(in crate::layout) fn top(self) -> f32 {
        self.border.top + self.padding.top
    }

    pub(in crate::layout) fn bottom(self) -> f32 {
        self.border.bottom + self.padding.bottom
    }

    pub(in crate::layout) fn total(self) -> css::Edges {
        css::Edges {
            top: self.top(),
            right: self.right(),
            bottom: self.bottom(),
            left: self.left(),
        }
    }
}

impl PageContext {
    pub(in crate::layout) fn from_options(options: &RenderOptions) -> Self {
        Self {
            size: options.page_size,
            margins: options.iframe_page_margins.unwrap_or(PageMargins::DEFAULT),
            edges: PageBoxEdges::ZERO,
            rotation: 0,
        }
    }

    pub(in crate::layout) fn left(self) -> f32 {
        self.margins.left() + self.edges.left()
    }

    pub(in crate::layout) fn right(self) -> f32 {
        self.size.width() - self.margins.right() - self.edges.right()
    }

    pub(in crate::layout) fn top(self) -> f32 {
        self.size.height() - self.margins.top() - self.edges.top()
    }

    pub(in crate::layout) fn bottom(self) -> f32 {
        self.margins.bottom() + self.edges.bottom()
    }

    pub(in crate::layout) fn area_width(self) -> f32 {
        (self.size.width()
            - self.margins.left()
            - self.margins.right()
            - self.edges.left()
            - self.edges.right())
        .max(0.0)
    }

    pub(in crate::layout) fn area_height(self) -> f32 {
        (self.size.height()
            - self.margins.top()
            - self.margins.bottom()
            - self.edges.top()
            - self.edges.bottom())
        .max(0.0)
    }

    /// Returns the physical page-area extent used as the logical inline size
    /// for a formatting context in `writing_mode`.
    ///
    /// Page boxes are physical rectangles, while a formatting context's
    /// percentage and fragmentation bases are logical. Keeping this mapping
    /// on the page context makes the initial page area explicit rather than
    /// letting vertical flows accidentally inherit the physical width basis:
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(in crate::layout) fn logical_inline_size(self, writing_mode: WritingMode) -> f32 {
        if WritingModeAxes::new(writing_mode, Direction::Ltr).swaps_physical_axes() {
            self.area_height()
        } else {
            self.area_width()
        }
    }

    /// Returns the physical page-area extent used as the logical block size
    /// for a formatting context in `writing_mode`.
    ///
    /// This is deliberately separate from `area_height()`: in a vertical or
    /// sideways flow, page fragmentation progresses across physical width.
    /// <https://www.w3.org/TR/css-writing-modes-4/#block-flow>.
    pub(in crate::layout) fn logical_block_size(self, writing_mode: WritingMode) -> f32 {
        if WritingModeAxes::new(writing_mode, Direction::Ltr).swaps_physical_axes() {
            self.area_width()
        } else {
            self.area_height()
        }
    }
}

pub(in crate::layout) fn page_for_context(context: PageContext) -> Page {
    let mut page = Page::new(context.size.width(), context.size.height());
    page.rotation = context.rotation;
    page
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn page_left(&self) -> f32 {
        self.current_page_context.left()
    }

    pub(in crate::layout) fn page_top(&self) -> f32 {
        self.current_page_context.top()
    }

    /// Whether the active traversal resolves an automatic positioned block
    /// size before the positioned box has been fragmented.
    pub(in crate::layout) fn is_positioned_auto_size_measurement(&self) -> bool {
        self.layout_pass_kind == LayoutPassKind::PositionedAutoSizeMeasurement
    }

    pub(in crate::layout) fn page_bottom(&self) -> f32 {
        if self.fragmentation_suppression_depth > 0 || self.footnote_measurement_depth > 0 {
            self.current_page_context.bottom() - 1_000_000.0
        } else {
            self.current_page_context.bottom()
                + self
                    .footnote_reservations
                    .get(&self.pages.len())
                    .copied()
                    .unwrap_or(0.0)
                // Every active cloned block owns its block-end padding and
                // border in this fragmentainer. Keep that reservation in the
                // layout capacity so descendants cannot consume the space
                // that its principal-box decoration must occupy.
                // <https://www.w3.org/TR/css-break-3/#box-model-for-breaking>
                + self
                    .fragment_top_offsets
                    .iter()
                    .map(|offset| offset.continuation_end())
                    .sum::<f32>()
        }
    }

    pub(in crate::layout) fn page_area_width(&self) -> f32 {
        self.current_page_context.area_width()
    }

    pub(in crate::layout) fn page_area_height(&self) -> f32 {
        self.page_top() - self.page_bottom()
    }

    /// The physical block-axis size of the document initial containing block.
    ///
    /// This is the immutable initial printable page area, not the remaining
    /// extent of the current fragmentainer. Orthogonal-flow line fitting falls
    /// back to this size after direct and scroll-container candidates have
    /// been exhausted.
    /// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
    pub(in crate::layout) fn initial_containing_block_physical_height(
        &self,
    ) -> PhysicalContentHeight {
        PhysicalContentHeight::new(content_box_pt(self.initial_viewport_context.area_height()))
    }

    pub(in crate::layout) fn current_content_logical_inline_size(&self) -> f32 {
        self.content_logical_inline_size_stack
            .last()
            .cloned()
            .unwrap_or_else(|| (self.content_right - self.content_left).max(0.0))
    }

    /// Return the active containing block's logical inline content-box size.
    ///
    /// The stack is still scalar while legacy inline collection is migrated,
    /// but consumers resolving CSS percentage edges must cross through this
    /// typed boundary rather than treating the value as a physical width.
    pub(in crate::layout) fn current_content_logical_inline_content_size(
        &self,
    ) -> LogicalInlineContentSize {
        LogicalInlineContentSize::new(content_box_pt(self.current_content_logical_inline_size()))
    }

    /// Select the available logical inline measure for a descendant flow.
    ///
    /// Parallel writing modes inherit the containing formatting context's
    /// already-selected logical inline measure, even when its corresponding
    /// physical height is automatic and therefore not a percentage basis.
    /// Only an orthogonal descendant consults the physical-axis fallback
    /// policy carried by [`ChildAvailableSpace`].
    /// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
    pub(in crate::layout) fn current_available_logical_inline_size_for(
        &self,
        writing_mode: WritingMode,
    ) -> LogicalInlineContentSize {
        let containing_space = self.current_child_available_space();
        if WritingModeAxes::new(containing_space.writing_mode, Direction::Ltr).swaps_physical_axes()
            == WritingModeAxes::new(writing_mode, Direction::Ltr).swaps_physical_axes()
        {
            self.current_content_logical_inline_content_size()
        } else {
            containing_space.logical_inline_size_for(writing_mode)
        }
    }

    /// Return the active containing block's definite logical inline basis for
    /// CSS edge-percentage resolution.
    pub(in crate::layout) fn current_content_logical_inline_percentage_basis(
        &self,
    ) -> LogicalInlinePercentageBasis {
        PercentageBasis::definite(self.current_content_logical_inline_content_size())
    }

    pub(in crate::layout) fn page_child_available_space(&self) -> ChildAvailableSpace {
        ChildAvailableSpace::new(
            // The initial containing block takes the principal writing mode
            // from the document root. Its physical dimensions remain the page
            // area's dimensions, but treating a vertical root as orthogonal
            // would incorrectly shrink its auto inline size to its contents.
            // https://www.w3.org/TR/css-writing-modes-4/#principal-flow
            self.initial_containing_block_writing_mode,
            PhysicalContentWidth::new(content_box_pt(self.page_area_width())),
            true,
            Some(PhysicalContentHeight::new(content_box_pt(
                self.page_area_height(),
            ))),
            self.initial_containing_block_physical_height(),
        )
    }

    pub(in crate::layout) fn current_child_available_space(&self) -> ChildAvailableSpace {
        self.child_available_space_stack
            .last()
            .cloned()
            .unwrap_or_else(|| self.page_child_available_space())
    }

    pub(in crate::layout) fn resolved_page_context(
        &mut self,
        page_number: usize,
        is_blank: bool,
    ) -> PageContext {
        let page_name = self.current_page_name.clone();
        self.resolved_page_context_for_name(page_number, is_blank, page_name.as_deref())
    }

    /// Convert a scratch-local 1-based page ordinal into the page number of
    /// its eventual document destination. Normal flow has no scratch origin,
    /// so its ordinal is already the document page number.
    /// <https://drafts.csswg.org/css-position-3/#fragmenting-abspos>
    pub(in crate::layout) fn destination_document_page_number(
        &self,
        local_page_number: usize,
    ) -> usize {
        self.positioned_scratch_page_origin
            .map_or(local_page_number, |origin| origin.get() + local_page_number)
    }

    /// Resolves a concrete destination page context without changing the page
    /// type of the source page currently being committed.
    ///
    /// A named-page transition selects its destination before it materializes
    /// the new page box, while the previous page retains its existing named
    /// type for `@page` matching and final decoration.
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>
    pub(in crate::layout) fn resolved_page_context_for_name(
        &mut self,
        page_number: usize,
        is_blank: bool,
        page_name: Option<&str>,
    ) -> PageContext {
        let declarations = self.page_declarations_for_page(page_number, page_name, is_blank);
        let base = PageContext::from_options(self.options);
        let page_style = self.page_context_style_for_declarations(&declarations);
        let ch_advance = self.ch_advance_for_style(&page_style, page_style.requires_ch_advance());
        // The first empty page context is needed before the document root is
        // traversed, so root-relative page lengths cannot yet use the selected
        // root font. Bootstrap with the page style's initial metric estimates;
        // `layout_dom_with_font_system` rebuilds this still-empty context as
        // soon as document-root metrics have been established.
        // <https://www.w3.org/TR/css-values-4/#root-relative-fonts>
        let root_metrics =
            self.root_metric_state
                .font_size_basis()
                .unwrap_or(css::RootFontMetricLengthBasis {
                    font_size: layout_pt(page_style.font_size),
                    ch_advance,
                    x_height: layout_pt(page_style.font_size * 0.5),
                    cap_height: layout_pt(page_style.font_size * 0.7),
                    ic_advance: ch_advance,
                    line_height: layout_pt(page_style.line_height),
                });
        // CSS Paged Media defines page size and page margins in the page
        // context; these declarations select the page box before its content
        // area is used for layout.
        // https://www.w3.org/TR/css-page-3/#page-model
        let size = css::page_size_from_with_ch_advance_and_root_metrics(
            &declarations,
            base.size,
            ch_advance,
            root_metrics,
        );
        let page_edges = page_box_edges_from_declarations_with_ch_advance_and_root_metrics(
            &declarations,
            size,
            ch_advance,
            root_metrics,
        );
        PageContext {
            size,
            margins:
                css::page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style_and_root_metrics(
                    &declarations,
                    base.margins,
                    size,
                    css::PageMarginResolutionContext {
                        viewport_size: self.page_descriptor_viewport_size,
                        non_margin_edges: page_edges.total(),
                        ch_advance,
                        style: &page_style,
                        root_metrics,
                    },
                ),
            edges: page_edges,
            rotation: css::page_rotation_from(&declarations, base.rotation),
        }
    }

    pub(in crate::layout) fn finished_page_context(
        &mut self,
        page_number: usize,
        page_size: PageSize,
    ) -> PageContext {
        let page_name = self.page_name_for_number(page_number);
        let is_blank = self.page_is_blank_for_number(page_number);
        let declarations = self.page_declarations_for_page(page_number, page_name, is_blank);
        let base = PageContext::from_options(self.options);
        let page_style = self.page_context_style_for_declarations(&declarations);
        let ch_advance = self.ch_advance_for_style(&page_style, page_style.requires_ch_advance());
        let root_metrics = self.root_metric_state.resolved().basis();
        let page_edges = page_box_edges_from_declarations_with_ch_advance_and_root_metrics(
            &declarations,
            page_size,
            ch_advance,
            root_metrics,
        );
        PageContext {
            size: page_size,
            margins:
                css::page_margins_from_for_size_and_edges_with_ch_advance_and_page_context_style_and_root_metrics(
                    &declarations,
                    base.margins,
                    page_size,
                    css::PageMarginResolutionContext {
                        viewport_size: self.page_descriptor_viewport_size,
                        non_margin_edges: page_edges.total(),
                        ch_advance,
                        style: &page_style,
                        root_metrics,
                    },
                ),
            edges: page_edges,
            rotation: css::page_rotation_from(&declarations, base.rotation),
        }
    }

    /// Builds the inherited page context used for logical page properties.
    pub(in crate::layout) fn page_context_style_for_declarations(
        &self,
        declarations: &Declarations,
    ) -> ComputedStyle {
        let mut style = self.page_margin_inherited_style.clone();
        css::apply_declarations_with_inheritance_source(
            &mut style,
            declarations,
            &self.page_margin_inherited_style,
        );
        style
    }

    pub(in crate::layout) fn rebuild_empty_current_page_context(&mut self) {
        if self.current_page_has_content() {
            return;
        }
        let mut offsets = if self.pages.is_empty() {
            // Before the first page is materialized, a descendant may select
            // its named page from inside an ancestor's first fragment. Keep
            // that ancestor's initial block-start inset.
            self.current_fragment_offsets()
        } else {
            // A named-page selection before its first in-flow descendant is
            // laid out establishes a new page-area containing block. Retain
            // the ancestor offsets while rebuilding that empty context so the
            // document root/body margin is not lost merely because the page
            // name changed.
            // https://www.w3.org/TR/css-page-3/#using-named-pages
            self.current_fragment_offsets()
        };
        // Page-context replacement measures the current content edge against
        // the old page area's edge. When that old page has a larger margin,
        // that transient measurement is negative even though the active
        // ancestor inset (for example the body's used margin) is positive.
        // Fragment insets are distances, so retain their magnitude across
        // the page-area change.
        offsets.left = offsets.left.abs();
        offsets.right = offsets.right.abs();
        // The document canvas' block-start inset is intentionally removed
        // from normal fragment accounting, because it is not cloned at an
        // ordinary continuation. A named-page replacement is different: the
        // same root/body fragment continues in a new page area, so preserve
        // its used block-start margin.
        offsets.top += self
            .document_canvas_fragment_insets
            .iter()
            .map(|inset| inset.top)
            .sum::<f32>();
        let page_number = self.destination_document_page_number(self.pages.len() + 1);
        let context = self.resolved_page_context(page_number, false);
        self.current_page = page_for_context(context);
        self.apply_page_context(context, offsets);
        self.current_page_selected_name = self.current_page_name.clone();
        // A first in-flow box can select a named page before it emits any
        // content. CSS viewport units use that first actual page's initial
        // containing block, not the renderer's provisional default page.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        if self.pages.is_empty() {
            self.initial_viewport_context = context;
        }
    }

    /// Selects a named page type for an already-committed, otherwise empty
    /// destination page.
    ///
    /// A forced break can materialize its destination before the succeeding
    /// class-A box supplies its page value. That page is not a first-page
    /// replacement: it is a continuation fragment, so its root/body canvas
    /// origin must remain the one installed by the forced break. Rebuilding
    /// it through the initial-page path would add that inset again.
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>
    pub(in crate::layout) fn select_named_page_for_committed_empty_page(&mut self) {
        debug_assert!(!self.pages.is_empty());
        debug_assert!(!self.current_page_has_content());

        let previous_context = self.current_page_context;
        let offsets = FragmentOffsets {
            left: self.content_left - previous_context.left(),
            right: previous_context.right() - self.content_right,
            top: previous_context.top() - self.cursor_y,
        };
        let page_name = self.current_page_name.clone();
        let context = self.resolved_page_context_for_name(
            self.destination_document_page_number(self.pages.len() + 1),
            false,
            page_name.as_deref(),
        );
        self.current_page = page_for_context(context);
        self.apply_page_context(context, offsets);
        self.current_page_selected_name = self.current_page_name.clone();
    }

    pub(in crate::layout) fn has_renderable_content(&self) -> bool {
        !self.pages.is_empty()
            || self.current_page_has_content()
            || !self.positioned_layers.is_empty()
            || !self.fixed_layers.is_empty()
            || self
                .page_rules
                .iter()
                .any(|rule| !rule.margin_boxes.is_empty())
    }

    pub(in crate::layout) fn cursor_is_at_page_top(&self) -> bool {
        (self.cursor_y - self.page_top()).abs() < 0.01
    }

    /// Resolves a captured assignment to the first source fragment's final page.
    ///
    /// CSS GCPM `start` lookups are based on the source fragment at the page
    /// boundary, not on the earlier style/counter capture point. If layout
    /// pushes a page after capture, the original page checkpoint tells whether
    /// the source painted there or moved wholly to the new current page:
    /// <https://www.w3.org/TR/css-gcpm-3/#named-strings>.
    pub(in crate::layout) fn final_source_assignment_placement(
        &self,
        style: &ComputedStyle,
        captured_page_index: usize,
        captured_paint_checkpoint: PaintCheckpoint,
        captured_starts_page_fragment: bool,
        captured_content_left: f32,
        captured_cursor_y: f32,
    ) -> AssignmentPlacement {
        let height = style.line_height.max(0.0);
        let width = (self.content_right - self.content_left).max(0.0);
        if captured_page_index < self.pages.len() {
            let original_page_changed =
                self.pages[captured_page_index].paint_checkpoint() != captured_paint_checkpoint;
            if original_page_changed {
                return AssignmentPlacement {
                    page_index: captured_page_index,
                    starts_page_fragment: captured_starts_page_fragment,
                    border_box: Some(
                        PageTopRect::new(captured_content_left, captured_cursor_y, width, height)
                            .paint_clip(),
                    ),
                };
            }
            return AssignmentPlacement {
                page_index: self.pages.len(),
                starts_page_fragment: true,
                border_box: Some(
                    PageTopRect::new(self.content_left, self.page_top(), width, height)
                        .paint_clip(),
                ),
            };
        }
        AssignmentPlacement {
            page_index: captured_page_index,
            starts_page_fragment: captured_starts_page_fragment,
            border_box: Some(
                PageTopRect::new(captured_content_left, captured_cursor_y, width, height)
                    .paint_clip(),
            ),
        }
    }
}
