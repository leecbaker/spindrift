use super::*;
use crate::layout::inline_collect::TextDecorationPropagationContext;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_formatting_box(
        &mut self,
        formatting_box: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
    ) {
        self.layout_formatting_box_with_parent_decoration(formatting_box, stylesheets, None);
    }

    /// Lay out a frozen formatting box with the decoration origins propagated
    /// by its in-flow parent.
    ///
    /// Frozen box trees retain computed styles, so normal CSS inheritance
    /// cannot carry line-decoration provenance across this boundary. Resolve
    /// the layout-only propagation context here before dispatching the box's
    /// formatting algorithm.
    /// <https://drafts.csswg.org/css-text-decor-4/#line-decoration>
    pub(in crate::layout) fn layout_formatting_box_with_parent_decoration(
        &mut self,
        formatting_box: &box_tree::FormattingBox<'_>,
        stylesheets: &Stylesheets<'_>,
        parent_style: Option<&ComputedStyle>,
    ) {
        let decoration_context = parent_style
            .map(TextDecorationPropagationContext::from_style)
            .unwrap_or_default();
        match formatting_box {
            box_tree::FormattingBox::Block(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                // The document box can recur through an anonymous root-flow
                // wrapper before it reaches its own descendants. Keep the
                // stored style computed, but apply the principal-flow axes at
                // every formatting entry for that one principal box.
                let layout_style = box_
                    .core
                    .element
                    .tag
                    .eq_ignore_ascii_case("html")
                    .then_some(())
                    .filter(|_| matches!(&box_.core.source, box_tree::BoxSource::Principal))
                    .map(|_| self.principal_flow.root_layout_style(&used_style));
                self.layout_element_box(
                    box_.core.element,
                    layout_style.as_ref().unwrap_or(&used_style),
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &box_.run_in_children,
                    &box_.core.children,
                );
            }
            box_tree::FormattingBox::Inline(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_element_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &[],
                    &box_.core.children,
                )
            }
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                let used_style = decoration_context.used_child_style(&box_.style);
                self.layout_anonymous_block(&used_style, &box_.children, stylesheets, None);
            }
            box_tree::FormattingBox::InlineSplitBlockContext(box_) => self
                .layout_inline_split_block_context_with_parent_decoration(
                    box_,
                    stylesheets,
                    parent_style,
                ),
            box_tree::FormattingBox::AtomicInline(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_element_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &[],
                    &box_.core.children,
                )
            }
            box_tree::FormattingBox::Table(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_table_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &box_.core.children,
                    &box_.fragment,
                );
            }
            box_tree::FormattingBox::Flex(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_element_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &[],
                    &box_.core.children,
                )
            }
            box_tree::FormattingBox::Replaced(box_) => {
                let used_style = decoration_context.used_child_style(&box_.core.style);
                self.layout_element_box(
                    box_.core.element,
                    &used_style,
                    stylesheets,
                    box_.core.signature.clone(),
                    &box_.core.source,
                    &[],
                    &box_.core.children,
                )
            }
            box_tree::FormattingBox::Text(box_) => {
                let used_style = decoration_context.used_child_style(&box_.style);
                let text = normalized_text_for_style(&box_.text, &used_style);
                if !text.is_empty() {
                    self.layout_text_block(&text, &used_style, 0.0, 0.0, None);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_element_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        signature: ElementSignature,
        source: &box_tree::BoxSource<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        children: &[box_tree::FormattingBox<'_>],
    ) {
        self.push_ancestor_signature(signature);
        match source {
            box_tree::BoxSource::Principal => {
                self.capture_suppressed_named_strings_before(element.id);
                self.layout_element_with_child_boxes_and_run_ins(
                    element,
                    style,
                    stylesheets,
                    run_in_children,
                    Some(children),
                );
                self.capture_suppressed_named_strings_after(element.id);
            }
            box_tree::BoxSource::GeneratedPseudo(pseudo) => {
                self.layout_generated_pseudo_box(
                    element,
                    style,
                    pseudo.kind.counter_event_source(),
                    stylesheets,
                    run_in_children,
                    Some(children),
                    None,
                    PrincipalBoxPaintMode::RootPaints,
                );
            }
        }
        self.ancestors.pop();
    }

    /// Lays out a table formatting box through the generic element entry path.
    ///
    /// CSS Paged Media applies the `page` property to normal-flow boxes before
    /// their page context is generated, and CSS Tables uses a table wrapper/grid
    /// fragment for layout. This preserves the prebuilt durable table fragment
    /// while still applying named-page, counter, running-element, and
    /// break-inside entry behavior:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/CSS22/tables.html#model>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_table_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        signature: ElementSignature,
        source: &box_tree::BoxSource<'_>,
        children: &[box_tree::FormattingBox<'_>],
        fragment: &box_tree::TableFragment<'_>,
    ) {
        self.push_ancestor_signature(signature);
        match source {
            box_tree::BoxSource::Principal => {
                self.capture_suppressed_named_strings_before(element.id);
                self.layout_element_with_child_boxes_run_ins_and_table_fragment(
                    element,
                    style,
                    stylesheets,
                    &[],
                    Some(children),
                    Some(fragment),
                );
                self.capture_suppressed_named_strings_after(element.id);
            }
            box_tree::BoxSource::GeneratedPseudo(pseudo) => {
                self.layout_generated_pseudo_box(
                    element,
                    style,
                    pseudo.kind.counter_event_source(),
                    stylesheets,
                    &[],
                    Some(children),
                    Some(fragment),
                    PrincipalBoxPaintMode::RootPaints,
                );
            }
        }
        self.ancestors.pop();
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_generated_pseudo_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        source: box_tree::CounterEventSource,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        let counter_scope = self.begin_pseudo_counter_scope(element, source, style);
        self.element_side_effect_suppression_depth += 1;
        // A sole image in a tree-abiding ::before/::after box is anonymous
        // replaced content inside that pseudo's decorated box. Keep the
        // pseudo's authored dimensions for its own background/border while
        // the image payload retains its zoomed natural size. Principal
        // `content: <image>` remains a replacement of the element itself.
        // <https://www.w3.org/TR/css-content-3/#content-property>
        let mut pseudo_content_style;
        let style = if matches!(
            style.content,
            css::Content::Replacement {
                image: css::GeneratedContentPart::Image { .. },
                ..
            }
        ) {
            pseudo_content_style = style.clone();
            pseudo_content_style.object_fit = css::ObjectFit::None;
            pseudo_content_style.object_position = css::BackgroundPosition::INITIAL;
            &pseudo_content_style
        } else {
            style
        };
        let consuming_root_canvas =
            !style.display.is_block_level() && self.begin_root_inline_canvas_continuation(element);
        let previous_root_pseudo_block_projection = self.root_pseudo_block_projection;
        let root_before_principal_track_start = (element.tag.eq_ignore_ascii_case("html")
            && source == box_tree::CounterEventSource::Before
            && style.writing_mode == WritingMode::HorizontalTb
            && self.principal_flow.writing_mode == WritingMode::VerticalLr)
            .then_some(self.content_left);
        if element.tag.eq_ignore_ascii_case("html") {
            self.root_pseudo_block_projection =
                match (style.writing_mode, self.principal_flow.writing_mode) {
                    // A root pseudo retains its horizontal computed style, but
                    // a propagated vertical-lr body establishes the initial
                    // containing block's used principal flow. Project this one
                    // direct root child through that flow so the ordinary child
                    // traversal can advance the horizontal block track before
                    // entering the body.
                    // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
                    (WritingMode::HorizontalTb, WritingMode::VerticalLr)
                        if source == box_tree::CounterEventSource::Before =>
                    {
                        Some(RootPseudoBlockProjection {
                            element: element.id,
                            block_start: PhysicalSide::Left,
                            block_end_inset: layout_pt(0.0),
                        })
                    }
                    // The inverse projection retains the propagated body's
                    // physical block-end canvas inset while a vertical root
                    // pseudo participates in a horizontal principal flow.
                    (WritingMode::VerticalLr, WritingMode::HorizontalTb) => {
                        Some(RootPseudoBlockProjection {
                            element: element.id,
                            block_start: PhysicalSide::Left,
                            block_end_inset: self.principal_body_block_end_inset,
                        })
                    }
                    _ => None,
                };
        }
        self.layout_element_inner_with_principal_effect_context(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            true,
            principal_box_paint_mode,
            None,
        );
        if element.tag.eq_ignore_ascii_case("html")
            && source == box_tree::CounterEventSource::Before
            && style.writing_mode == WritingMode::HorizontalTb
            && self.principal_flow.writing_mode == WritingMode::VerticalLr
        {
            // The generated root pseudo is laid out directly rather than as a
            // normal child traversal entry. It therefore must explicitly
            // consume its committed margin-box span from the propagated
            // body's horizontal track. The outcome span already includes the
            // projected logical block-end margin; adding a physical margin
            // here would count the horizontal pseudo's margin twice.
            // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
            let advance = self
                .last_block_layout_outcome
                .physical_border_box_inline_span
                .points();
            self.content_left = (root_before_principal_track_start
                .expect("the vertical principal-track start was captured")
                + advance)
                .min(self.content_right);
        }
        if consuming_root_canvas {
            self.finish_root_inline_canvas_continuation();
        }
        self.root_pseudo_block_projection = previous_root_pseudo_block_projection;
        self.element_side_effect_suppression_depth -= 1;
        self.end_counter_scope(counter_scope);
    }

    /// Resolves a propagated body's completed document canvas immediately
    /// before the next source-ordered root inline sequence is laid out.
    ///
    /// This is a layout transition, rather than a paint-fragment adjustment:
    /// the source page is already committed by the body traversal when this
    /// method is reached.
    /// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) fn begin_root_inline_canvas_continuation(
        &mut self,
        element: &Element,
    ) -> bool {
        if !element.tag.eq_ignore_ascii_case("html")
            || !self.principal_flow.has_propagated_body()
            || self
                .root_principal_flow_context
                .active_root_inline_canvas
                .is_some()
        {
            return false;
        }
        let axes = WritingModeAxes::new(
            self.principal_flow.writing_mode,
            self.principal_flow.used_direction(),
        );
        if !axes.swaps_physical_axes() {
            return false;
        }
        let Some(continuation) = self.root_principal_flow_context.completed_canvas.take() else {
            return false;
        };
        debug_assert_eq!(continuation.source_page.get(), self.pages.len());
        let placement = continuation.resolve_root_inline_placement(
            axes,
            PageInlineSpan::from_edges(self.content_left, self.content_right),
        );
        match placement {
            RootInlineCanvasPlacement::RemainingTrack {
                block_track,
                inline_origin,
            } => {
                self.content_left = block_track.left_x();
                self.content_right = block_track.right_x();
                self.cursor_y = inline_origin.points();
            }
            RootInlineCanvasPlacement::NextPage { .. } => {
                // The preceding body has completed its page-owned canvas
                // before this root sequence begins. Mark that normal-flow
                // occupancy so page finalization cannot coalesce the source
                // page away, then establish the destination inline origin
                // before line construction starts.
                self.mark_current_page_flow_content();
                self.push_page();
                self.cursor_y = match inline_start_side(
                    self.principal_flow.writing_mode,
                    self.principal_flow.used_direction(),
                ) {
                    PhysicalSide::Top => self.page_top(),
                    // A bottom-origin principal flow reaches the following
                    // page at the body canvas's inline end. Its inset lies
                    // beyond the new fragmentainer's physical bottom, so the
                    // next root inline sequence is clipped there by ordinary
                    // line layout rather than replaying a translated paint
                    // fragment from the source page.
                    PhysicalSide::Bottom => {
                        self.page_bottom() - continuation.inline_end_inset.points()
                    }
                    PhysicalSide::Left | PhysicalSide::Right => {
                        unreachable!("a vertical principal flow has a vertical inline axis")
                    }
                };
            }
        }
        self.root_principal_flow_context.active_root_inline_canvas = Some(continuation);
        true
    }

    /// Completes the root inline sequence that consumed the propagated body
    /// continuation. The state remains live through line layout so nested
    /// paint and pagination paths cannot observe a partially consumed canvas.
    pub(in crate::layout) fn finish_root_inline_canvas_continuation(&mut self) {
        debug_assert!(
            self.root_principal_flow_context
                .active_root_inline_canvas
                .is_some()
        );
        self.root_principal_flow_context.active_root_inline_canvas = None;
    }
}
