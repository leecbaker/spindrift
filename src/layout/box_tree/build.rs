use super::*;

#[cfg(test)]
pub(crate) fn build_page_box<'a>(
    root: &'a Node,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
) -> MutablePageBox<'a> {
    build_page_box_inner(root, stylesheets, parent_style, None, None)
}

/// Builds the document formatting tree with the used principal-flow axes.
///
/// CSS Writing Modes substitutes an eligible HTML body's writing mode and
/// direction for the root element's used values. Supplying that substitution
/// while constructing the root box makes inherited root pseudo-elements and
/// descendants observe the same principal flow as the initial containing
/// block.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
pub(crate) fn build_page_box_with_principal_flow<'a>(
    root: &'a Node,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
    principal_flow: DocumentPrincipalFlow,
) -> MutablePageBox<'a> {
    build_page_box_inner(root, stylesheets, parent_style, None, Some(principal_flow))
}

#[cfg(test)]
pub(crate) fn build_page_box_with_font_metrics<'a>(
    root: &'a Node,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
    font_system: &mut FontSystem,
) -> MutablePageBox<'a> {
    build_page_box_inner(root, stylesheets, parent_style, Some(font_system), None)
}

fn build_page_box_inner<'a>(
    root: &'a Node,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
    font_system: Option<&mut FontSystem>,
    principal_flow: Option<DocumentPrincipalFlow>,
) -> MutablePageBox<'a> {
    let built = match &root.kind {
        NodeKind::Element(element) => build_child_boxes_inner(
            element,
            stylesheets,
            parent_style,
            &[],
            true,
            false,
            font_system,
            principal_flow,
        ),
        NodeKind::Text(text) => {
            if text.is_empty() {
                BuiltChildren::default()
            } else {
                BuiltChildren {
                    boxes: vec![MutableFormattingBox::Text(MutableTextBox {
                        text: text.clone(),
                        style: inherited_text_style(parent_style),
                    })],
                    counter_events: Vec::new(),
                }
            }
        }
    };
    MutablePageBox {
        children: built.boxes,
        counter_events: built.counter_events,
    }
}

/// Text nodes inherit their parent's used font size. The parent may itself
/// carry a relative deferred expression, so duplicating it would apply the
/// expression a second time during pre-freeze resolution.
fn inherited_text_style(parent_style: &ComputedStyle) -> Box<ComputedStyle> {
    let mut style = parent_style.clone();
    style.deferred_font_size = css::DeferredFontSize::Inherit;
    Box::new(style)
}

/// Return the inherited text style for a text node flattened out of a
/// `display: contents` element.
///
/// The flattened text box is physically reparented to the contents element's
/// box parent, but its inherited values still come from the suppressed
/// element. Store that already-computed font size as absolute so the later
/// font-metric pass does not inherit again from the physical parent:
/// <https://www.w3.org/TR/css-display-3/#valdef-display-contents>.
fn flattened_contents_text_style(parent_style: &ComputedStyle) -> Box<ComputedStyle> {
    let mut style = parent_style.clone();
    style.deferred_font_size = css::DeferredFontSize::Absolute(style.font_size);
    Box::new(style)
}

#[derive(Default)]
struct BuiltChildren<'a> {
    boxes: Vec<MutableFormattingBox<'a>>,
    counter_events: Vec<CounterEventNode<'a>>,
}

struct BuiltElement<'a> {
    box_: MutableFormattingBox<'a>,
    counter_event: CounterEventNode<'a>,
}

pub(crate) fn build_child_boxes_with_font_metrics<'a>(
    element: &'a Element,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
    ancestors: &[ElementSignature],
    font_system: &mut FontSystem,
) -> Vec<MutableFormattingBox<'a>> {
    build_child_boxes_inner(
        element,
        stylesheets,
        parent_style,
        ancestors,
        true,
        false,
        Some(font_system),
        None,
    )
    .boxes
}

