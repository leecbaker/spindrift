use super::*;

/// The page value associated with one side of a structural class-A boundary.
///
/// `Inherited` is deliberately distinct from `Auto`: a box with an omitted
/// `page` declaration still participates in the boundary, but its used value
/// has to be resolved from the active lexical page-value scope. An explicit
/// `page: auto` resolves to the nearest non-auto ancestor while retaining its
/// structural identity. `Inapplicable` is used for a box that
/// cannot establish a page boundary at all (for example text before it is
/// wrapped in its anonymous block).
///
/// CSS Paged Media's page-group algorithm uses the used value for the
/// boundary comparison while preserving this lexical distinction:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::layout) enum PageBoundaryValue {
    Inapplicable,
    Inherited,
    Auto,
    Named(String),
}

impl PageBoundaryValue {
    pub(in crate::layout) fn from_style(style: &ComputedStyle) -> Self {
        if !style.page.is_specified() {
            return Self::Inherited;
        }
        style
            .page
            .specified_name()
            .map(|name| name.as_str().to_string())
            .map(Self::Named)
            .unwrap_or(Self::Auto)
    }

    /// Whether this child value replaces its parent's structural start/end
    /// summary. An inherited child has no authored value to propagate.
    pub(in crate::layout) fn overrides_parent_summary(&self) -> bool {
        matches!(self, Self::Auto | Self::Named(_))
    }
}

/// First and last page-boundary values propagated from one formatting box.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::layout) struct PageBoundaryValues {
    pub(in crate::layout) start: PageBoundaryValue,
    pub(in crate::layout) end: PageBoundaryValue,
}

/// The used page types at the two propagated sides of a formatting box.
///
/// Unlike [`PageBoundaryValues`], this record contains no authored-value
/// placeholders. `auto` has already been resolved against the nearest
/// non-auto ancestor in the *formatting tree*, before layout starts moving the
/// page cursor. This is the value class-A break selection compares.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::layout) struct ResolvedPageBoundaryValues {
    pub(in crate::layout) start: Option<String>,
    pub(in crate::layout) end: Option<String>,
}

impl ResolvedPageBoundaryValues {
    pub(in crate::layout) fn uniform(page_name: Option<String>) -> Self {
        Self {
            start: page_name.clone(),
            end: page_name,
        }
    }

    pub(in crate::layout) fn inapplicable() -> Self {
        Self::uniform(None)
    }
}

impl PageBoundaryValues {
    pub(in crate::layout) fn inapplicable() -> Self {
        Self {
            start: PageBoundaryValue::Inapplicable,
            end: PageBoundaryValue::Inapplicable,
        }
    }

    pub(in crate::layout) fn from_style(style: &ComputedStyle) -> Self {
        let own = PageBoundaryValue::from_style(style);
        Self {
            start: own.clone(),
            end: own,
        }
    }
}

/// Returns first/last CSS `page` boundary values.
///
/// CSS Paged Media's `auto` page value can explicitly end an ancestor named
/// page group, while an omitted `page` declaration inherits the surrounding
/// page group at a class-A boundary. Layout therefore has to preserve the
/// distinction instead of flattening both to `None`:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn page_value_sources_from_style_and_children(
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> PageBoundaryValues {
    let PageBoundaryValues { mut start, mut end } = PageBoundaryValues::from_style(style);
    if style.display.is_flex() {
        return PageBoundaryValues { start, end };
    }
    // Ignore transparent wrappers rooted in boxes to which `page` cannot
    // apply (such as an absolutely positioned descendant), but retain an
    // inline atomic box as an inapplicable first/last participant. The latter
    // makes its parent fall back to its own used value rather than allowing a
    // later named sibling to select the document's first page.
    // <https://drafts.csswg.org/css-page-3/#using-named-pages>
    let mut normal_flow_children = child_boxes
        .iter()
        .filter(|child| formatting_box_is_page_value_participant(child));
    let Some(first) = normal_flow_children.next() else {
        return PageBoundaryValues { start, end };
    };
    let first_sources = formatting_box_page_value_sources(first);
    if first_sources.start.overrides_parent_summary() {
        start = first_sources.start.clone();
    }
    // A single normal-flow child supplies both boundaries. Compute its paired
    // summary once: recursively querying it separately for start and end
    // would revisit the same sole-child chain twice at every depth.
    let last_sources = normal_flow_children
        .next_back()
        .map(formatting_box_page_value_sources)
        .unwrap_or(first_sources);
    if last_sources.end.overrides_parent_summary() {
        end = last_sources.end;
    }
    PageBoundaryValues { start, end }
}

