use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
    ) {
        self.layout_element_with_child_boxes(element, style, stylesheets, None);
    }

    pub(in crate::layout) fn layout_element_with_child_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        self.layout_element_with_child_boxes_and_run_ins(
            element,
            style,
            stylesheets,
            &[],
            child_boxes,
        );
    }

    pub(in crate::layout) fn layout_element_with_child_boxes_and_run_ins(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) {
        self.layout_element_with_child_boxes_run_ins_and_table_fragment(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            None,
        );
    }

    pub(in crate::layout) fn layout_element_with_child_boxes_and_table_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        self.layout_element_with_child_boxes_run_ins_and_table_fragment(
            element,
            style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
        );
    }

    pub(in crate::layout) fn layout_element_with_child_boxes_run_ins_and_table_fragment(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        self.layout_element_with_child_boxes_run_ins_and_table_fragment_with_principal_effect_context(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            true,
            PrincipalBoxPaintMode::RootPaints,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn layout_element_with_child_boxes_run_ins_and_table_fragment_with_principal_effect_context(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        capture_principal_effect_context: bool,
        principal_box_paint_mode: PrincipalBoxPaintMode,
    ) {
        debug_assert_ne!(
            style.float.layout_role(),
            FloatLayoutRole::Footnote,
            "principal-flow dispatch must retain only a footnote call; detached bodies use a Float::None layout style"
        );
        // Most formatting contexts dispatch principal children directly to
        // this common boundary instead of through `layout_element_box`.
        // Consume any non-painting GCPM source event at that same source-order
        // point before a following child's break/page selection is applied.
        self.capture_suppressed_named_strings_before(element.id);
        self.push_page_value_scope(style);
        let page_name_scope = self.enter_page_name_scope(element, style, child_boxes);
        // The common element-dispatch boundary is also used while a
        // multicolumn container lays out its temporary column fragmentainers.
        // Break selection must therefore use the active fragmentation context
        // rather than assuming the outer paged-media page:
        // <https://www.w3.org/TR/css-break-3/#break-types>.
        let fragmentainer_kind = self.active_fragmentainer_kind();
        if self.should_prebreak_avoid_inside(
            element,
            style,
            stylesheets,
            child_boxes,
            fragmentainer_kind,
        ) {
            // Prebreaking before an avoid-kept subtree is a real box
            // fragmentation boundary. Preserve the same destination-local
            // containing-block geometry as the later speculative retry path;
            // raw `push_page` offsets otherwise retain the prior fragment's
            // root/body canvas translation for tables and other nested BFCs.
            // <https://www.w3.org/TR/css-break-3/#box-splitting>
            let continuation = (fragmentainer_kind == FragmentainerKind::Page)
                .then(|| self.block_page_break_continuation_context());
            let source_page_count = self.pages.len();
            self.push_page_if_nonempty();
            if self.pages.len() != source_page_count
                && let Some(continuation) = continuation
            {
                self.replay_fragment_continuation_on_page(&continuation, self.current_page_context);
            }
        }
        let mut layout_style;
        let box_break_context = FragmentBreakContext::for_standalone_box(style);
        let style = if !style.display.is_none()
            && let Some(forced_break_before) =
                box_break_context.forced_break_before_in(fragmentainer_kind)
        {
            // CSS Fragmentation places forced `break-before` before the
            // generated box. Counters, named strings, and running elements must
            // therefore observe the post-break page assignment rather than the
            // previous fragmentainer:
            // https://www.w3.org/TR/css-break-3/#break-between
            self.apply_forced_break_in(fragmentainer_kind, forced_break_before);
            layout_style = style.clone();
            layout_style.break_before = PageBreak::Auto;
            &layout_style
        } else {
            style
        };
        let counter_scope =
            (!style.display.is_none()).then(|| self.begin_counter_scope(element, style));
        let source_page_index = self.pages.len();
        let source_paint_checkpoint = self.current_page.paint_checkpoint();
        let source_starts_page_fragment = !self.current_page_has_content();
        let source_content_left = self.content_left;
        let source_cursor_y = self.cursor_y;
        if !style.display.is_none() {
            let named_assignment_ids = self.capture_named_strings(element, style);
            if self.capture_running_element(element, style) {
                // `position: running()` removes the flex item before normal
                // element dispatch, so it must consume the replay item's
                // one-shot percentage basis here instead of leaving it armed
                // for the next sibling.
                // <https://www.w3.org/TR/css-gcpm-3/#running-elements>
                let _ = self.take_replayed_flex_item_percentage_height_basis();
                let placement = AssignmentPlacement {
                    page_index: source_page_index,
                    starts_page_fragment: source_starts_page_fragment,
                    border_box: Some(PaintClip::from_paint_rect(paint_space_rect(
                        source_content_left,
                        source_cursor_y,
                        0.0,
                        0.0,
                    ))),
                };
                self.update_named_assignment_placements(&named_assignment_ids, placement);
                if let Some(counter_scope) = counter_scope {
                    self.end_counter_scope(counter_scope);
                }
                self.capture_suppressed_named_strings_after(element.id);
                self.pop_page_value_scope();
                self.exit_page_name_scope(page_name_scope);
                return;
            }
            if self.should_try_avoid_break_inside(style, fragmentainer_kind) {
                self.layout_avoiding_break_inside(
                    element,
                    style,
                    stylesheets,
                    run_in_children,
                    child_boxes,
                    table_fragment,
                    principal_box_paint_mode,
                );
                let placement = self.final_source_assignment_placement(
                    style,
                    source_page_index,
                    source_paint_checkpoint,
                    source_starts_page_fragment,
                    source_content_left,
                    source_cursor_y,
                );
                self.update_named_assignment_placements(&named_assignment_ids, placement);
                if let Some(counter_scope) = counter_scope {
                    self.end_counter_scope(counter_scope);
                }
                self.capture_suppressed_named_strings_after(element.id);
                self.pop_page_value_scope();
                self.exit_page_name_scope(page_name_scope);
                return;
            }
            self.layout_element_inner_with_principal_effect_context(
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                table_fragment,
                capture_principal_effect_context,
                principal_box_paint_mode,
                None,
            );
            let placement = self.final_source_assignment_placement(
                style,
                source_page_index,
                source_paint_checkpoint,
                source_starts_page_fragment,
                source_content_left,
                source_cursor_y,
            );
            self.update_named_assignment_placements(&named_assignment_ids, placement);
            if let Some(counter_scope) = counter_scope {
                self.end_counter_scope(counter_scope);
            }
            self.capture_suppressed_named_strings_after(element.id);
            self.pop_page_value_scope();
            self.exit_page_name_scope(page_name_scope);
            return;
        }
        self.layout_element_inner_with_principal_effect_context(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
            capture_principal_effect_context,
            principal_box_paint_mode,
            None,
        );
        if let Some(counter_scope) = counter_scope {
            self.end_counter_scope(counter_scope);
        }
        self.capture_suppressed_named_strings_after(element.id);
        self.pop_page_value_scope();
        self.exit_page_name_scope(page_name_scope);
    }
}