#[allow(clippy::too_many_arguments)]
fn build_child_boxes_inner<'a>(
    element: &'a Element,
    stylesheets: &[Stylesheet],
    parent_style: &ComputedStyle,
    ancestors: &[ElementSignature],
    normalize_for_parent: bool,
    text_parent_is_flattened_contents: bool,
    mut font_system: Option<&mut FontSystem>,
    principal_flow: Option<DocumentPrincipalFlow>,
) -> BuiltChildren<'a> {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut raw = Vec::new();
    let mut counter_events = Vec::new();
    push_generated_pseudo_box(
        &mut raw,
        &mut counter_events,
        element,
        parent_style,
        parent_style.before_style.as_deref(),
        GeneratedPseudoKind::Before,
    );
    for child in &element.children {
        match &child.kind {
            NodeKind::Text(text) => {
                if !text.is_empty() && !element_suppresses_direct_text_children(element) {
                    raw.push(MutableFormattingBox::Text(MutableTextBox {
                        text: text.clone(),
                        style: if text_parent_is_flattened_contents {
                            flattened_contents_text_style(parent_style)
                        } else {
                            inherited_text_style(parent_style)
                        },
                    }));
                }
            }
            NodeKind::Element(child_element) => {
                let signature = ElementSignature::with_sibling_list(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                if is_html_select_item_element(child_element)
                    && !has_html_select_context(element, ancestors)
                {
                    continue;
                }
                let style = match font_system.as_deref_mut() {
                    Some(font_system) => {
                        let parent_ch_advance = font_system.ch_advance(parent_style);
                        let mut style = style_for_layout_element_with_parent_ch_advance(
                            child_element,
                            signature.clone(),
                            stylesheets,
                            Some(parent_style),
                            ancestors,
                            parent_ch_advance,
                        );
                        let pseudo_parent_ch_advance = font_system.ch_advance(&style);
                        let pseudo_signature = layout_element_signature(
                            child_element,
                            signature.clone(),
                            Some(parent_style),
                        );
                        css::apply_pseudo_rules_with_parent_ch_advance(
                            &mut style,
                            &pseudo_signature,
                            stylesheets,
                            ancestors,
                            pseudo_parent_ch_advance,
                        );
                        style
                    }
                    None => style_for_layout_element(
                        child_element,
                        signature.clone(),
                        stylesheets,
                        Some(parent_style),
                        ancestors,
                    ),
                };
                let style = if ancestors.is_empty() {
                    root_display_fixed_style(style)
                } else {
                    style
                };
                let style = if ancestors.is_empty()
                    && child_element.document_syntax == dom::DocumentSyntax::Html
                    && child_element.tag.eq_ignore_ascii_case("html")
                    && let Some(principal_flow) = principal_flow
                {
                    principal_flow_root_style(style, principal_flow)
                } else {
                    style
                };
                if style.display.is_contents() {
                    // CSS Display 3 `display: contents` suppresses the
                    // element's principal box but keeps its children in the box
                    // tree, inheriting from the contents element and matching
                    // selectors with that element in their ancestor chain.
                    // https://www.w3.org/TR/css-display-3/#valdef-display-contents
                    let mut child_ancestors = ancestors.to_vec();
                    child_ancestors.push(signature);
                    let built = build_child_boxes_inner(
                        child_element,
                        stylesheets,
                        &style,
                        &child_ancestors,
                        false,
                        true,
                        font_system.as_deref_mut(),
                        None,
                    );
                    raw.extend(built.boxes);
                    counter_events.extend(built.counter_events);
                } else if let Some(built) = build_element_box(
                    child_element,
                    signature,
                    style,
                    stylesheets,
                    ancestors,
                    font_system.as_deref_mut(),
                ) {
                    raw.push(built.box_);
                    counter_events.push(built.counter_event);
                }
            }
        }
    }
    push_generated_pseudo_box(
        &mut raw,
        &mut counter_events,
        element,
        parent_style,
        parent_style.after_style.as_deref(),
        GeneratedPseudoKind::After,
    );
    let boxes = if normalize_for_parent {
        normalize_block_container_children(raw, parent_style)
    } else {
        raw
    };
    BuiltChildren {
        boxes,
        counter_events,
    }
}

/// Applies the principal-flow used values to the HTML root's formatting style.
///
/// This intentionally happens after cascading: the root retains its computed
/// declarations for selector matching, while its principal box, generated
/// pseudo-elements, and inherited descendants use the CSS Writing Modes
/// body-propagated axes.
/// <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
fn principal_flow_root_style(
    mut style: ComputedStyle,
    principal_flow: DocumentPrincipalFlow,
) -> ComputedStyle {
    style.writing_mode = principal_flow.writing_mode;
    style.direction = principal_flow.direction;
    style
}