/// Returns page-value sources for an element's box, retaining leading direct
/// inline content as the element's first page-group participant.
///
/// Formatting-tree normalization stores direct text separately from the
/// element's block-level child boxes. A later descendant with `page: <name>`
/// must not make the document root select that named page before direct text
/// which precedes it in tree order. That text belongs to the initial `auto`
/// page group and establishes the class-A boundary before the named child.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
pub(in crate::layout) fn page_value_sources_from_element_style_and_children(
    element: &Element,
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
) -> PageBoundaryValues {
    let mut sources = page_value_sources_from_style_and_children(style, child_boxes);
    if !style.page.is_specified() && element_has_leading_direct_inline_content(element, style) {
        sources.start = PageBoundaryValue::Inherited;
    }
    sources
}

/// Whether direct inline content precedes the first element child.
///
/// Inline element children remain represented in the formatting tree and are
/// therefore covered by `page_value_sources_from_style_and_children`. This
/// helper accounts only for direct text and `<br>` nodes, which normalization
/// otherwise keeps outside that child-box sequence.
fn element_has_leading_direct_inline_content(element: &Element, style: &ComputedStyle) -> bool {
    if element_suppresses_direct_text_children(element) {
        return false;
    }
    let mut text = String::new();
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(value) => text.push_str(value),
            NodeKind::Element(child) if is_line_break_element(child) => text.push(INLINE_BREAK),
            NodeKind::Element(_) => break,
        }
    }
    !normalized_text_for_style(&text, style).is_empty()
}

pub(in crate::layout) fn formatting_box_page_value_sources(
    box_: &box_tree::FormattingBox<'_>,
) -> PageBoundaryValues {
    match box_ {
        box_tree::FormattingBox::Block(box_) => page_value_sources_from_element_style_and_children(
            box_.core.element,
            &box_.core.style,
            &box_.core.children,
        ),
        // `page` does not apply to inline boxes. Keep their normal-flow
        // descendants, which may themselves establish class-A boundaries,
        // but discard the inline box's authored value.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        box_tree::FormattingBox::Inline(box_) => {
            let mut descendant_style = box_.core.style.as_ref().clone();
            descendant_style.page = css::PageAssignment::Unspecified;
            page_value_sources_from_element_style_and_children(
                box_.core.element,
                &descendant_style,
                &box_.core.children,
            )
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            let mut descendant_style = box_.core.style.as_ref().clone();
            descendant_style.page = css::PageAssignment::Unspecified;
            page_value_sources_from_element_style_and_children(
                box_.core.element,
                &descendant_style,
                &box_.core.children,
            )
        }
        // Atomic inline formatting contexts do not establish class-A page
        // boundaries in their parent flow. Their descendants remain atomic
        // with the inline box rather than propagating page groups outward.
        // <https://drafts.csswg.org/css-page-3/#using-named-pages>
        box_tree::FormattingBox::AtomicInline(_) => PageBoundaryValues::inapplicable(),
        box_tree::FormattingBox::Flex(box_) => PageBoundaryValues::from_style(&box_.core.style),
        // A table's durable fragment is the CSS table formatting tree. Its
        // generic core children still describe the pre-fixup element tree and
        // can therefore omit or mis-order the effective row boundaries.
        // Named-page propagation must follow the same row sequence table
        // layout fragments.
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        box_tree::FormattingBox::Table(box_) => {
            table::table_page_boundary_summary(&box_.fragment, &box_.core.style, None).sources
        }
        box_tree::FormattingBox::Replaced(box_) => PageBoundaryValues::from_style(&box_.core.style),
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            page_value_sources_from_style_and_children(&box_.style, &box_.children)
        }
        box_tree::FormattingBox::Text(_) => PageBoundaryValues::inapplicable(),
    }
}

