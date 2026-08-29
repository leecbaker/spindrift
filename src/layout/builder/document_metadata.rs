use super::*;

pub(in crate::layout) fn target_element_text(element: &Element) -> String {
    let mut output = String::new();
    for child in &element.children {
        collect_target_element_text(child, &mut output);
    }
    collapse_whitespace(&output)
}

pub(in crate::layout) fn collect_target_element_text(node: &Node, output: &mut String) {
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

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn add_bookmark(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        position: PaintPoint,
    ) {
        if self.element_side_effect_suppression_depth > 0 {
            return;
        }
        let css::BookmarkLevel::Level(level) = style.bookmark_level else {
            return;
        };
        if style.display.is_none() || style.visibility != Visibility::Visible {
            return;
        }
        let label = collapse_whitespace(&evaluate_bookmark_label(element, style));
        if label.is_empty() {
            return;
        }
        self.bookmarks.push(Bookmark::new(
            level.get(),
            label,
            self.pages.len(),
            position.x,
            position.y,
            match style.bookmark_state {
                CssBookmarkState::Open => BookmarkState::Open,
                CssBookmarkState::Closed => BookmarkState::Closed,
            },
        ));
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
    pub(in crate::layout) fn add_page_anchor(&mut self, element: &Element, style: &ComputedStyle) {
        if self.element_side_effect_suppression_depth > 0 {
            return;
        }
        if let Some(id) = element.attrs.get("id").filter(|value| !value.is_empty()) {
            self.record_page_anchor(id.clone(), element, style);
        }
        if element.tag.eq_ignore_ascii_case("a")
            && let Some(name) = element.attrs.get("name").filter(|value| !value.is_empty())
        {
            self.record_page_anchor(name.clone(), element, style);
        }
    }

    fn record_page_anchor(&mut self, name: String, element: &Element, style: &ComputedStyle) {
        self.page_anchors
            .entry(name.clone())
            .or_insert(self.pages.len());
        self.page_anchor_source_positions
            .entry(name.clone())
            .or_insert_with(|| PaintPoint::new(self.content_left, self.cursor_y));
        if !self.page_anchor_text.contains_key(&name) {
            let anchor_text = self.anchor_text_for_element(element, style);
            self.page_anchor_text.insert(name.clone(), anchor_text);
        }
        let counters =
            self.counter_stacks_at_origin(element, box_tree::CounterEventSource::Principal);
        self.page_anchor_counters.entry(name).or_insert(counters);
    }

    /// Captures text exposed to generated-content cross references.
    ///
    /// CSS Generated Content for Paged Media defines `target-text()` keywords
    /// for target element content and generated `::before`/`::after` text. This
    /// helper records those values at layout time so page-margin generated
    /// content can resolve them after pagination:
    /// <https://www.w3.org/TR/css-gcpm-3/#target-text>.
    pub(in crate::layout) fn anchor_text_for_element(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) -> AnchorText {
        AnchorText {
            content: target_element_text(element),
            before: self.evaluate_generated_pseudo_text_rollback(
                element,
                box_tree::CounterEventSource::Before,
                style.before_style.as_deref(),
            ),
            after: self.evaluate_generated_pseudo_text_rollback(
                element,
                box_tree::CounterEventSource::After,
                style.after_style.as_deref(),
            ),
        }
    }
}