fn build_element_box<'a>(
    element: &'a Element,
    signature: ElementSignature,
    style: ComputedStyle,
    stylesheets: &[Stylesheet],
    ancestors: &[ElementSignature],
    font_system: Option<&mut FontSystem>,
) -> Option<BuiltElement<'a>> {
    let mut style = Box::new(style);
    if style.display.is_none() {
        return None;
    }
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        style.abspos_static_source_was_inline_level = style.display.is_inline_level();
        style.abspos_static_source_was_atomic_inline = style.display.is_atomic_inline();
        style.display = style.display.blockified();
    }

    let content_replacement = matches!(style.content, Content::Replacement { .. });
    let mut child_ancestors = ancestors.to_vec();
    child_ancestors.push(signature.clone());
    let built_children = if content_replacement || is_horizontal_rule_element(element) {
        BuiltChildren::default()
    } else {
        build_child_boxes_inner(
            element,
            stylesheets,
            &style,
            &child_ancestors,
            true,
            false,
            font_system,
            None,
        )
    };
    let children = built_children.boxes;
    let mut counter_children = built_children.counter_events;
    let marker = marker_box(&style);
    if let Some(marker) = &marker {
        counter_children.insert(
            0,
            CounterEventNode {
                element,
                source: CounterEventSource::Marker,
                style: marker.style.as_ref().clone(),
                children: Vec::new(),
            },
        );
    }
    let source = BoxSource::Principal;
    let counter_style = style.as_ref().clone();

    if content_replacement || is_replaced_element(element) {
        style.display = if style.display.is_block_level() {
            Display::BLOCK_REPLACED.with_list_item(style.display.is_list_item())
        } else if style.display.is_run_in() {
            style.display.with_inner(DisplayInner::Replaced)
        } else {
            Display::INLINE_REPLACED.with_list_item(style.display.is_list_item())
        };
        let box_ = if style.display.is_inline_or_run_in_level() {
            Some(MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
                element,
                signature,
                source,
                marker,
                style,
                children,
                table_fragment: None,
            }))
        } else {
            Some(MutableFormattingBox::Replaced(MutableReplacedBox {
                element,
                signature,
                source,
                marker,
                style,
                children,
            }))
        }?;
        return Some(BuiltElement {
            box_,
            counter_event: CounterEventNode {
                element,
                source: CounterEventSource::Principal,
                style: counter_style,
                children: counter_children,
            },
        });
    }

    let box_ = if style.display.is_table() && style.display.is_inline_or_run_in_level() {
        let fragment = build_table_fragment(element, &signature, &children);
        MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
            element,
            signature,
            source,
            marker,
            style,
            children,
            table_fragment: Some(fragment),
        })
    } else if style.display.is_table()
        || (style.display.is_block_level() && is_html_table_element(element))
    {
        let fragment = build_table_fragment(element, &signature, &children);
        MutableFormattingBox::Table(MutableTableBox {
            element,
            signature,
            source,
            marker,
            style,
            children,
            fragment,
        })
    } else if style.display.is_flex() && style.display.is_block_level() {
        MutableFormattingBox::Flex(MutableFlexBox {
            element,
            signature,
            source,
            marker,
            style,
            children,
        })
    } else if style.display.is_atomic_inline()
        || (style.display.is_run_in() && !style.display.is_flow())
    {
        MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
            element,
            signature,
            source,
            marker,
            style,
            children,
            table_fragment: None,
        })
    } else if style.display.is_block_level() {
        MutableFormattingBox::Block(MutableBlockBox {
            element,
            signature,
            source,
            marker,
            style,
            run_in_children: Vec::new(),
            children,
        })
    } else {
        MutableFormattingBox::Inline(MutableInlineBox {
            element,
            signature,
            source,
            marker,
            style,
            fragment_edges: InlineBoxFragmentEdges::ALL,
            children,
        })
    };
    Some(BuiltElement {
        box_,
        counter_event: CounterEventNode {
            element,
            source: CounterEventSource::Principal,
            style: counter_style,
            children: counter_children,
        },
    })
}