/// Resolves the used `page` value for one style without consulting the output
/// page cursor. CSS Paged Media resolves `auto` to the nearest ancestor whose
/// used page value is not `auto`; this lexical operation must happen before
/// start/end values are propagated through a formatting tree.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
fn resolved_page_name_for_style(
    style: &ComputedStyle,
    inherited_page_name: Option<&str>,
) -> Option<String> {
    style
        .page
        .effective_name(inherited_page_name.map(str::to_string))
}

/// Resolves start/end page types for a style and its formatting-tree children.
///
/// The structural source helper retains whether a descendant's `auto` or
/// named value replaces its parent summary. This helper supplies the missing
/// lexical half: every recursive call receives the parent's already-used page
/// type, so a deep `page:auto` cannot accidentally bind to the mutable page
/// selected by an unrelated preceding sibling.
pub(in crate::layout) fn resolved_page_boundary_values_from_style_and_children(
    style: &ComputedStyle,
    child_boxes: &[box_tree::FormattingBox<'_>],
    inherited_page_name: Option<&str>,
) -> ResolvedPageBoundaryValues {
    let own_page_name = resolved_page_name_for_style(style, inherited_page_name);
    let mut values = ResolvedPageBoundaryValues::uniform(own_page_name.clone());
    if style.display.is_flex() {
        return values;
    }
    let mut normal_flow_children = child_boxes
        .iter()
        .filter(|child| formatting_box_is_page_value_participant(child));
    let Some(first) = normal_flow_children.next() else {
        return values;
    };
    let first_sources = formatting_box_page_value_sources(first);
    let first_values =
        resolved_formatting_box_page_boundary_values(first, own_page_name.as_deref());
    if first_sources.start.overrides_parent_summary() {
        values.start = first_values.start;
    }
    let last = normal_flow_children.next_back().unwrap_or(first);
    let last_sources = formatting_box_page_value_sources(last);
    if last_sources.end.overrides_parent_summary() {
        values.end =
            resolved_formatting_box_page_boundary_values(last, own_page_name.as_deref()).end;
    }
    values
}

/// Resolves propagated class-A start/end page types for one formatting box.
///
/// This deliberately follows formatting boxes rather than DOM children so
/// display-none, floated, positioned, and otherwise non-participating boxes
/// never take part in named-page propagation.
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>
pub(in crate::layout) fn resolved_formatting_box_page_boundary_values(
    box_: &box_tree::FormattingBox<'_>,
    inherited_page_name: Option<&str>,
) -> ResolvedPageBoundaryValues {
    match box_ {
        box_tree::FormattingBox::Block(box_) => {
            let mut values = resolved_page_boundary_values_from_style_and_children(
                &box_.core.style,
                &box_.core.children,
                inherited_page_name,
            );
            if !box_.core.style.page.is_specified()
                && element_has_leading_direct_inline_content(box_.core.element, &box_.core.style)
            {
                values.start = resolved_page_name_for_style(&box_.core.style, inherited_page_name);
            }
            values
        }
        box_tree::FormattingBox::Inline(box_) => {
            let mut style = box_.core.style.as_ref().clone();
            style.page = css::PageAssignment::Unspecified;
            let mut values = resolved_page_boundary_values_from_style_and_children(
                &style,
                &box_.core.children,
                inherited_page_name,
            );
            if element_has_leading_direct_inline_content(box_.core.element, &style) {
                values.start = resolved_page_name_for_style(&style, inherited_page_name);
            }
            values
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            let mut style = box_.core.style.as_ref().clone();
            style.page = css::PageAssignment::Unspecified;
            let mut values = resolved_page_boundary_values_from_style_and_children(
                &style,
                &box_.core.children,
                inherited_page_name,
            );
            if element_has_leading_direct_inline_content(box_.core.element, &style) {
                values.start = resolved_page_name_for_style(&style, inherited_page_name);
            }
            values
        }
        box_tree::FormattingBox::AtomicInline(_) => ResolvedPageBoundaryValues::inapplicable(),
        box_tree::FormattingBox::Flex(box_) => ResolvedPageBoundaryValues::uniform(
            resolved_page_name_for_style(&box_.core.style, inherited_page_name),
        ),
        box_tree::FormattingBox::Table(box_) => {
            table::table_page_boundary_summary(
                &box_.fragment,
                &box_.core.style,
                inherited_page_name,
            )
            .resolved
        }
        box_tree::FormattingBox::Replaced(box_) => ResolvedPageBoundaryValues::uniform(
            resolved_page_name_for_style(&box_.core.style, inherited_page_name),
        ),
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            resolved_page_boundary_values_from_style_and_children(
                &box_.style,
                &box_.children,
                inherited_page_name,
            )
        }
        box_tree::FormattingBox::Text(_) => ResolvedPageBoundaryValues::inapplicable(),
    }
}

