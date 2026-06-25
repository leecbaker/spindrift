use super::*;
use crate::layout::assets::paint_effects_for_box;
use crate::text::trim_css_collapsible_whitespace;

#[derive(Debug, Clone)]
pub(super) struct PageNameScope {
    end_page_name: Option<String>,
    start_page_count: usize,
    start_page_has_content: bool,
}

impl<'a> LayoutBuilder<'a> {
    pub(super) fn new(config: LayoutBuilderConfig<'a>) -> Self {
        let mut page_margin_boxes = HashMap::new();
        let mut counter_styles = HashMap::new();
        let mut page_rules = Vec::new();
        let mut page_declarations = Declarations::new();
        let mut first_page_declarations = Declarations::new();
        for stylesheet in config.stylesheets {
            for rule in &stylesheet.rules {
                if rule.selector_text.trim() == ":root" {
                    page_declarations.extend(
                        (&rule.declarations)
                            .into_iter()
                            .filter_map(|(name, value)| {
                                name.starts_with("--")
                                    .then_some((name.clone(), value.clone()))
                            })
                            .collect(),
                    );
                }
            }
            first_page_declarations.extend(stylesheet.first_page_declarations.clone());
            page_rules.extend(stylesheet.page_rules.clone());
            for counter_style in &stylesheet.counter_styles {
                counter_styles.insert(counter_style.name.clone(), counter_style.clone());
            }
            for (name, declarations) in &stylesheet.page_margin_boxes {
                page_margin_boxes
                    .entry(name.clone())
                    .or_insert_with(Declarations::new)
                    .extend(declarations.clone());
            }
        }
        let page_context = PageContext::from_options(config.options);
        let mut builder = Self {
            options: config.options,
            base_url: config.base_url,
            root_url: config.root_url,
            resource_cache: config.resource_cache,
            pages: Vec::new(),
            page_names: Vec::new(),
            page_blanks: Vec::new(),
            page_name_scope_suppression: 0,
            page_name_element_scope_suppression: 0,
            page_named_strings: Vec::new(),
            page_running_elements: Vec::new(),
            page_anchors: HashMap::new(),
            page_anchor_text: HashMap::new(),
            document_canvas_background: None,
            root_canvas_background_defined: false,
            current_page: page_for_context(page_context),
            current_page_has_flow_content: false,
            current_page_name: None,
            current_page_context: page_context,
            cursor_y: page_context.top(),
            content_left: page_context.left(),
            content_right: page_context.right(),
            inline_static_baseline_y: None,
            containing_block_direction: Direction::Ltr,
            fragment_top_offsets: Vec::new(),
            definite_block_size_stack: Vec::new(),
            truncate_page_start_margins: false,
            avoid_inside_retry_depth: 0,
            containing_blocks: Vec::new(),
            list_stack: Vec::new(),
            counter_set: CounterSet::new(),
            quote_depth: 0,
            current_page_named_strings: HashMap::new(),
            current_page_running_elements: HashMap::new(),
            ancestors: Vec::new(),
            page_counter_initial_values: config.page_counter_initial_values,
            page_rules,
            page_progression_direction: config.page_progression_direction,
            page_declarations,
            page_margin_boxes,
            counter_styles,
            first_page_declarations,
            font_system: config.font_system,
            bookmarks: Vec::new(),
            positioned_layers: Vec::new(),
            fixed_layers: Vec::new(),
            next_paint_source_order: 1,
            overflow_clips: Vec::new(),
            float_contexts: vec![FloatContext { shapes: Vec::new() }],
            pending_float_fragments: Vec::new(),
            preserve_scoped_paint_public_order: false,
        };
        builder.rebuild_empty_current_page_context();
        builder
    }

    pub(super) fn layout_page_box(
        &mut self,
        page_box: &box_tree::PageBox<'_>,
        stylesheets: &[Stylesheet],
    ) {
        for child in &page_box.children {
            self.layout_formatting_box(child, stylesheets);
        }
    }

    pub(super) fn next_paint_source_order(&mut self) -> usize {
        let source_order = self.next_paint_source_order;
        self.next_paint_source_order += 1;
        source_order
    }

    /// Resolves font-metric-relative computed lengths in a formatting tree.
    ///
    /// CSS Values defines `ch` from the used font's "0" glyph advance. The
    /// box tree is built before fonts are resolved, so layout performs this
    /// used-value projection after `FontSystem` is available and before any
    /// formatting context consumes sizes:
    /// <https://www.w3.org/TR/css-values-4/#font-relative-lengths> and
    /// <https://www.w3.org/TR/css-cascade-5/#used>.
    pub(super) fn resolve_font_metric_lengths_in_page_box(
        &mut self,
        page_box: &mut box_tree::PageBox<'_>,
    ) {
        for child in &mut page_box.children {
            self.resolve_font_metric_lengths_in_box(child);
        }
    }

    fn resolve_style_viewport_lengths(
        style: &mut ComputedStyle,
        viewport_width: f32,
        viewport_height: f32,
    ) {
        style.resolve_viewport_lengths(viewport_width, viewport_height);
        if let Some(style) = &mut style.marker_style {
            Self::resolve_style_viewport_lengths(style, viewport_width, viewport_height);
        }
        if let Some(style) = &mut style.before_style {
            Self::resolve_style_viewport_lengths(style, viewport_width, viewport_height);
        }
        if let Some(style) = &mut style.after_style {
            Self::resolve_style_viewport_lengths(style, viewport_width, viewport_height);
        }
    }

    pub(super) fn style_with_current_viewport_lengths(
        &self,
        style: &ComputedStyle,
    ) -> ComputedStyle {
        let mut style = style.clone();
        self.resolve_style_current_viewport_lengths(&mut style);
        style
    }

    pub(super) fn resolve_style_current_viewport_lengths(&self, style: &mut ComputedStyle) {
        Self::resolve_style_viewport_lengths(
            style,
            self.page_area_width(),
            self.page_area_height(),
        );
    }