fn push_generated_pseudo_box<'a>(
    output: &mut Vec<MutableFormattingBox<'a>>,
    counter_events: &mut Vec<CounterEventNode<'a>>,
    originating_element: &'a Element,
    originating_style: &ComputedStyle,
    pseudo_style: Option<&ComputedStyle>,
    kind: GeneratedPseudoKind,
) {
    let Some(pseudo_style) = pseudo_style else {
        return;
    };
    if pseudo_style.display.is_none() || !pseudo_style.content.is_generated() {
        return;
    }
    let originating_signature = ElementSignature::new(
        originating_element.tag.clone(),
        originating_element.attrs.clone(),
    );
    let mut style = Box::new(pseudo_style.clone());
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        style.abspos_static_source_was_inline_level = style.display.is_inline_level();
        style.abspos_static_source_was_atomic_inline = style.display.is_atomic_inline();
        style.display = style.display.blockified();
    }
    if let Some(box_) = build_generated_pseudo_box(
        originating_element,
        originating_signature,
        originating_style.clear,
        style,
        kind,
    ) {
        output.push(box_);
        counter_events.push(CounterEventNode {
            element: originating_element,
            source: match kind {
                GeneratedPseudoKind::Before => CounterEventSource::Before,
                GeneratedPseudoKind::After => CounterEventSource::After,
            },
            style: pseudo_style.clone(),
            children: Vec::new(),
        });
    }
}

fn build_generated_pseudo_box<'a>(
    originating_element: &'a Element,
    originating_signature: ElementSignature,
    originating_clear: Clear,
    style: Box<ComputedStyle>,
    kind: GeneratedPseudoKind,
) -> Option<MutableFormattingBox<'a>> {
    if style.display.is_none() {
        return None;
    }
    let source = BoxSource::GeneratedPseudo(Box::new(GeneratedPseudoBox {
        originating_element,
        originating_signature: originating_signature.clone(),
        originating_clear,
        kind,
    }));
    let marker = marker_box(&style);
    let children = Vec::new();

    if style.display.is_table() && style.display.is_inline_or_run_in_level() {
        let fragment = build_table_fragment(originating_element, &originating_signature, &children);
        Some(MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
            element: originating_element,
            signature: originating_signature,
            source,
            marker,
            style,
            children,
            table_fragment: Some(fragment),
        }))
    } else if style.display.is_table()
        || (style.display.is_block_level() && is_html_table_element(originating_element))
    {
        let fragment = build_table_fragment(originating_element, &originating_signature, &children);
        Some(MutableFormattingBox::Table(MutableTableBox {
            element: originating_element,
            signature: originating_signature,
            source,
            marker,
            style,
            children,
            fragment,
        }))
    } else if style.display.is_flex() && style.display.is_block_level() {
        Some(MutableFormattingBox::Flex(MutableFlexBox {
            element: originating_element,
            signature: originating_signature,
            source,
            marker,
            style,
            children,
        }))
    } else if style.display.is_atomic_inline()
        || (style.display.is_run_in() && !style.display.is_flow())
    {
        Some(MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
            element: originating_element,
            signature: originating_signature,
            source,
            marker,
            style,
            children,
            table_fragment: None,
        }))
    } else if style.display.is_block_level() {
        Some(MutableFormattingBox::Block(MutableBlockBox {
            element: originating_element,
            signature: originating_signature,
            source,
            marker,
            style,
            run_in_children: Vec::new(),
            children,
        }))
    } else {
        Some(MutableFormattingBox::Inline(MutableInlineBox {
            element: originating_element,
            signature: originating_signature,
            source,
            marker,
            style,
            fragment_edges: InlineBoxFragmentEdges::ALL,
            children,
        }))
    }
}

/// Applies CSS Display root-element display fixups during box-tree construction.
///
/// CSS Display 4 blockifies the root element's principal box, and
/// `display: contents` computes to `block` on the root:
/// <https://www.w3.org/TR/css-display-4/#root>.
fn root_display_fixed_style(mut style: ComputedStyle) -> ComputedStyle {
    style.display = if style.display.is_contents() {
        Display::BLOCK
    } else {
        style.display.blockified()
    };
    style
}