/// Whether a formatting box can contribute a propagated start/end `page`
/// value to its parent.
///
/// CSS Paged Media propagates values only from child boxes to which `page`
/// applies. Anonymous wrappers generated solely around formatting whitespace or
/// out-of-flow descendants create no class-A boundary, so they are transparent
/// for this purpose:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn formatting_box_is_page_value_participant(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    // Anonymous inline runs participate in the class-A boundary surrounding
    // their containing block even though a text formatting box is not a
    // normal-flow *box* for the generic flow helper.  Otherwise a named
    // block followed by direct text has no succeeding page value, and the
    // return to the parent's used page type is never forced.
    // <https://www.w3.org/TR/css-page-3/#using-named-pages>
    if let box_tree::FormattingBox::Text(box_) = box_ {
        return !(box_.text.is_empty()
            || (box_.style.white_space.collapses_spaces()
                && box_.text.chars().all(is_css_collapsible_whitespace)));
    }
    if !formatting_box_is_in_normal_flow(box_) {
        return false;
    }
    match box_ {
        box_tree::FormattingBox::AnonymousBlock(box_) => box_
            .children
            .iter()
            .any(formatting_box_is_page_value_participant),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_
            .core
            .children
            .iter()
            .any(formatting_box_is_page_value_participant),
        _ => !formatting_box_can_only_create_phantom_line_boxes(box_),
    }
}

/// Returns the first and last effective CSS `page` values for one formatting box.
///
/// Absolutely positioned, fixed-position, floated, running, and display-none
/// boxes are not in normal flow and therefore do not create class A sibling
/// page-name boundaries:
/// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
#[cfg(test)]
pub(in crate::layout) fn formatting_box_page_values(
    box_: &box_tree::FormattingBox<'_>,
) -> (Option<String>, Option<String>) {
    let sources = formatting_box_page_value_sources(box_);
    let name = |source: PageBoundaryValue| match source {
        PageBoundaryValue::Named(name) => Some(name),
        PageBoundaryValue::Inapplicable
        | PageBoundaryValue::Inherited
        | PageBoundaryValue::Auto => None,
    };
    (name(sources.start), name(sources.end))
}

pub(in crate::layout) fn formatting_box_is_in_normal_flow(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    !matches!(box_, box_tree::FormattingBox::Text(_)) && style_is_in_normal_flow(box_.style())
}