    fn resolve_font_metric_lengths_in_box(
        &mut self,
        formatting_box: &mut box_tree::FormattingBox<'_>,
    ) {
        match formatting_box {
            box_tree::FormattingBox::Block(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.run_in_children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::FormattingBox::Inline(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::FormattingBox::AtomicInline(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::FormattingBox::Line(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_style_font_metric_lengths(&mut child.style);
                }
            }
            box_tree::FormattingBox::Text(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
            }
            box_tree::FormattingBox::Table(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::FormattingBox::Flex(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
            box_tree::FormattingBox::Replaced(box_) => {
                self.resolve_style_font_metric_lengths(&mut box_.style);
                for child in &mut box_.children {
                    self.resolve_font_metric_lengths_in_box(child);
                }
            }
        }
    }

    fn resolve_style_font_metric_lengths(&mut self, style: &mut ComputedStyle) {
        let ch_advance = self.font_system.ch_advance(style);
        style.resolve_font_metric_lengths(ch_advance);
        if let Some(style) = &mut style.marker_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.before_style {
            self.resolve_style_font_metric_lengths(style);
        }
        if let Some(style) = &mut style.after_style {
            self.resolve_style_font_metric_lengths(style);
        }
    }

    pub(super) fn layout_formatting_box(
        &mut self,
        formatting_box: &box_tree::FormattingBox<'_>,
        stylesheets: &[Stylesheet],
    ) {
        match formatting_box {
            box_tree::FormattingBox::Block(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &box_.run_in_children,
                &box_.children,
            ),
            box_tree::FormattingBox::Inline(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &[],
                &box_.children,
            ),
            box_tree::FormattingBox::AnonymousBlock(box_) => {
                self.layout_anonymous_block(&box_.style, &box_.children, stylesheets, None)
            }
            box_tree::FormattingBox::AtomicInline(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &[],
                &box_.children,
            ),
            box_tree::FormattingBox::Table(box_) => {
                self.layout_table_box(
                    box_.element,
                    &box_.style,
                    stylesheets,
                    box_.signature.clone(),
                    &box_.children,
                    &box_.fragment,
                );
            }
            box_tree::FormattingBox::Flex(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &[],
                &box_.children,
            ),
            box_tree::FormattingBox::Replaced(box_) => self.layout_element_box(
                box_.element,
                &box_.style,
                stylesheets,
                box_.signature.clone(),
                &[],
                &box_.children,
            ),
            box_tree::FormattingBox::Line(box_) => {
                for child in &box_.children {
                    self.layout_text_block(&child.text, &child.style, 0.0, 0.0, None);
                }
            }
            box_tree::FormattingBox::Text(box_) => {
                let text = normalized_text_for_style(&box_.text, &box_.style);
                if !text.is_empty() {
                    self.layout_text_block(&text, &box_.style, 0.0, 0.0, None);
                }
            }
        }
    }

    pub(super) fn layout_element_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        signature: ElementSignature,
        run_in_children: &[box_tree::FormattingBox<'_>],
        children: &[box_tree::FormattingBox<'_>],
    ) {
        self.push_ancestor_signature(signature);
        self.layout_element_with_child_boxes_and_run_ins(
            element,
            style,
            stylesheets,
            run_in_children,
            Some(children),
        );
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
    pub(super) fn layout_table_box(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        signature: ElementSignature,
        children: &[box_tree::FormattingBox<'_>],
        fragment: &box_tree::TableFragment<'_>,
    ) {
        self.push_ancestor_signature(signature);
        self.layout_element_with_child_boxes_run_ins_and_table_fragment(
            element,
            style,
            stylesheets,
            &[],
            Some(children),
            Some(fragment),
        );
        self.ancestors.pop();
    }

    pub(super) fn layout_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
    ) {
        self.layout_element_with_child_boxes(element, style, stylesheets, None);
    }

    pub(super) fn layout_element_with_child_boxes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
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

    pub(super) fn layout_element_with_child_boxes_and_run_ins(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
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
        stylesheets: &[Stylesheet],
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
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let page_name_scope = self.enter_page_name_scope(style, child_boxes);
        if self.should_prebreak_avoid_inside(element, style, stylesheets, child_boxes) {
            self.push_page_if_nonempty();
        }
        let mut layout_style;
        let style = if !style.display.is_none() && style.break_before.is_forced() {
            // CSS Fragmentation places forced `break-before` before the
            // generated box. Counters, named strings, and running elements must
            // therefore observe the post-break page assignment rather than the
            // previous fragmentainer:
            // https://www.w3.org/TR/css-break-3/#break-between
            self.apply_forced_break(style.break_before);
            layout_style = style.clone();
            layout_style.break_before = PageBreak::Auto;
            &layout_style
        } else {
            style
        };
        let counter_scope =
            (!style.display.is_none()).then(|| self.begin_counter_scope(element, style));
        if !style.display.is_none() {
            self.capture_named_strings(element, style);
            if self.capture_running_element(element, style) {
                if let Some(counter_scope) = counter_scope {
                    self.end_counter_scope(counter_scope);
                }
                self.exit_page_name_scope(page_name_scope);
                return;
            }
        }
        if self.should_try_avoid_break_inside(style) {
            self.layout_avoiding_break_inside(
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                table_fragment,
            );
            if let Some(counter_scope) = counter_scope {
                self.end_counter_scope(counter_scope);
            }
            self.materialize_empty_named_page_scope(style, page_name_scope.as_ref());
            self.exit_page_name_scope(page_name_scope);
            return;
        }
        self.layout_element_inner(
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
        );
        if let Some(counter_scope) = counter_scope {
            self.end_counter_scope(counter_scope);
        }
        self.materialize_empty_named_page_scope(style, page_name_scope.as_ref());
        self.exit_page_name_scope(page_name_scope);
    }

    /// Materializes an empty page box for an entered named-page scope.
    ///
    /// CSS Paged Media forms named page groups from elements with a `page`
    /// value at class A break boundaries. WPT `page-name-display-none-child`
    /// expects an otherwise empty page-owning block to still occupy its named
    /// page before the next page group starts:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    fn materialize_empty_named_page_scope(
        &mut self,
        style: &ComputedStyle,
        scope: Option<&PageNameScope>,
    ) {
        let Some(scope) = scope else {
            return;
        };
        if style.page_name_specified && !self.scope_produced_current_page_content(scope) {
            self.materialize_current_empty_page();
        }
    }

    fn enter_page_name_scope(
        &mut self,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Option<PageNameScope> {
        if self.page_name_scope_suppression > 0 || self.page_name_element_scope_suppression > 0 {
            return None;
        }
        if style.display.is_none()
            || matches!(style.position, Position::Absolute | Position::Fixed)
            || style.running_element_name.is_some()
        {
            return None;
        }
        if style.display.is_flex() {
            if !style.page_name_specified {
                return None;
            }
            let page_name = style.page_name.as_deref();
            self.enter_page_name_scope_for_value(page_name);
            return Some(self.page_name_scope_checkpoint(style.page_name.clone()));
        }
        if !style.page_name_specified
            && child_boxes
                .map(formatting_boxes_page_values)
                .is_none_or(|(start, end)| start.is_none() && end.is_none())
        {
            return None;
        }
        let (start_page_name, end_page_name) =
            page_values_from_style_and_children(style, child_boxes.unwrap_or_default());
        self.enter_page_name_scope_for_value(start_page_name.as_deref());
        Some(self.page_name_scope_checkpoint(end_page_name))
    }

    fn page_name_scope_checkpoint(&self, end_page_name: Option<String>) -> PageNameScope {
        PageNameScope {
            end_page_name,
            start_page_count: self.pages.len(),
            start_page_has_content: self.current_page_has_content(),
        }
    }

    fn scope_produced_current_page_content(&self, scope: &PageNameScope) -> bool {
        self.pages.len() != scope.start_page_count
            || self.current_page_has_content() != scope.start_page_has_content
    }

    fn materialize_current_empty_page(&mut self) {
        if self.current_page_has_content() {
            return;
        }
        let next_context = self.resolved_page_context(self.pages.len() + 2, false);
        let next_page = page_for_context(next_context);
        let page = std::mem::replace(&mut self.current_page, next_page);
        self.pages.push(page);
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(false);
        self.page_named_strings
            .push(std::mem::take(&mut self.current_page_named_strings));
        self.page_running_elements
            .push(std::mem::take(&mut self.current_page_running_elements));
        self.current_page_has_flow_content = false;
        self.apply_page_context(
            next_context,
            FragmentOffsets {
                left: 0.0,
                right: 0.0,
                top: 0.0,
            },
        );
        self.truncate_page_start_margins = true;
    }

    /// Switches named page groups at a class A sibling page-break boundary.
    ///
    /// CSS Paged Media defines `page` transitions at possible page-break
    /// points between block-level siblings, using the previous sibling's end
    /// page value and the next sibling's start page value:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
    pub(super) fn switch_page_name_at_class_a_boundary(&mut self, page_name: Option<&str>) {
        if self.page_name_scope_suppression > 0 {
            return;
        }
        self.enter_page_name_scope_for_value(page_name);
    }

    /// Enters a page-name scope for inline-level content.
    ///
    /// CSS Paged Media applies the `page` property to boxes, including
    /// inline-level boxes. When a later inline box specifies a different page,
    /// the current line fragment must end and following content must be laid
    /// out in that page group:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(super) fn enter_inline_page_name_scope(
        &mut self,
        page_name: Option<&str>,
    ) -> Option<PageNameScope> {
        if self.page_name_scope_suppression > 0 {
            return None;
        }
        let previous = self.current_page_name.clone();
        self.enter_page_name_scope_for_value(page_name);
        Some(self.page_name_scope_checkpoint(previous))
    }

    /// Exits an inline page-name scope, breaking before following inline content.
    ///
    /// When inline content has already been painted on the named page, returning
    /// to the surrounding page group must create a new page box before
    /// restoring that group. Otherwise following inline content would use the
    /// previous page box's margins and page selectors:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(super) fn exit_inline_page_name_scope(&mut self, scope: Option<PageNameScope>) {
        if scope.is_some() && self.current_page_has_content() {
            self.push_page_if_nonempty();
        }
        self.exit_page_name_scope(scope);
    }

    /// Suppresses CSS named-page group creation for out-of-flow and atomic layout.
    ///
    /// CSS Paged Media defines named page groups through normal-flow class A
    /// page-break boundaries. Absolutely positioned and fixed-position boxes
    /// are out of flow, while inline-block contents are laid out in an
    /// independent atomic inline formatting context; in both cases descendant
    /// `page` values do not directly select document page groups:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>,
    /// <https://www.w3.org/TR/CSS22/visuren.html#inline-blocks>, and
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning>.
    pub(super) fn push_page_name_scope_suppression(&mut self) {
        self.page_name_scope_suppression += 1;
    }

    /// Re-enables CSS named-page group creation after suppressed layout.
    ///
    /// This closes the temporary suppression scope opened for out-of-flow or
    /// atomic inline formatting-context layout:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(super) fn pop_page_name_scope_suppression(&mut self) {
        self.page_name_scope_suppression = self.page_name_scope_suppression.saturating_sub(1);
    }

    /// Suppresses element-entry named-page scopes while preserving sibling switches.
    ///
    /// Flex items do not expose their own `page` value, or descendant-derived
    /// first/last page values, to the flex container boundary. Class A break
    /// opportunities between ordinary block descendants inside the flex item
    /// still select named page groups:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
    pub(super) fn push_page_name_element_scope_suppression(&mut self) {
        self.page_name_element_scope_suppression += 1;
    }

    /// Re-enables element-entry named-page scopes after isolated item layout.
    ///
    /// This closes the flex-item page-scope isolation described by CSS Paged
    /// Media named pages and CSS Flexbox pagination:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(super) fn pop_page_name_element_scope_suppression(&mut self) {
        self.page_name_element_scope_suppression =
            self.page_name_element_scope_suppression.saturating_sub(1);
    }

    fn enter_page_name_scope_for_value(
        &mut self,
        page_name: Option<&str>,
    ) -> Option<Option<String>> {
        if self.current_page_name.as_deref() == page_name {
            return None;
        }
        let previous = self.current_page_name.clone();
        // CSS Paged Media assigns a named page type to boxes using the `page`
        // property. The initial `auto` value is still a real page type when
        // explicitly specified, because it can end an ancestor's named page
        // group. In this cursor-based layout engine, pages occupied by the
        // scoped element inherit that page value until the element finishes.
        // https://www.w3.org/TR/css-page-3/#using-named-pages
        self.push_page_if_nonempty();
        self.current_page_name = page_name.map(str::to_string);
        self.rebuild_empty_current_page_context();
        Some(previous)
    }

    pub(super) fn exit_page_name_scope(&mut self, scope: Option<PageNameScope>) {
        let Some(scope) = scope else {
            return;
        };
        if self.current_page_name == scope.end_page_name {
            return;
        }
        self.current_page_name = scope.end_page_name;
        self.rebuild_empty_current_page_context();
    }

    pub(super) fn layout_element_inner(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let layout_kind = element_layout_kind(element, style);
        if self.should_capture_non_positioned_effect_context(layout_kind, style) {
            self.layout_non_positioned_effect_context(
                layout_kind,
                element,
                style,
                stylesheets,
                run_in_children,
                child_boxes,
                table_fragment,
            );
            return;
        }
        self.layout_element_inner_kind(
            layout_kind,
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_element_inner_kind(
        &mut self,
        layout_kind: ElementLayoutKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        match layout_kind {
            ElementLayoutKind::None => (),
            ElementLayoutKind::Positioned => {
                self.layout_positioned_block_with_static_source(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    table_fragment,
                );
            }
            ElementLayoutKind::HorizontalRule => self.layout_hr(style),
            ElementLayoutKind::Canvas => self.layout_canvas(element, style),
            ElementLayoutKind::Image => self.layout_image(element, style),
            ElementLayoutKind::GeneratedImage => self.layout_generated_image(element, style),
            ElementLayoutKind::Svg => self.layout_svg(element, style),
            ElementLayoutKind::Flex => self.layout_flex(element, style, stylesheets, child_boxes),
            ElementLayoutKind::Table => {
                let built_child_boxes;
                let table_children = if let Some(children) = child_boxes {
                    children
                } else {
                    built_child_boxes =
                        box_tree::build_child_boxes(element, stylesheets, style, &self.ancestors);
                    &built_child_boxes
                };
                let built_fragment;
                let fragment = if let Some(fragment) = table_fragment {
                    fragment
                } else {
                    let signature = self.ancestors.last().cloned().unwrap_or_else(|| {
                        ElementSignature::new(element.tag.clone(), element.attrs.clone())
                    });
                    built_fragment =
                        box_tree::build_table_fragment(element, &signature, table_children);
                    &built_fragment
                };
                self.layout_table(element, style, stylesheets, fragment)
            }
            ElementLayoutKind::InlineFlow => {
                let text = inline_text_for_style(element, style);
                if !text.is_empty() {
                    if style.display.is_list_item() {
                        let marker = self.marker_for_list_item(
                            element,
                            style,
                            self.containing_block_direction,
                        );
                        self.layout_list_text_block(
                            &text,
                            style,
                            0.0,
                            0.0,
                            element.attrs.get("href").map(String::as_str),
                            marker.as_ref(),
                        );
                    } else {
                        self.layout_text_block(
                            &text,
                            style,
                            0.0,
                            0.0,
                            element.attrs.get("href").map(String::as_str),
                        );
                    }
                }
            }
            ElementLayoutKind::BlockFlow => {
                self.layout_block(element, style, stylesheets, run_in_children, child_boxes);
            }
        }
    }

    fn should_capture_non_positioned_effect_context(
        &self,
        layout_kind: ElementLayoutKind,
        style: &ComputedStyle,
    ) -> bool {
        !matches!(
            layout_kind,
            ElementLayoutKind::None | ElementLayoutKind::Positioned
        ) && (style.opacity < 1.0 || !style.transform.is_empty() || style.overflow.clips_overflow())
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_non_positioned_effect_context(
        &mut self,
        layout_kind: ElementLayoutKind,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        run_in_children: &[box_tree::FormattingBox<'_>],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        self.layout_element_inner_kind(
            layout_kind,
            element,
            style,
            stylesheets,
            run_in_children,
            child_boxes,
            table_fragment,
        );
        let child_layers = if positioned_layer_start < self.positioned_layers.len() {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        let fragments = self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        for (page_index, fragment) in fragments {
            if fragment.is_empty() {
                continue;
            }
            let child_contexts = child_layers
                .iter()
                .filter(|layer| layer.page_index == page_index)
                .cloned()
                .map(|layer| layer.context.with_links(layer.links))
                .collect::<Vec<_>>();
            let source_order = self.next_paint_source_order();
            let (page_width, page_height) = if page_index < self.pages.len() {
                (self.pages[page_index].width, self.pages[page_index].height)
            } else {
                (self.current_page.width, self.current_page.height)
            };
            let target_page = if page_index < self.pages.len() {
                &mut self.pages[page_index]
            } else {
                &mut self.current_page
            };
            let bounds = fragment.bounds().unwrap_or(PaintClip {
                x: 0.0,
                y: 0.0,
                width: page_width,
                height: page_height,
            });
            let context = PaintStackingContext::from_banded_fragment(fragment, child_contexts)
                .with_source_order(source_order)
                .with_effects(paint_effects_for_box(style, bounds))
                .with_bounds(bounds);
            let context_fragment =
                PaintFragment::from_stacking_context_in_band(PaintBand::InFlowBlock, context);
            target_page.append_paint_fragment(&context_fragment, 0.0, 0.0);
        }
    }

    pub(super) fn layout_positioned_block_with_static_source(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        if style.abspos_static_source_was_inline_level
            && let Some(static_baseline_y) = self.current_page.lines.last().map(|line| line.y)
        {
            self.layout_positioned_block_with_inline_static_baseline(
                element,
                style,
                stylesheets,
                child_boxes,
                table_fragment,
                static_baseline_y,
            );
            return;
        }
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
    }

    pub(super) fn layout_anonymous_block(
        &mut self,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        stylesheets: &[Stylesheet],
        marker: Option<&ListMarker>,
    ) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        if marker.is_none()
            && anonymous_block_is_plain_text_with_style(children, style)
            && !self.active_float_exclusions_at(self.cursor_y, style.line_height)
        {
            let text = inline_text_from_formatting_boxes(children);
            if !text.is_empty() {
                self.layout_text_block(&text, style, 0.0, 0.0, None);
            }
            return;
        }
        let mut items = Vec::new();
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Outside
        {
            if self.cursor_y - style.font_size < self.page_bottom() {
                self.push_page();
            }
            self.paint_outside_marker(
                marker,
                style,
                self.content_left,
                self.content_right,
                self.cursor_y,
            );
        }
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_start(style, None, 0.0, &mut items);
        }
        if let Some(marker) = marker
            && marker.position == ListStylePosition::Inside
            && (marker.image.is_some() || !trim_css_collapsible_whitespace(&marker.text).is_empty())
        {
            self.push_inside_marker_items(marker, style, None, &mut items);
        }
        self.collect_inline_box_items(
            children,
            stylesheets,
            None,
            0.0,
            style.text_decoration,
            &mut items,
        );
        if block_bidi_scope_needs_inline_controls(style) {
            self.push_bidi_scope_end(style, None, 0.0, &mut items);
        }
        if !items.is_empty() {
            self.layout_inline_items(items, style, available_width, 0.0, 0.0, stylesheets);
        }
    }

    pub(super) fn push_page(&mut self) {
        if !self.current_page_has_content() {
            // CSS Fragmentation allows a box fragment to be split across
            // fragmentainers, but a carried fragment offset must not make a
            // fresh empty page permanently unfillable. If a break is requested
            // before anything painted on the current page, keep the same page
            // number and retry the fragment at the top of this page area:
            // <https://www.w3.org/TR/css-break-3/#breaking-rules>.
            let offsets = FragmentOffsets {
                top: 0.0,
                ..self.current_fragment_offsets()
            };
            let context = self.resolved_page_context(self.pages.len() + 1, false);
            self.current_page = page_for_context(context);
            self.current_page_has_flow_content = false;
            self.apply_page_context(context, offsets);
            self.truncate_page_start_margins = true;
            self.apply_pending_float_fragments_for_current_page();
            return;
        }
        self.flush_positioned_layers();
        let offsets = self.current_fragment_offsets_for_page_break();
        let next_context = self.resolved_page_context(self.pages.len() + 2, false);
        let next_page = page_for_context(next_context);
        let page = std::mem::replace(&mut self.current_page, next_page);
        self.current_page_has_flow_content = false;
        self.pages.push(page);
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(false);
        self.page_named_strings
            .push(std::mem::take(&mut self.current_page_named_strings));
        self.page_running_elements
            .push(std::mem::take(&mut self.current_page_running_elements));
        self.apply_page_context(next_context, offsets);
        self.truncate_page_start_margins = true;
        self.apply_pending_float_fragments_for_current_page();
    }

    pub(super) fn push_blank_page(&mut self) {
        // CSS Fragmentation forced left/right/recto/verso breaks can generate
        // blank pages. Those pages are real page boxes and match `@page :blank`.
        // https://www.w3.org/TR/css-break-3/#break-between
        let page_number = self.pages.len() + 1;
        let context = self.resolved_page_context(page_number, true);
        self.pages.push(page_for_context(context));
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(true);
        self.page_named_strings.push(HashMap::new());
        self.page_running_elements.push(HashMap::new());
    }

    pub(super) fn push_page_if_nonempty(&mut self) {
        if self.current_page_has_content() {
            self.push_page();
        }
    }

    /// Captures the active formatting-context insets from the current page area.
    ///
    /// A page break fragments boxes without leaving their containing block, while
    /// a named-page transition can select a different page area. Keeping these
    /// offsets preserves ancestor margins and padding on the new page fragment:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting> and
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    fn current_fragment_offsets(&self) -> FragmentOffsets {
        FragmentOffsets {
            left: self.content_left - self.current_page_context.left(),
            right: self.current_page_context.right() - self.content_right,
            top: self
                .fragment_top_offsets
                .last()
                .copied()
                .unwrap_or_else(|| self.current_page_context.top() - self.cursor_y),
        }
    }

    /// Captures fragment insets for an actual page break.
    ///
    /// The split box's next fragment starts in the same ancestor formatting
    /// context, but it must not reuse the split box's old block-position on the
    /// previous page. CSS Fragmentation defines the next fragment in a new
    /// fragmentainer while preserving the surrounding formatting context:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    fn current_fragment_offsets_for_page_break(&self) -> FragmentOffsets {
        let mut offsets = self.current_fragment_offsets();
        if self.fragment_top_offsets.len() >= 2 {
            offsets.top = self.fragment_top_offsets[self.fragment_top_offsets.len() - 2];
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
    fn apply_page_context(&mut self, context: PageContext, offsets: FragmentOffsets) {
        self.current_page_context = context;
        self.current_page.rotation = context.rotation;
        self.cursor_y = context.top() - offsets.top;
        self.content_left = context.left() + offsets.left;
        self.content_right = (context.right() - offsets.right).max(self.content_left);
    }

    pub(super) fn apply_forced_break(&mut self, forced_break: PageBreak) {
        if !forced_break.is_forced() {
            return;
        }
        if self.current_page_has_content() {
            self.push_page();
        }
        while !forced_break_satisfied(
            forced_break,
            self.pages.len() + 1,
            self.page_progression_direction,
        ) {
            self.push_blank_page();
        }
        self.rebuild_empty_current_page_context();
        self.truncate_page_start_margins = true;
    }

    pub(super) fn current_page_has_content(&self) -> bool {
        self.current_page.has_paint_content() || self.current_page_has_flow_content
    }

    /// Marks the current page as containing a non-empty normal-flow box.
    ///
    /// CSS Fragmentation fragments boxes into page fragmentainers even when a
    /// particular fragment has no visible paint. A used border box with
    /// positive area must therefore keep its page for forced breaks and final
    /// pagination, independently from PDF paint primitives:
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
    /// <https://www.w3.org/TR/css-box-3/#box-model>.
    pub(super) fn mark_current_page_flow_content(&mut self) {
        self.current_page_has_flow_content = true;
    }

    pub(super) fn page_left(&self) -> f32 {
        self.current_page_context.left()
    }

    pub(super) fn page_top(&self) -> f32 {
        self.current_page_context.top()
    }

    pub(super) fn page_bottom(&self) -> f32 {
        self.current_page_context.bottom()
    }

    pub(super) fn page_area_width(&self) -> f32 {
        self.current_page_context.area_width()
    }

    pub(super) fn page_area_height(&self) -> f32 {
        self.current_page_context.area_height()
    }

    fn resolved_page_context(&self, page_number: usize, is_blank: bool) -> PageContext {
        let declarations = self.page_declarations_for_page(
            page_number,
            self.current_page_name.as_deref(),
            is_blank,
        );
        let base = PageContext::from_options(self.options);
        // CSS Paged Media defines page size and page margins in the page
        // context; these declarations select the page box before its content
        // area is used for layout.
        // https://www.w3.org/TR/css-page-3/#page-model
        let size = css::page_size_from(&declarations, base.size);
        let page_edges = page_box_edges_from_declarations(&declarations, size);
        PageContext {
            size,
            margins: css::page_margins_from_for_size_and_edges(
                &declarations,
                base.margins,
                size,
                page_edges.total(),
            ),
            edges: page_edges,
            rotation: css::page_rotation_from(&declarations, base.rotation),
        }
    }

    fn finished_page_context(&self, page_number: usize, page_size: PageSize) -> PageContext {
        let page_name = self.page_name_for_number(page_number);
        let is_blank = self.page_is_blank_for_number(page_number);
        let declarations = self.page_declarations_for_page(page_number, page_name, is_blank);
        let base = PageContext::from_options(self.options);
        let page_edges = page_box_edges_from_declarations(&declarations, page_size);
        PageContext {
            size: page_size,
            margins: css::page_margins_from_for_size_and_edges(
                &declarations,
                base.margins,
                page_size,
                page_edges.total(),
            ),
            edges: page_edges,
            rotation: css::page_rotation_from(&declarations, base.rotation),
        }
    }

    fn rebuild_empty_current_page_context(&mut self) {
        if self.current_page_has_content() {
            return;
        }
        let offsets = self.current_fragment_offsets();
        let page_number = self.pages.len() + 1;
        let context = self.resolved_page_context(page_number, false);
        self.current_page = page_for_context(context);
        self.apply_page_context(context, offsets);
    }

    pub(super) fn has_renderable_content(&self) -> bool {
        !self.pages.is_empty()
            || self.current_page_has_content()
            || !self.positioned_layers.is_empty()
            || !self.fixed_layers.is_empty()
            || !self.page_margin_boxes.is_empty()
            || self
                .page_rules
                .iter()
                .any(|rule| !rule.margin_boxes.is_empty())
    }

    pub(super) fn cursor_is_at_page_top(&self) -> bool {
        (self.cursor_y - self.page_top()).abs() < 0.01
    }

    pub(super) fn snapshot(&self) -> LayoutSnapshot {
        LayoutSnapshot {
            pages: self.pages.clone(),
            page_names: self.page_names.clone(),
            page_blanks: self.page_blanks.clone(),
            page_name_scope_suppression: self.page_name_scope_suppression,
            page_name_element_scope_suppression: self.page_name_element_scope_suppression,
            page_named_strings: self.page_named_strings.clone(),
            page_running_elements: self.page_running_elements.clone(),
            page_anchors: self.page_anchors.clone(),
            page_anchor_text: self.page_anchor_text.clone(),
            document_canvas_background: self.document_canvas_background.clone(),
            root_canvas_background_defined: self.root_canvas_background_defined,
            current_page: self.current_page.clone(),
            current_page_has_flow_content: self.current_page_has_flow_content,
            current_page_name: self.current_page_name.clone(),
            current_page_context: self.current_page_context,
            cursor_y: self.cursor_y,
            content_left: self.content_left,
            content_right: self.content_right,
            inline_static_baseline_y: self.inline_static_baseline_y,
            fragment_top_offsets: self.fragment_top_offsets.clone(),
            definite_block_size_stack: self.definite_block_size_stack.clone(),
            truncate_page_start_margins: self.truncate_page_start_margins,
            avoid_inside_retry_depth: self.avoid_inside_retry_depth,
            containing_blocks: self.containing_blocks.clone(),
            list_stack: self.list_stack.clone(),
            counter_set: self.counter_set.clone(),
            current_page_named_strings: self.current_page_named_strings.clone(),
            current_page_running_elements: self.current_page_running_elements.clone(),
            ancestors: self.ancestors.clone(),
            bookmarks: self.bookmarks.clone(),
            positioned_layers: self.positioned_layers.clone(),
            fixed_layers: self.fixed_layers.clone(),
            next_paint_source_order: self.next_paint_source_order,
            float_contexts: self.float_contexts.clone(),
            pending_float_fragments: self.pending_float_fragments.clone(),
            preserve_scoped_paint_public_order: self.preserve_scoped_paint_public_order,
        }
    }

    pub(super) fn restore(&mut self, snapshot: LayoutSnapshot) {
        self.pages = snapshot.pages;
        self.page_names = snapshot.page_names;
        self.page_blanks = snapshot.page_blanks;
        self.page_name_scope_suppression = snapshot.page_name_scope_suppression;
        self.page_name_element_scope_suppression = snapshot.page_name_element_scope_suppression;
        self.page_named_strings = snapshot.page_named_strings;
        self.page_running_elements = snapshot.page_running_elements;
        self.page_anchors = snapshot.page_anchors;
        self.page_anchor_text = snapshot.page_anchor_text;
        self.document_canvas_background = snapshot.document_canvas_background;
        self.root_canvas_background_defined = snapshot.root_canvas_background_defined;
        self.current_page = snapshot.current_page;
        self.current_page_has_flow_content = snapshot.current_page_has_flow_content;
        self.current_page_name = snapshot.current_page_name;
        self.current_page_context = snapshot.current_page_context;
        self.cursor_y = snapshot.cursor_y;
        self.content_left = snapshot.content_left;
        self.content_right = snapshot.content_right;
        self.inline_static_baseline_y = snapshot.inline_static_baseline_y;
        self.fragment_top_offsets = snapshot.fragment_top_offsets;
        self.definite_block_size_stack = snapshot.definite_block_size_stack;
        self.truncate_page_start_margins = snapshot.truncate_page_start_margins;
        self.avoid_inside_retry_depth = snapshot.avoid_inside_retry_depth;
        self.containing_blocks = snapshot.containing_blocks;
        self.list_stack = snapshot.list_stack;
        self.counter_set = snapshot.counter_set;
        self.current_page_named_strings = snapshot.current_page_named_strings;
        self.current_page_running_elements = snapshot.current_page_running_elements;
        self.ancestors = snapshot.ancestors;
        self.bookmarks = snapshot.bookmarks;
        self.positioned_layers = snapshot.positioned_layers;
        self.fixed_layers = snapshot.fixed_layers;
        self.next_paint_source_order = snapshot.next_paint_source_order;
        self.float_contexts = snapshot.float_contexts;
        self.pending_float_fragments = snapshot.pending_float_fragments;
        self.preserve_scoped_paint_public_order = snapshot.preserve_scoped_paint_public_order;
    }

    pub(super) fn finish(mut self) -> Document {
        self.flush_positioned_layers();
        self.apply_pending_float_fragments_for_current_page();
        if self.current_page_has_content() {
            self.push_page();
        }
        while !self.pending_float_fragments.is_empty() {
            self.apply_pending_float_fragments_for_current_page();
            if self.current_page_has_content() {
                self.push_page();
            } else {
                break;
            }
        }
        if self.pages.is_empty() {
            let mut page = page_for_context(self.current_page_context);
            page.push_line(RenderedLine {
                text: String::new(),
                x: self.page_left(),
                y: self.page_top() - self.options.font_size,
                font_size: self.options.font_size,
                font_id: {
                    let mut style = ComputedStyle::initial();
                    style.font_size = self.options.font_size;
                    style.line_height_value =
                        css::ComputedLineHeight::Length(self.options.line_height);
                    style.line_height = self.options.line_height;
                    style.line_height_multiplier = None;
                    style.line_height_is_normal = false;
                    self.font_system.resolve_style(&style)
                },
                color: Color::BLACK,
                runs: Vec::new(),
            });
            self.pages.push(page);
            self.page_names.push(self.current_page_name.clone());
            self.page_blanks.push(false);
            self.page_named_strings
                .push(std::mem::take(&mut self.current_page_named_strings));
            self.page_running_elements
                .push(std::mem::take(&mut self.current_page_running_elements));
        }
        self.apply_fixed_layers_to_pages();
        self.add_page_backgrounds();
        self.add_page_margin_boxes();
        for page in &mut self.pages {
            page.finalize_paint_tree_for_public_view();
        }
        let fonts = self.font_system.into_fonts();
        Document {
            pages: self.pages,
            fonts,
            bookmarks: self.bookmarks,
            metadata: DocumentMetadata {
                producer: self.options.producer.clone(),
                ..DocumentMetadata::default()
            },
        }
    }

    /// Inserts page-box background and border paint below document content.
    ///
    /// CSS Paged Media allows backgrounds and borders on the page box, and CSS
    /// Backgrounds and Borders paints backgrounds below borders. These
    /// primitives are inserted at the start of the PDF page paint stream so
    /// normal document content remains above the page underlay:
    /// <https://www.w3.org/TR/css-page-3/#page-properties> and
    /// <https://www.w3.org/TR/css-backgrounds-3/#layering>.
    pub(super) fn add_page_backgrounds(&mut self) {
        if self.pages.is_empty() {
            return;
        }
        for page_index in 0..self.pages.len() {
            let page_number = page_index + 1;
            let declarations = self.page_declarations_for(page_number);
            let page_width = self.pages[page_index].width;
            let page_height = self.pages[page_index].height;
            let page_size = PageSize {
                width: page_width,
                height: page_height,
            };
            let mut has_visible_page_paint = false;
            if !declarations.is_empty() {
                let mut style = ComputedStyle::initial();
                css::apply_declarations(&mut style, &declarations);
                has_visible_page_paint = page_style_has_visible_paint(&style);
                let image_area = page_background_positioning_area(
                    &declarations,
                    PageContext::from_options(self.options).margins,
                    page_size,
                    style.background_origin,
                );
                let images = self.background_images(
                    image_area.x,
                    image_area.y,
                    image_area.width,
                    image_area.height,
                    &style,
                );
                let page = &mut self.pages[page_index];

                let mut background_style = style.clone();
                background_style.border_widths = css::Edges::ZERO;
                background_style.border_styles = css::BorderStyles::NONE;
                background_style.border_width = 0.0;
                let (rects, rounded_rects, paths, strokes) =
                    block_paint_ops(0.0, 0.0, page_width, page_height, &background_style);
                for rect in rects {
                    page.push_rect_in_band(PaintBand::BackgroundBorder, rect);
                }
                for rounded_rect in rounded_rects {
                    page.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rounded_rect);
                }
                for path in paths {
                    page.push_path_in_band(PaintBand::BackgroundBorder, path);
                }
                for stroke in strokes {
                    page.push_stroke_in_band(PaintBand::BackgroundBorder, stroke);
                }
                for image in images {
                    page.push_image_in_band(PaintBand::BackgroundBorder, image);
                }

                let mut border_style = style;
                border_style.background_color = None;
                border_style.background_image = None;
                let (rects, rounded_rects, paths, strokes) =
                    block_paint_ops(0.0, 0.0, page_width, page_height, &border_style);
                for rect in rects {
                    page.push_rect_in_band(PaintBand::BackgroundBorder, rect);
                }
                for rounded_rect in rounded_rects {
                    page.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rounded_rect);
                }
                for path in paths {
                    page.push_path_in_band(PaintBand::BackgroundBorder, path);
                }
                for stroke in strokes {
                    page.push_stroke_in_band(PaintBand::BackgroundBorder, stroke);
                }
            }
            self.add_document_canvas_background(
                page_index,
                page_size,
                has_visible_page_paint
                    || (!self.root_canvas_background_defined
                        && self.pages.len() > 1
                        && self.has_authored_page_rules()),
            );
        }
    }

    fn has_authored_page_rules(&self) -> bool {
        !self.page_rules.is_empty() || !self.first_page_declarations.is_empty()
    }

    fn add_document_canvas_background(
        &mut self,
        page_index: usize,
        page_size: PageSize,
        has_visible_page_paint: bool,
    ) {
        let Some(style) = self.document_canvas_background.clone() else {
            return;
        };
        let (x, y, width, height) = if has_visible_page_paint {
            let context = self.finished_page_context(page_index + 1, page_size);
            (
                context.left(),
                context.bottom(),
                context.area_width(),
                context.area_height(),
            )
        } else {
            (0.0, 0.0, page_size.width, page_size.height)
        };
        let images = self.background_images(x, y, width, height, &style);
        let page = &mut self.pages[page_index];
        let (rects, rounded_rects, paths, strokes) = block_paint_ops(x, y, width, height, &style);
        for rect in rects {
            page.push_rect_in_band(PaintBand::BackgroundBorder, rect);
        }
        for rounded_rect in rounded_rects {
            page.push_rounded_rect_in_band(PaintBand::BackgroundBorder, rounded_rect);
        }
        for path in paths {
            page.push_path_in_band(PaintBand::BackgroundBorder, path);
        }
        for stroke in strokes {
            page.push_stroke_in_band(PaintBand::BackgroundBorder, stroke);
        }
        for image in images {
            page.push_image_in_band(PaintBand::BackgroundBorder, image);
        }
    }

    pub(super) fn add_bookmark(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        x: f32,
        y: f32,
    ) {
        let Some(level) = style.bookmark_level else {
            return;
        };
        if style.display.is_none() || style.visibility != Visibility::Visible {
            return;
        }
        let label = collapse_whitespace(&evaluate_bookmark_label(element, style));
        if label.is_empty() {
            return;
        }
        self.bookmarks.push(Bookmark {
            level,
            label,
            page_index: self.pages.len(),
            x,
            y,
            state: match style.bookmark_state {
                CssBookmarkState::Open => BookmarkState::Open,
                CssBookmarkState::Closed => BookmarkState::Closed,
            },
        });
    }

    /// Captures the propagated document-canvas background source.
    ///
    /// CSS Backgrounds defines the special root/body background propagation
    /// rule: the root element background paints the canvas; when the root has
    /// no background, the first body background is propagated instead. In
    /// paged media, that propagated canvas background paints each page canvas
    /// unless an explicit visible page background or border owns the margin
    /// paint:
    /// <https://www.w3.org/TR/css-backgrounds-3/#special-backgrounds> and
    /// <https://www.w3.org/TR/css-page-3/#painting>.
    pub(super) fn capture_document_canvas_background(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) {
        if !is_document_canvas_element(element) {
            return;
        }
        let has_background = style.background_color.is_some_and(Color::is_visible)
            || style.background_image.is_some();
        if element.tag.eq_ignore_ascii_case("html") {
            self.root_canvas_background_defined = has_background;
            if has_background {
                self.document_canvas_background = Some(canvas_background_style(style));
            }
        } else if element.tag.eq_ignore_ascii_case("body")
            && !self.root_canvas_background_defined
            && has_background
        {
            self.document_canvas_background = Some(canvas_background_style(style));
        }
    }

    /// Records the generated page containing an HTML anchor.
    ///
    /// WeasyPrint's UA stylesheet maps `[id]` and `a[name]` to document
    /// anchors, and CSS Generated Content for Paged Media allows generated
    /// content such as `target-counter(..., page)` to resolve those targets:
    /// <https://www.w3.org/TR/css-gcpm-3/#cross-references>.
    pub(super) fn add_page_anchor(&mut self, element: &Element, style: &ComputedStyle) {
        if let Some(id) = element.attrs.get("id").filter(|value| !value.is_empty()) {
            self.page_anchors
                .entry(id.clone())
                .or_insert(self.pages.len());
            if !self.page_anchor_text.contains_key(id) {
                let anchor_text = self.anchor_text_for_element(element, style);
                self.page_anchor_text.insert(id.clone(), anchor_text);
            }
        }
        if element.tag.eq_ignore_ascii_case("a")
            && let Some(name) = element.attrs.get("name").filter(|value| !value.is_empty())
        {
            self.page_anchors
                .entry(name.clone())
                .or_insert(self.pages.len());
            if !self.page_anchor_text.contains_key(name) {
                let anchor_text = self.anchor_text_for_element(element, style);
                self.page_anchor_text.insert(name.clone(), anchor_text);
            }
        }
    }

    /// Captures text exposed to generated-content cross references.
    ///
    /// CSS Generated Content for Paged Media defines `target-text()` keywords
    /// for target element content and generated `::before`/`::after` text. This
    /// helper records those values at layout time so page-margin generated
    /// content can resolve them after pagination:
    /// <https://www.w3.org/TR/css-gcpm-3/#target-text>.
    fn anchor_text_for_element(&mut self, element: &Element, style: &ComputedStyle) -> AnchorText {
        AnchorText {
            content: target_element_text(element),
            before: self
                .evaluate_generated_pseudo_text_rollback(element, style.before_style.as_deref()),
            after: self
                .evaluate_generated_pseudo_text_rollback(element, style.after_style.as_deref()),
        }
    }

    // CSS 2.2 Appendix E paints positioned descendants by stack level in their
    // containing stacking context. Each layer now carries a nested display-list
    // context; this final page flush only chooses the root stack-level slot.
    fn flush_positioned_layers(&mut self) {
        if self.positioned_layers.is_empty() {
            return;
        }
        let mut positioned_layers = std::mem::take(&mut self.positioned_layers);
        positioned_layers.sort_by_key(|layer| (layer.page_index, layer.z_index));
        for layer in positioned_layers {
            let fragment = positioned_layer_fragment(&layer);
            let target_page = if layer.page_index < self.pages.len() {
                &mut self.pages[layer.page_index]
            } else {
                &mut self.current_page
            };
            let recorded = target_page.record_paint_fragment(&fragment, 0.0, 0.0);
            if layer.z_index < 0 {
                target_page.prepend_recorded_paint_fragment(recorded);
            } else {
                target_page.append_recorded_paint_fragment(recorded);
            }
        }
    }

    // CSS 2.2 Appendix E keeps positioned descendants inside their ancestor
    // stacking context. During out-of-flow layout, replay only the layers that
    // were created by that subtree so they become part of the parent's fragment
    // instead of leaking into the page-level stacking context.
    pub(super) fn flush_positioned_layers_since(&mut self, start_index: usize) {
        if start_index >= self.positioned_layers.len() {
            return;
        }
        let mut subtree_layers = self.positioned_layers.split_off(start_index);
        subtree_layers.sort_by_key(|layer| layer.z_index);
        for layer in subtree_layers {
            let fragment = positioned_layer_fragment(&layer);
            self.current_page.append_paint_fragment(&fragment, 0.0, 0.0);
        }
    }

    fn apply_fixed_layers_to_pages(&mut self) {
        if self.fixed_layers.is_empty() {
            return;
        }
        self.fixed_layers.sort_by_key(|layer| layer.z_index);
        let fixed_layers = self.fixed_layers.clone();
        for page in &mut self.pages {
            for layer in &fixed_layers {
                append_fixed_layer_to_page(page, layer);
            }
        }
    }
}

fn page_for_context(context: PageContext) -> Page {
    let mut page = Page::new(context.size.width, context.size.height);
    page.rotation = context.rotation;
    page
}

fn canvas_background_style(style: &ComputedStyle) -> ComputedStyle {
    let mut style = style.clone();
    style.border_widths = css::Edges::ZERO;
    style.border_styles = css::BorderStyles::NONE;
    style.border_width = 0.0;
    style.border_image = css::BorderImage::initial();
    style
}

fn page_style_has_visible_paint(style: &ComputedStyle) -> bool {
    style.background_color.is_some_and(Color::is_visible)
        || style.background_image.is_some()
        || used_border_width(style) > 0.0
        || style.border_image.source.is_some()
}

fn target_element_text(element: &Element) -> String {
    let mut output = String::new();
    for child in &element.children {
        collect_target_element_text(child, &mut output);
    }
    collapse_whitespace(&output)
}

fn collect_target_element_text(node: &Node, output: &mut String) {
    match &node.kind {
        NodeKind::Text(text) => {
            output.push_str(text);
            output.push(' ');
        }
        NodeKind::Element(element) => {
            for child in &element.children {
                collect_target_element_text(child, output);
            }
        }
    }
}

/// Resolves CSS page border and padding declarations to used physical edges.
///
/// CSS Paged Media applies border and padding to the page box, and CSS Box
/// Model resolves padding percentages against the containing block's inline
/// size before layout consumes used values:
/// <https://www.w3.org/TR/css-page-3/#page-model> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(super) fn page_box_edges_from_declarations(
    declarations: &Declarations,
    page_size: PageSize,
) -> PageBoxEdges {
    if declarations.is_empty() {
        return PageBoxEdges::ZERO;
    }
    let mut style = ComputedStyle::initial();
    css::apply_declarations(&mut style, declarations);
    PageBoxEdges {
        border: used_border_widths(&style),
        padding: css::page_padding_from_for_size(declarations, page_size),
    }
}

/// Used page background positioning area.
///
/// CSS Backgrounds and Borders defines `background-origin` as selecting the
/// border, padding, or content box used for background image positioning. CSS
/// Paged Media applies that box model to page boxes:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PageBackgroundArea {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl PageBackgroundArea {
    fn inset(self, edges: css::Edges) -> Self {
        Self {
            x: self.x + edges.left,
            y: self.y + edges.bottom,
            width: (self.width - edges.left - edges.right).max(0.0),
            height: (self.height - edges.top - edges.bottom).max(0.0),
        }
    }
}

/// Resolves the page background positioning area selected by `background-origin`.
///
/// For a page box, the border box starts inside the page margins, the padding
/// box is inset by page borders, and the content box is additionally inset by
/// page padding:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
fn page_background_positioning_area(
    declarations: &Declarations,
    base_margins: PageMargins,
    page_size: PageSize,
    origin: css::BackgroundBox,
) -> PageBackgroundArea {
    let edges = page_box_edges_from_declarations(declarations, page_size);
    let margins = css::page_margins_from_for_size_and_edges(
        declarations,
        base_margins,
        page_size,
        edges.total(),
    );
    let border_box = PageBackgroundArea {
        x: margins.left,
        y: margins.bottom,
        width: (page_size.width - margins.left - margins.right).max(0.0),
        height: (page_size.height - margins.top - margins.bottom).max(0.0),
    };

    match origin {
        css::BackgroundBox::Border => border_box,
        css::BackgroundBox::Padding => border_box.inset(edges.border),
        css::BackgroundBox::Content => border_box.inset(edges.border).inset(edges.padding),
    }
}

/// Returns whether a forced break target is satisfied by the next page number.
///
/// CSS Fragmentation defines `left`/`right` as spread sides and `recto`/`verso`
/// as first/opposite page sides in the current page progression:
/// <https://www.w3.org/TR/css-break-3/#valdef-break-before-recto> and
/// <https://www.w3.org/TR/css-page-3/#spread-pseudos>.
fn forced_break_satisfied(
    forced_break: PageBreak,
    next_page_number: usize,
    page_progression_direction: Direction,
) -> bool {
    let is_left = page_is_left(next_page_number, page_progression_direction);
    match forced_break {
        PageBreak::Auto | PageBreak::Avoid | PageBreak::Page => true,
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
fn page_is_left(page_number: usize, page_progression_direction: Direction) -> bool {
    match page_progression_direction {
        Direction::Ltr => page_number.is_multiple_of(2),
        Direction::Rtl => !page_number.is_multiple_of(2),
    }
}

fn anonymous_block_is_plain_text_with_style(
    children: &[box_tree::FormattingBox<'_>],
    style: &ComputedStyle,
) -> bool {
    children
        .iter()
        .all(|child| matches!(child, box_tree::FormattingBox::Text(box_) if box_.style == *style))
}

/// Returns whether a page is the recto side for forced recto/verso breaks.
///
/// CSS Fragmentation maps `recto` to the first side of a spread in the current
/// page progression and `verso` to the opposite side:
/// <https://www.w3.org/TR/css-break-3/#valdef-break-before-recto>.
fn is_recto_page(page_number: usize, page_progression_direction: Direction) -> bool {
    match page_progression_direction {
        Direction::Ltr => !page_is_left(page_number, page_progression_direction),
        Direction::Rtl => page_is_left(page_number, page_progression_direction),
    }
}

fn append_fixed_layer_to_page(page: &mut Page, layer: &FixedPaintLayer) {
    let fragment = fixed_layer_fragment(layer);
    let recorded = page.record_paint_fragment(&fragment, 0.0, 0.0);
    if layer.z_index < 0 {
        page.prepend_recorded_paint_fragment(recorded);
    } else {
        page.append_recorded_paint_fragment(recorded);
    }
}

fn positioned_layer_fragment(layer: &PositionedPaintLayer) -> PaintFragment {
    PaintFragment::from_stacking_context(layer.context.clone().with_links(layer.links.clone()))
}

fn fixed_layer_fragment(layer: &FixedPaintLayer) -> PaintFragment {
    PaintFragment::from_stacking_context(layer.context.clone().with_links(layer.links.clone()))
}