/// Build a durable CSS table fragment from normalized child boxes.
///
/// CSS table fixup generates missing child wrappers before missing parents,
/// and table layout then requires a stable table wrapper, row-group, row, cell,
/// column, caption, and occupancy model before layout:
/// <https://drafts.csswg.org/css-tables/#fixup-algorithm> and
/// <https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes>.
pub(crate) fn build_table_fragment<'a>(
    element: &'a Element,
    signature: &ElementSignature,
    children: &[MutableFormattingBox<'a>],
) -> MutableTableFragment<'a> {
    let captions = table_fragment_captions(children);
    let columns = table_fragment_columns(children);
    let mut rows = Vec::new();
    collect_table_fragment_rows(children, &mut rows, std::slice::from_ref(signature), &[]);
    if rows.is_empty() && is_html_table_row_element(element) {
        rows.push(MutableTableFragmentRow {
            element: Some(element),
            signature: signature.clone(),
            ancestors: Vec::new(),
            row_groups: Vec::new(),
            style: None,
            cells: Vec::new(),
        });
    }
    let grid = table_fragment_grid(&rows);
    MutableTableFragment {
        rows,
        captions,
        columns,
        grid,
    }
}

pub(crate) fn build_frozen_table_fragment<'a>(
    element: &'a Element,
    signature: &ElementSignature,
    children: &[FrozenFormattingBox<'a>],
) -> FrozenTableFragment<'a> {
    let mutable_children = clone_frozen_child_boxes_as_mutable(children);
    freeze_table_fragment(build_table_fragment(element, signature, &mutable_children))
}

fn table_fragment_captions<'a>(
    children: &[MutableFormattingBox<'a>],
) -> Vec<MutableTableFragmentCaption<'a>> {
    let mut captions = Vec::new();
    collect_table_fragment_captions(children, &mut captions);
    captions
}

fn collect_table_fragment_captions<'a>(
    children: &[MutableFormattingBox<'a>],
    captions: &mut Vec<MutableTableFragmentCaption<'a>>,
) {
    for child in children {
        if let Some((element, signature, style, descendants)) = child.element_parts()
            && is_table_caption_box(element, style)
        {
            captions.push(MutableTableFragmentCaption {
                element,
                signature: signature.clone(),
                style: Some(Box::new(style.clone())),
                children: descendants.to_vec(),
            });
            continue;
        }
        collect_table_fragment_captions(child.children(), captions);
    }
}

fn table_fragment_columns<'a>(
    children: &[MutableFormattingBox<'a>],
) -> Vec<MutableTableFragmentColumn<'a>> {
    let mut columns = Vec::new();
    collect_table_fragment_columns(children, &mut columns);
    columns
}

fn collect_table_fragment_columns<'a>(
    children: &[MutableFormattingBox<'a>],
    columns: &mut Vec<MutableTableFragmentColumn<'a>>,
) {
    for child in children {
        let Some((element, signature, style, descendants)) = child.element_parts() else {
            continue;
        };
        if is_table_column_box(element, style) {
            columns.push(MutableTableFragmentColumn {
                element,
                signature: signature.clone(),
                style: Some(Box::new(style.clone())),
                group: None,
                span: html_table_column_span(element),
            });
            continue;
        }
        if is_table_column_group_box(element, style) {
            collect_table_fragment_column_group(element, signature, style, descendants, columns);
            continue;
        }
        collect_table_fragment_columns(descendants, columns);
    }
}

fn collect_table_fragment_column_group<'a>(
    group_element: &'a Element,
    group_signature: &ElementSignature,
    group_style: &ComputedStyle,
    children: &[MutableFormattingBox<'a>],
    columns: &mut Vec<MutableTableFragmentColumn<'a>>,
) {
    let group = MutableTableFragmentColumnGroup {
        element: group_element,
        signature: group_signature.clone(),
        style: Some(Box::new(group_style.clone())),
        span: html_table_column_span(group_element),
    };
    let mut saw_column = false;
    for child in children {
        let Some((element, signature, style, _)) = child.element_parts() else {
            continue;
        };
        if is_table_column_box(element, style) {
            saw_column = true;
            columns.push(MutableTableFragmentColumn {
                element,
                signature: signature.clone(),
                style: Some(Box::new(style.clone())),
                group: Some(group.clone()),
                span: html_table_column_span(element),
            });
        }
    }
    if !saw_column {
        columns.push(MutableTableFragmentColumn {
            element: group_element,
            signature: group_signature.clone(),
            style: Some(Box::new(group_style.clone())),
            group: Some(group.clone()),
            span: group.span,
        });
    }
}