/// Returns true for an explicit zero-height page-owning block boundary.
///
/// CSS Paged Media forms page groups at class A break opportunities, but WPT
/// `page-name-zero-height-001-print.html` treats consecutive `height: 0`
/// page-owning siblings as not forcing separate page boxes. Their overflowing
/// contents are laid out in the next nonzero page group:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn formatting_box_is_zero_height_page_boundary(
    box_: &box_tree::FormattingBox<'_>,
) -> bool {
    let Some((_, _, style, _)) = box_.element_parts() else {
        return false;
    };
    formatting_box_is_in_normal_flow(box_)
        && style.page.is_specified()
        && style
            .box_values
            .height
            .length_if_no_percent()
            .is_some_and(|height| height.abs() < 0.01)
}

/// Finds the page value that a zero-height page-owning sibling run coalesces into.
///
/// Consecutive explicit zero-height page-owning boxes do not each create a
/// separate page group. The run is laid out in the next nonzero in-flow
/// sibling's start page group when one exists, otherwise in the current box's
/// own start group:
/// <https://www.w3.org/TR/css-page-3/#using-named-pages>.
pub(in crate::layout) fn coalesced_zero_height_page_start(
    child_boxes: &[box_tree::FormattingBox<'_>],
    current_index: usize,
    inherited_page_name: Option<&str>,
) -> Option<String> {
    child_boxes
        .iter()
        .skip(current_index + 1)
        .filter(|child| formatting_box_is_in_normal_flow(child))
        .find(|child| !formatting_box_is_zero_height_page_boundary(child))
        .map(|child| resolved_formatting_box_page_boundary_values(child, inherited_page_name).start)
        .unwrap_or_else(|| {
            resolved_formatting_box_page_boundary_values(
                &child_boxes[current_index],
                inherited_page_name,
            )
            .start
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_parent_style() -> ComputedStyle {
        ComputedStyle {
            font_size: 12.0,
            line_height: 14.4,
            color: CssColor::BLACK,
            ..ComputedStyle::initial()
        }
    }

    #[test]
    fn atomic_inline_page_values_do_not_escape_its_atomic_formatting_context() {
        let root = dom::parse(
            "<html><body>\
             <div style=\"page:c; display:inline-block\">\
               <div style=\"page:a\">A</div>\
               <div style=\"page:b\">B</div>\
             </div>\
             <div style=\"page:c\">C</div>\
             </body></html>",
        );
        let stylesheets = Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
        let page = box_tree::freeze_page_box(box_tree::build_page_box(
            &root,
            &stylesheets,
            &test_parent_style(),
        ));
        let body = &page.children[0].children()[0];
        let anonymous_inline_run = &body.children()[0];

        assert_eq!(
            formatting_box_page_values(anonymous_inline_run),
            (None, None)
        );
        assert_eq!(
            formatting_box_page_values(&body.children()[1]),
            (Some("c".to_string()), Some("c".to_string()))
        );
    }

    #[test]
    fn deeply_nested_inline_page_values_do_not_repeat_single_child_traversal() {
        // The formatting tree itself is recursive, while the regression
        // verifies its named-page summary no longer branches exponentially.
        std::thread::Builder::new()
            .name("deep-page-value-regression".to_string())
            .spawn(|| {
                // WPT: css/css-zoom/crashtests/zoom-deeply-nested.html. The
                // CSS declaration is incidental: the regression is a
                // sole-child chain in named-page summary traversal.
                let spans = "<span>".repeat(40);
                let closing_spans = "</span>".repeat(40);
                let root = dom::parse(&format!(
                    "<html><style>span {{ zoom: .1%; }}</style><body>{spans}text{closing_spans}</body></html>"
                ));
                let stylesheets =
                    Stylesheets::for_document(css::html5_user_agent_stylesheet(), None, &[]);
                let page = box_tree::freeze_page_box(box_tree::build_page_box(
                    &root,
                    &stylesheets,
                    &test_parent_style(),
                ));
                let body = &page.children[0].children()[0];

                assert_eq!(
                    formatting_box_page_value_sources(body),
                    PageBoundaryValues {
                        start: PageBoundaryValue::Inherited,
                        end: PageBoundaryValue::Inherited,
                    }
                );
            })
            .expect("deep page-value regression thread should start")
            .join()
            .expect("deep page-value regression thread should complete");
    }
}
