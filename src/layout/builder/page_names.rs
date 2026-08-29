use super::*;

#[derive(Debug, Clone)]
pub(in crate::layout) enum PageNameScope {
    Element,
    Inline { previous_page_name: Option<String> },
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn exit_inline_page_name_scope(&mut self, scope: Option<PageNameScope>) {
        let Some(PageNameScope::Inline { previous_page_name }) = scope else {
            return;
        };
        if self.current_page_has_content() {
            self.push_page_if_nonempty();
        }
        self.enter_page_name_scope_for_value(previous_page_name.as_deref());
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
    pub(in crate::layout) fn push_page_name_scope_suppression(&mut self) {
        self.page_name_scope_suppression += 1;
    }

    /// Re-enables CSS named-page group creation after suppressed layout.
    ///
    /// This closes the temporary suppression scope opened for out-of-flow or
    /// atomic inline formatting-context layout:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn pop_page_name_scope_suppression(&mut self) {
        self.page_name_scope_suppression = self.page_name_scope_suppression.saturating_sub(1);
    }

    /// Enters the lexical used-value scope for the CSS `page` property.
    ///
    /// CSS Paged Media resolves `page:auto` from the nearest non-`auto`
    /// ancestor. That lexical value survives a descendant's temporary page
    /// group, so it cannot be recovered from the current page cursor:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn push_page_value_scope(&mut self, style: &ComputedStyle) {
        let inherited = self.page_value_scope_stack.last().cloned().flatten();
        // An explicitly specified `page: auto` differs structurally from an
        // omitted declaration, but its *used* value is the nearest non-auto
        // ancestor page name. Keep the resolved lexical value in this stack;
        // `PageBoundaryValue::Auto` preserves the authored distinction at the
        // class-A boundary.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        let used = style.page.effective_name(inherited);
        self.page_value_scope_stack.push(used);
    }

    /// Leaves one lexical CSS `page` used-value scope.
    pub(in crate::layout) fn pop_page_value_scope(&mut self) {
        self.page_value_scope_stack.pop();
    }

    /// Resolves a class-A child boundary in the active lexical page scope.
    pub(in crate::layout) fn page_boundary_name_in_active_scope(
        &self,
        source: PageBoundaryValue,
        parent_style: &ComputedStyle,
    ) -> Option<String> {
        match source {
            PageBoundaryValue::Named(name) => Some(name),
            PageBoundaryValue::Inapplicable => None,
            PageBoundaryValue::Auto | PageBoundaryValue::Inherited => self
                .page_value_scope_stack
                .last()
                .cloned()
                .flatten()
                .or_else(|| {
                    parent_style
                        .page
                        .specified_name()
                        .map(|name| name.as_str().to_string())
                }),
        }
    }

    /// Returns the used page type inherited by a formatting-tree child.
    ///
    /// Boundary propagation resolves this once, before a class-A transition
    /// materializes a destination page. Keeping it separate from
    /// `current_page_name` prevents a preceding sibling's output page from
    /// becoming an ancestor for a later descendant's `page:auto`.
    pub(in crate::layout) fn active_page_value_scope(
        &self,
        parent_style: &ComputedStyle,
    ) -> Option<String> {
        self.page_value_scope_stack
            .last()
            .cloned()
            .flatten()
            .or_else(|| {
                parent_style
                    .page
                    .specified_name()
                    .map(|name| name.as_str().to_string())
            })
    }

    /// Suppresses element-entry named-page scopes while preserving sibling switches.
    ///
    /// Flex items do not expose their own `page` value, or descendant-derived
    /// first/last page values, to the flex container boundary. Class A break
    /// opportunities between ordinary block descendants inside the flex item
    /// still select named page groups:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
    pub(in crate::layout) fn push_page_name_element_scope_suppression(&mut self) {
        self.page_name_element_scope_suppression += 1;
    }

    /// Re-enables element-entry named-page scopes after isolated item layout.
    ///
    /// This closes the flex-item page-scope isolation described by CSS Paged
    /// Media named pages and CSS Flexbox pagination:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
    pub(in crate::layout) fn pop_page_name_element_scope_suppression(&mut self) {
        self.page_name_element_scope_suppression =
            self.page_name_element_scope_suppression.saturating_sub(1);
    }

    /// Selects the destination named-page group at an already-established
    /// class-A boundary.
    ///
    /// The page that is being completed keeps `current_page_name`; the
    /// destination page context must instead be resolved from `page_name`
    /// before it is materialized.  Updating the cursor first made the source
    /// page acquire the destination type, while updating it after a generic
    /// page push made the destination inherit the source context.
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages>
    pub(in crate::layout) fn enter_page_name_scope_for_value(
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
        // A named-page boundary is a class-A break between normal-flow boxes.
        // Prior out-of-flow paint (for example a float) remains on the current
        // page but does not by itself establish a preceding page group that
        // the next in-flow box must break away from. Conversely, once a named
        // page has been selected, its out-of-flow paint materializes that page
        // context and it must be committed before a later class-A transition.
        // This keeps page-type selection separate from normal-flow geometry:
        // `page` applies to Class-A boxes even when their only paint is an
        // absolutely positioned descendant.
        // <https://www.w3.org/TR/css-break-3/#possible-breaks>
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        // The page context records whether the source page was selected as a
        // named page independently of normal-flow occupancy. In particular,
        // positioned descendants are laid out after their source's page-value
        // boundaries have been determined, so their paint cannot be used to
        // decide whether that named page must be committed.
        let materialized_destination = self.current_page_has_named_page_flow_content
            || self.current_page_selected_name.is_some();
        let replacing_committed_empty_page =
            !materialized_destination && !self.current_page_has_content() && !self.pages.is_empty();
        let empty_page_selected_by_named_boundary = replacing_committed_empty_page
            && self.page_names.last().map(Option::as_deref)
                != Some(self.current_page_name.as_deref());
        if materialized_destination {
            self.push_page_for_page_name(page_name);
        } else if empty_page_selected_by_named_boundary {
            // Preserve the preceding empty named group's structural end
            // value while replacing its unpainted page with the successor's
            // continuation context.
            self.push_page_for_page_name(page_name);
        }
        self.current_page_name = page_name.map(str::to_string);
        // A first-page replacement occurs before its root/body fragment is
        // materialized, so it must retain that fragment's document-canvas
        // insets. A committed class-A transition already has a fresh
        // destination context from `push_page_for_page_name`; rebuilding it
        // would remeasure those source-page insets against the destination
        // page area and shift its first line.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        if !materialized_destination {
            if replacing_committed_empty_page {
                if !empty_page_selected_by_named_boundary {
                    self.select_named_page_for_committed_empty_page();
                }
            } else {
                self.rebuild_empty_current_page_context();
            }
        }
        Some(previous)
    }

    pub(in crate::layout) fn exit_page_name_scope(&mut self, _scope: Option<PageNameScope>) {
        // Element scope exit only restores lexical CSS-value state (handled by
        // `pop_page_value_scope`). A page is selected exclusively by the
        // parent formatting context's class-A preceding-end/succeeding-start
        // comparison; changing `current_page_name` here manufactures an extra
        // boundary for nested page groups and `page:auto`.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
    }

    pub(in crate::layout) fn enter_page_name_scope(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Option<PageNameScope> {
        if self.page_name_scope_suppression > 0 || self.page_name_element_scope_suppression > 0 {
            return None;
        }
        if style.display.is_none()
            || matches!(style.position, Position::Absolute | Position::Fixed)
            || style.float != Float::None
            || style.position.is_running()
        {
            return None;
        }
        let page_value_sources = page_value_sources_from_element_style_and_children(
            element,
            style,
            child_boxes.unwrap_or_default(),
        );
        let start_page_name = match &page_value_sources.start {
            PageBoundaryValue::Named(name) => Some(name.as_str()),
            PageBoundaryValue::Inapplicable
            | PageBoundaryValue::Inherited
            | PageBoundaryValue::Auto => None,
        };
        let end_page_name = match page_value_sources.end {
            PageBoundaryValue::Named(name) => Some(name),
            PageBoundaryValue::Inapplicable
            | PageBoundaryValue::Inherited
            | PageBoundaryValue::Auto => None,
        };
        if !style.page.is_specified() && start_page_name.is_none() && end_page_name.is_none() {
            return None;
        }
        // Element scopes establish lexical `page` used-value resolution, but
        // do not themselves materialize a page. The parent formatting
        // context owns the class-A boundary and compares this box's
        // propagated start value with its preceding sibling's end value.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        Some(PageNameScope::Element)
    }

    /// Switches named page groups at a class A sibling page-break boundary.
    ///
    /// CSS Paged Media defines `page` transitions at possible page-break
    /// points between block-level siblings, using the previous sibling's end
    /// page value and the next sibling's start page value:
    /// <https://www.w3.org/TR/css-page-3/#using-named-pages> and
    /// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
    pub(in crate::layout) fn switch_page_name_at_class_a_boundary(
        &mut self,
        page_name: Option<&str>,
    ) {
        if self.page_name_scope_suppression > 0 || self.fragmentation_suppression_depth > 0 {
            return;
        }
        // A class-A page boundary belongs to the active principal
        // fragmentation flow. An orthogonal nested block progresses along a
        // different physical axis and cannot materialize a page transition by
        // itself; its parent fragmentainer owns any eventual page break.
        // <https://www.w3.org/TR/css-break-3/#possible-breaks>
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        if self.containing_block_writing_mode != self.principal_flow.writing_mode {
            return;
        }
        // A class-A boundary belongs to the participating boxes in that
        // principal fragmentation flow, rather than to a physical-Y cursor.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        if self.principal_flow.writing_mode != WritingMode::HorizontalTb {
            // The root start value chooses the first page type but does not
            // itself cross a fragmentainer boundary.  Moving the vertical
            // block cursor here made the first named child disappear before
            // it could contribute any content.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            if !self.current_page_has_named_page_flow_content {
                self.enter_page_name_scope_for_value(page_name);
                return;
            }
            // The page fragmentainer remains physically top-to-bottom even
            // when the principal block axis is horizontal. Selecting a named
            // page therefore updates the active fragment's type without
            // manufacturing an unrelated horizontal page strip; subsequent
            // vertical-flow placement remains owned by that fragmentainer.
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            // <https://www.w3.org/TR/css-page-3/#using-named-pages>
            let Some(page_name) = page_name else {
                return;
            };
            self.current_page_name = Some(page_name.to_string());
            return;
        }
        // The structural page value on the preceding side may differ from
        // the succeeding one even when both surrounding lexical scopes
        // resolve to the same materialized page name.  For example, a first
        // child can end the parent's `a` group with `b`; the following
        // inherited child must start a *new* `a` page.  Comparing only the
        // destination with `current_page_name` loses that return boundary.
        // <https://drafts.csswg.org/css-page-3/#using-named-pages>
        if self.current_page_name.as_deref() == page_name
            && self.current_page_has_named_page_flow_content
        {
            self.push_page_for_page_name(page_name);
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
    pub(in crate::layout) fn enter_inline_page_name_scope(
        &mut self,
        page_name: Option<&str>,
    ) -> Option<PageNameScope> {
        if self.page_name_scope_suppression > 0 {
            return None;
        }
        let previous = self.current_page_name.clone();
        self.enter_page_name_scope_for_value(page_name);
        Some(PageNameScope::Inline {
            previous_page_name: previous,
        })
    }
}