fn collect_table_fragment_rows<'a>(
    children: &[MutableFormattingBox<'a>],
    rows: &mut Vec<MutableTableFragmentRow<'a>>,
    ancestors: &[ElementSignature],
    row_groups: &[MutableTableFragmentRowGroup<'a>],
) {
    let mut anonymous_cells = Vec::new();
    let mut anonymous_cell_children = Vec::new();
    for (index, child) in children.iter().enumerate() {
        let Some((element, signature, style, descendants)) = child.element_parts() else {
            if matches!(child, MutableFormattingBox::Text(_))
                && !table_fragment_whitespace_is_ignorable(children, index)
            {
                anonymous_cell_children.push(child.clone());
            }
            continue;
        };
        if is_table_row_box(element, style) {
            flush_anonymous_table_fragment_cell(&mut anonymous_cells, &mut anonymous_cell_children);
            flush_anonymous_table_fragment_row(rows, &mut anonymous_cells, ancestors, row_groups);
            let cells = table_fragment_row_child_cells(descendants);
            rows.push(MutableTableFragmentRow {
                element: Some(element),
                signature: signature.clone(),
                ancestors: ancestors.to_vec(),
                row_groups: row_groups.to_vec(),
                style: Some(Box::new(style.clone())),
                cells,
            });
            continue;
        }
        if is_table_cell_box(element, style) {
            flush_anonymous_table_fragment_cell(&mut anonymous_cells, &mut anonymous_cell_children);
            anonymous_cells.push(MutableTableFragmentCell {
                element: Some(element),
                signature: signature.clone(),
                style: Some(Box::new(style.clone())),
                children: descendants.to_vec(),
                anonymous: false,
            });
            continue;
        }
        if is_table_column_box(element, style)
            || is_table_column_group_box(element, style)
            || is_table_caption_box(element, style)
        {
            continue;
        }
        if is_table_row_group_box(element, style) && row_groups.is_empty() {
            flush_anonymous_table_fragment_cell(&mut anonymous_cells, &mut anonymous_cell_children);
            flush_anonymous_table_fragment_row(rows, &mut anonymous_cells, ancestors, row_groups);
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature.clone());
            let mut child_row_groups = row_groups.to_vec();
            child_row_groups.push(MutableTableFragmentRowGroup {
                element,
                signature: signature.clone(),
                style: Some(Box::new(style.clone())),
            });
            collect_table_fragment_rows(descendants, rows, &child_ancestors, &child_row_groups);
            continue;
        }
        anonymous_cell_children.push(child.clone());
    }
    flush_anonymous_table_fragment_cell(&mut anonymous_cells, &mut anonymous_cell_children);
    flush_anonymous_table_fragment_row(rows, &mut anonymous_cells, ancestors, row_groups);
}

fn table_fragment_row_child_cells<'a>(
    children: &[MutableFormattingBox<'a>],
) -> Vec<MutableTableFragmentCell<'a>> {
    let mut cells = Vec::new();
    let mut anonymous_cell_children = Vec::new();
    for (index, child) in children.iter().enumerate() {
        let Some((element, signature, style, descendants)) = child.element_parts() else {
            if matches!(child, MutableFormattingBox::Text(_))
                && !table_fragment_whitespace_is_ignorable(children, index)
            {
                anonymous_cell_children.push(child.clone());
            }
            continue;
        };
        if is_table_cell_box(element, style) {
            flush_anonymous_table_fragment_cell(&mut cells, &mut anonymous_cell_children);
            cells.push(MutableTableFragmentCell {
                element: Some(element),
                signature: signature.clone(),
                style: Some(Box::new(style.clone())),
                children: descendants.to_vec(),
                anonymous: false,
            });
            continue;
        }
        // CSS Tables fixup generates missing child wrappers before missing
        // parents. A table-internal child that is not a real cell therefore
        // belongs to the current anonymous-cell run; the cell flush then wraps
        // any misparented table-internal boxes in their missing table objects.
        // <https://drafts.csswg.org/css-tables-3/#fixup>.
        anonymous_cell_children.push(child.clone());
    }
    flush_anonymous_table_fragment_cell(&mut cells, &mut anonymous_cell_children);
    cells
}

/// Return whether whitespace is ignored while fixing up table-internal boxes.
///
/// CSS Tables ignores whitespace-only anonymous inline boxes that touch
/// table-internal boxes, even when CSS Text would preserve those glyphs, but
/// the consecutive-box rules keep whitespace between non-internal siblings so
/// it can participate in the generated anonymous cell:
/// <https://drafts.csswg.org/css-tables/#consecutive-boxes>.
fn table_fragment_whitespace_is_ignorable(
    children: &[MutableFormattingBox<'_>],
    index: usize,
) -> bool {
    if !children
        .get(index)
        .is_some_and(formatting_box_is_document_whitespace)
    {
        return false;
    }

    // A CSS Tables whitespace box is a sequence of anonymous inline boxes.
    // HTML comments can split source indentation into several text nodes, so
    // inspect the sequence's non-whitespace neighbors rather than only the
    // immediately adjacent node.
    let previous = children[..index]
        .iter()
        .rev()
        .find(|child| !formatting_box_is_document_whitespace(child));
    let next = children[index + 1..]
        .iter()
        .find(|child| !formatting_box_is_document_whitespace(child));
    previous.is_none_or(table_fragment_box_is_internal_or_caption)
        || next.is_none_or(table_fragment_box_is_internal_or_caption)
}

fn table_fragment_box_is_internal_or_caption(box_: &MutableFormattingBox<'_>) -> bool {
    let Some((element, _, style, _)) = box_.element_parts() else {
        return false;
    };
    is_table_caption_box(element, style)
        || is_table_column_group_box(element, style)
        || is_table_column_box(element, style)
        || is_table_row_group_box(element, style)
        || is_table_row_box(element, style)
        || is_table_cell_box(element, style)
}

/// Flush consecutive improper table children into one anonymous table cell.
///
/// CSS Tables treats consecutive non-table-cell boxes as one run when
/// generating missing cells, and only ignores whitespace for table-internal
/// adjacency. Whitespace between improper children therefore remains inline
/// content inside the generated cell:
/// <https://drafts.csswg.org/css-tables/#consecutive-boxes> and
/// <https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes>.
fn flush_anonymous_table_fragment_cell<'a>(
    cells: &mut Vec<MutableTableFragmentCell<'a>>,
    children: &mut Vec<MutableFormattingBox<'a>>,
) {
    if children.is_empty() {
        return;
    }
    let (style, children) = anonymous_table_fragment_cell_style_and_children(children);
    cells.push(MutableTableFragmentCell {
        element: None,
        signature: ElementSignature::new("td", HashMap::new()),
        style: Some(Box::new(style)),
        children,
        anonymous: true,
    });
}

fn anonymous_table_fragment_cell_style_and_children<'a>(
    children: &mut Vec<MutableFormattingBox<'a>>,
) -> (ComputedStyle, Vec<MutableFormattingBox<'a>>) {
    let parent_style = anonymous_table_fragment_cell_parent_style(children);
    let normalized = normalize_orphan_table_internal_boxes(std::mem::take(children), &parent_style);
    (parent_style, normalized)
}

/// Build the effective parent style used while normalizing anonymous cell content.
///
/// CSS Tables generates missing child wrappers before missing parents. When a
/// table-internal box is wrapped in an anonymous table-cell, the later missing
/// parent stage must see a table-cell parent, not the original row or row-group:
/// <https://drafts.csswg.org/css-tables/#fixup-algorithm>.
fn anonymous_table_fragment_cell_parent_style(
    children: &[MutableFormattingBox<'_>],
) -> ComputedStyle {
    let mut style = children
        .first()
        .map(table_fragment_child_style)
        .unwrap_or_else(|| css::default_style_for_tag("td"));
    style.display = Display::TABLE_CELL;
    style
}

fn table_fragment_child_style(child: &MutableFormattingBox<'_>) -> ComputedStyle {
    match child {
        MutableFormattingBox::Block(box_) => box_.style.as_ref().clone(),
        MutableFormattingBox::Inline(box_) => box_.style.as_ref().clone(),
        MutableFormattingBox::InlineSplitBlockContext(box_) => box_.style.as_ref().clone(),
        MutableFormattingBox::AnonymousBlock(box_) => box_.style.as_ref().clone(),
        MutableFormattingBox::AtomicInline(box_) => box_.style.as_ref().clone(),
        MutableFormattingBox::Text(box_) => box_.style.as_ref().clone(),
        MutableFormattingBox::Table(box_) => box_.style.as_ref().clone(),
        MutableFormattingBox::Flex(box_) => box_.style.as_ref().clone(),
        MutableFormattingBox::Replaced(box_) => box_.style.as_ref().clone(),
    }
}

fn flush_anonymous_table_fragment_row<'a>(
    rows: &mut Vec<MutableTableFragmentRow<'a>>,
    cells: &mut Vec<MutableTableFragmentCell<'a>>,
    ancestors: &[ElementSignature],
    row_groups: &[MutableTableFragmentRowGroup<'a>],
) {
    if cells.is_empty() {
        return;
    }
    let mut style = css::anonymous_block_style(
        cells[0]
            .style
            .as_deref()
            .expect("anonymous table cells carry their inherited style"),
    );
    style.display = Display::TABLE_ROW;
    rows.push(MutableTableFragmentRow {
        element: cells[0].element,
        signature: ElementSignature::new("tr", HashMap::new()),
        ancestors: ancestors.to_vec(),
        row_groups: row_groups.to_vec(),
        style: Some(Box::new(style)),
        cells: std::mem::take(cells),
    });
}

fn table_fragment_grid(rows: &[MutableTableFragmentRow<'_>]) -> TableFragmentGrid {
    let mut grid_rows = Vec::with_capacity(rows.len());
    let mut active_rowspans: Vec<usize> = Vec::new();
    let mut column_count = 0usize;
    let row_group_ends = table_fragment_row_group_end_indices(rows);

    for (row_index, row) in rows.iter().enumerate() {
        let mut placements = Vec::new();
        let mut column = 0usize;
        for (cell_index, cell) in row.cells.iter().enumerate() {
            while active_rowspans.get(column).cloned().unwrap_or(0) > 0 {
                column += 1;
            }

            let colspan = cell.element.map(html_table_colspan).unwrap_or(1);
            let rowspan = cell
                .element
                .map(|element| html_table_rowspan(element, row_index, row_group_ends[row_index]))
                .unwrap_or(1);
            let end = column + colspan;
            if active_rowspans.len() < end {
                active_rowspans.resize(end, 0);
            }
            for active in &mut active_rowspans[column..end] {
                *active = (*active).max(rowspan);
            }
            placements.push(TableFragmentCellPlacement {
                cell: cell_index,
                column,
                colspan,
                rowspan,
            });
            column = end;
        }
        column_count = column_count.max(active_rowspans.len());
        for active in &mut active_rowspans {
            *active = active.saturating_sub(1);
        }
        while active_rowspans.last().cloned() == Some(0) {
            active_rowspans.pop();
        }
        grid_rows.push(placements);
    }

    TableFragmentGrid {
        rows: grid_rows,
        column_count: column_count.max(1),
    }
}

fn table_fragment_row_group_end_indices(rows: &[MutableTableFragmentRow<'_>]) -> Vec<usize> {
    let mut ends = vec![rows.len(); rows.len()];
    let mut start = 0usize;
    let mut current_group = rows.first().and_then(|row| row.row_groups.last().cloned());
    for (index, row) in rows.iter().enumerate() {
        let group = row.row_groups.last().cloned();
        if index > 0
            && table_fragment_group_signature(&group)
                != table_fragment_group_signature(&current_group)
        {
            for end in &mut ends[start..index] {
                *end = index;
            }
            start = index;
            current_group = group;
        }
    }
    ends
}

fn table_fragment_group_signature<'a>(
    group: &'a Option<MutableTableFragmentRowGroup<'_>>,
) -> Option<&'a ElementSignature> {
    group.as_ref().map(|group| &group.signature)
}

fn is_table_caption_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_caption_element(element) || style.display.is_table_caption()
}

fn is_table_column_group_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_column_group_element(element) || style.display.is_table_column_group()
}

fn is_table_column_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_column_element(element) || style.display.is_table_column()
}

fn is_table_row_group_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_row_group_element(element) || style.display.is_table_row_group()
}

fn is_table_row_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_row_element(element) || style.display.is_table_row()
}

fn is_table_cell_box(element: &Element, style: &ComputedStyle) -> bool {
    is_html_table_cell_element(element) || style.display.is_table_cell()
}

pub(crate) fn marker_box(style: &ComputedStyle) -> Option<MutableMarkerBox> {
    // CSS Lists 3: a list item generates a marker box associated with the
    // principal box. Full `::marker` styling will replace this style clone.
    // https://www.w3.org/TR/css-lists-3/#markers
    style.display.is_list_item().then(|| MutableMarkerBox {
        style: Box::new(
            style
                .marker_style
                .as_deref()
                .cloned()
                .unwrap_or_else(|| style.clone()),
        ),
    })
}
