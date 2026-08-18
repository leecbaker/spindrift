use super::*;

pub(crate) fn build_page_box<'a>(
    root: &'a Node,
    stylesheets: &Stylesheets<'_>,
    parent_style: &ComputedStyle,
) -> MutablePageBox<'a> {
    build_page_box_inner(root, stylesheets, parent_style, None)
}

#[cfg(test)]
pub(crate) fn build_page_box_with_font_metrics<'a>(
    root: &'a Node,
    stylesheets: &Stylesheets<'_>,
    parent_style: &ComputedStyle,
    font_system: &mut FontSystem,
) -> MutablePageBox<'a> {
    build_page_box_inner(root, stylesheets, parent_style, Some(font_system))
}

fn build_page_box_inner<'a>(
    root: &'a Node,
    stylesheets: &Stylesheets<'_>,
    parent_style: &ComputedStyle,
    font_system: Option<&mut FontSystem>,
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
                    footnotes: Vec::new(),
                    counter_events: Vec::new(),
                    suppressed_named_string_events: Vec::new(),
                }
            }
        }
    };
    MutablePageBox {
        children: built.boxes,
        footnotes: built.footnotes,
        counter_events: built.counter_events,
        suppressed_named_string_events: built.suppressed_named_string_events,
    }
}

/// Text nodes inherit their parent's used font size. The parent may itself
/// carry a relative deferred expression, so duplicating it would apply the
/// expression a second time during pre-freeze resolution.
fn inherited_text_style(parent_style: &ComputedStyle) -> Box<ComputedStyle> {
    Box::new(css::anonymous_text_style(parent_style))
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
    let mut style = css::anonymous_text_style(parent_style);
    // A `display: contents` element has no principal box and therefore cannot
    // originate typographic pseudo-elements. Its flattened text still
    // inherits ordinary values from the element, while a box-generating
    // ancestor can apply its own `::first-line`/`::first-letter` styling at
    // layout time.
    // <https://drafts.csswg.org/css-display-3/#valdef-display-contents>
    // <https://drafts.csswg.org/css-pseudo-4/#first-line-pseudo>
    style.first_line_style = None;
    style.first_letter_style = None;
    Box::new(style)
}

/// Freeze inherited font metrics before flattening a `display: contents`
/// subtree into its box parent.
///
/// The flattened descendants still inherit through the contents element, but
/// later font-metric resolution runs against their physical box parent.  An
/// unresolved relative inherited font size would therefore be resolved a
/// second time against that physical parent instead of the suppressed element.
/// <https://drafts.csswg.org/css-display-3/#valdef-display-contents>
fn flattened_contents_inheritance_style(parent_style: &ComputedStyle) -> ComputedStyle {
    // Element descendants still cascade against the suppressed element.
    // Copy its complete computed style here: a declaration such as
    // `background: inherit` must see that otherwise non-inherited property.
    // Text nodes use `flattened_contents_text_style` above, which deliberately
    // strips box paint because the contents element itself has no box.
    let mut style = parent_style.clone();
    // Cascading has already resolved this element's parent-relative
    // `font-size` into `font_size`; only the deferred representation remains
    // for the later physical-tree font-metric pass. Do not resolve it again
    // here: a `3em` contents element would otherwise become 9em when this
    // flattened inheritance style is materialized.
    style.deferred_font_size = css::DeferredFontSize::Absolute(style.font_size);
    style
}

#[derive(Default)]
struct BuiltChildren<'a> {
    boxes: Vec<MutableFormattingBox<'a>>,
    footnotes: Vec<MutableFootnoteBox<'a>>,
    counter_events: Vec<CounterEventNode<'a>>,
    suppressed_named_string_events: Vec<SuppressedNamedStringEvent>,
}

struct BuiltElement<'a> {
    box_: MutableFormattingBox<'a>,
    footnotes: Vec<MutableFootnoteBox<'a>>,
    counter_event: CounterEventNode<'a>,
    suppressed_named_string_events: Vec<SuppressedNamedStringEvent>,
}

pub(crate) fn build_child_boxes_with_font_metrics<'a>(
    element: &'a Element,
    stylesheets: &Stylesheets<'_>,
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
    )
    .boxes
}

#[allow(clippy::too_many_arguments)]
fn build_child_boxes_inner<'a>(
    element: &'a Element,
    stylesheets: &Stylesheets<'_>,
    parent_style: &ComputedStyle,
    ancestors: &[ElementSignature],
    normalize_for_parent: bool,
    text_parent_is_flattened_contents: bool,
    mut font_system: Option<&mut FontSystem>,
) -> BuiltChildren<'a> {
    let sibling_tags = element_sibling_signature_list(element);
    let mut element_index = 0usize;
    let mut raw = Vec::new();
    let mut footnotes = Vec::new();
    let mut counter_events = Vec::new();
    let mut suppressed_named_string_events = Vec::new();
    let mut pending_suppressed_named_string_events = Vec::new();
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
                let signature =
                    ElementSignature::from_sibling_snapshot(element_index, sibling_tags.clone())
                        .expect("source child must have a cached sibling signature");
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
                let mut style = if ancestors.is_empty() {
                    root_display_fixed_style(style)
                } else {
                    style
                };
                // This child is physically reparented past a `display:
                // contents` ancestor. An inherited font size must retain the
                // already-computed value from that suppressed ancestor rather
                // than resolving once more against its physical box parent.
                // Explicit relative font sizes remain deferred: their parent
                // is the flattened ancestor and still supplies the correct
                // percentage/em basis.
                // <https://drafts.csswg.org/css-display-3/#valdef-display-contents>
                if text_parent_is_flattened_contents
                    && matches!(style.deferred_font_size, css::DeferredFontSize::Inherit)
                {
                    style.deferred_font_size = css::DeferredFontSize::Absolute(style.font_size);
                }
                if text_parent_is_flattened_contents {
                    // Generated boxes are physically emitted beside the
                    // suppressed `display: contents` element as well. Their
                    // cascaded `font_size` already includes the flattened
                    // ancestor; freeze the deferred form so the metric pass
                    // cannot apply that ancestor's relative size a second
                    // time against the physical parent.
                    for pseudo in [
                        style.marker_style.as_deref_mut(),
                        style.before_style.as_deref_mut(),
                        style.after_style.as_deref_mut(),
                        style.first_line_style.as_deref_mut(),
                        style.first_letter_style.as_deref_mut(),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        pseudo.deferred_font_size =
                            css::DeferredFontSize::Absolute(pseudo.font_size);
                    }
                }
                if display_contents_computes_to_none_for_css_layout_svg_root(
                    child_element,
                    element,
                    &style,
                ) {
                    style.display = Display::NONE;
                }
                // A flex or grid container blockifies each tree-abiding child
                // before CSS Tables can run its anonymous-wrapper fixup. In
                // particular, direct table-internal children become independent
                // block-level flex/grid items rather than being collected into
                // a synthetic table fragment.
                // <https://drafts.csswg.org/css-display-4/#transformations>
                // <https://drafts.csswg.org/css-flexbox-1/#flex-items>
                // <https://drafts.csswg.org/css-tables-3/#fixup-algorithm>
                if parent_style.display.is_flex() || parent_style.display.is_grid() {
                    style.display = style.display.blockified();
                }
                if style.display.is_contents() {
                    // CSS Display 3 `display: contents` suppresses the
                    // element's principal box but keeps its children in the box
                    // tree, inheriting from the contents element and matching
                    // selectors with that element in their ancestor chain.
                    // https://www.w3.org/TR/css-display-3/#valdef-display-contents
                    let flattened_style = flattened_contents_inheritance_style(&style);
                    let mut child_ancestors = ancestors.to_vec();
                    child_ancestors.push(signature);
                    let built = build_child_boxes_inner(
                        child_element,
                        stylesheets,
                        &flattened_style,
                        &child_ancestors,
                        false,
                        true,
                        font_system.as_deref_mut(),
                    );
                    raw.extend(built.boxes);
                    footnotes.extend(built.footnotes);
                    counter_events.extend(built.counter_events);
                    suppressed_named_string_events.extend(built.suppressed_named_string_events);
                } else if style.display.is_none() {
                    pending_suppressed_named_string_events.extend(
                        suppressed_named_string_events_for_subtree(
                            child_element,
                            style,
                            stylesheets,
                            ancestors,
                        ),
                    );
                    counter_events.push(suppressed_counter_event_for_subtree(
                        child_element,
                        ancestors,
                    ));
                } else {
                    let is_footnote = style.float == Float::Footnote;
                    let footnote_display = style.footnote_display;
                    let footnote_policy = style.footnote_policy;
                    let footnote_call_style = style.footnote_call_style.clone();
                    let footnote_marker_style = style.footnote_marker_style.clone();
                    let Some(mut built) = build_element_box(
                        child_element,
                        signature,
                        style,
                        stylesheets,
                        ancestors,
                        font_system.as_deref_mut(),
                    ) else {
                        continue;
                    };
                    suppressed_named_string_events.extend(
                        pending_suppressed_named_string_events
                            .drain(..)
                            .map(|mut event| {
                                event.target = SuppressedNamedStringEventTarget::BeforeElement(
                                    child_element.id,
                                );
                                event
                            }),
                    );
                    if is_footnote {
                        let mut call_boxes = Vec::new();
                        let mut call_counter_events = Vec::new();
                        push_generated_pseudo_box(
                            &mut call_boxes,
                            &mut call_counter_events,
                            child_element,
                            parent_style,
                            footnote_call_style.as_deref(),
                            GeneratedPseudoKind::FootnoteCall,
                        );
                        // GCPM increments the `footnote` counter at the
                        // footnote's source position. Its call is then a
                        // child event, so both the call and detached body see
                        // the same stable post-increment counter snapshot.
                        // https://www.w3.org/TR/css-gcpm-3/#footnote-counter
                        built.counter_event.counter_style.counter_increments.push(
                            css::CounterChange {
                                name: "footnote".to_string(),
                                value: css::CounterValue::new(1),
                            },
                        );
                        built
                            .counter_event
                            .children
                            .splice(0..0, call_counter_events);
                        if let Some(marker_style) = footnote_marker_style.as_deref()
                            && marker_style.content.is_generated()
                        {
                            built.counter_event.children.insert(
                                1,
                                CounterEventNode {
                                    element: child_element,
                                    source: CounterEventSource::FootnoteMarker,
                                    counter_style: CounterEventStyle::from_computed(marker_style),
                                    children: Vec::new(),
                                },
                            );
                        }
                        built.box_.style_mut().float = Float::None;
                        raw.extend(call_boxes);
                        footnotes.push(MutableFootnoteBox {
                            element: child_element,
                            body: built.box_,
                            display: footnote_display,
                            policy: footnote_policy,
                        });
                    } else {
                        raw.push(built.box_);
                    }
                    footnotes.extend(built.footnotes);
                    counter_events.push(built.counter_event);
                    suppressed_named_string_events.extend(built.suppressed_named_string_events);
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
    suppressed_named_string_events.extend(pending_suppressed_named_string_events.drain(..).map(
        |mut event| {
            event.target = SuppressedNamedStringEventTarget::AfterElement(element.id);
            event
        },
    ));
    let boxes = if normalize_for_parent {
        normalize_block_container_children(raw, parent_style)
    } else {
        raw
    };
    BuiltChildren {
        boxes,
        footnotes,
        counter_events,
        suppressed_named_string_events,
    }
}

/// Inline SVG roots whose parent is outside SVG participate in CSS box layout.
/// CSS Display therefore makes `display: contents` compute to `none` for the
/// root itself, rather than flattening SVG scene children into HTML layout.
/// <https://drafts.csswg.org/css-display-3/#unbox-svg>
fn display_contents_computes_to_none_for_css_layout_svg_root(
    element: &Element,
    parent: &Element,
    style: &ComputedStyle,
) -> bool {
    style.display.is_contents()
        && element.namespace_url == "http://www.w3.org/2000/svg"
        && element.tag == "svg"
        && parent.namespace_url != "http://www.w3.org/2000/svg"
}

fn suppressed_named_string_events_for_subtree(
    element: &Element,
    style: ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
) -> Vec<SuppressedNamedStringEvent> {
    let mut events = Vec::new();
    if !style.string_sets.is_empty() {
        events.push(SuppressedNamedStringEvent {
            element: element.clone(),
            style: style.clone(),
            // The surrounding child builder replaces this temporary target
            // with the following source boundary.
            target: SuppressedNamedStringEventTarget::AfterElement(element.id),
        });
    }
    let mut child_ancestors = ancestors.to_vec();
    child_ancestors.push(element_signature(element));
    for child in &element.children {
        let NodeKind::Element(child) = &child.kind else {
            continue;
        };
        let signature = element_signature(child);
        let child_style = style_for_layout_element(
            child,
            signature,
            stylesheets,
            Some(&style),
            &child_ancestors,
        );
        events.extend(suppressed_named_string_events_for_subtree(
            child,
            child_style,
            stylesheets,
            &child_ancestors,
        ));
    }
    events
}

fn suppressed_counter_event_for_subtree<'a>(
    element: &'a Element,
    ancestors: &[ElementSignature],
) -> CounterEventNode<'a> {
    let mut child_ancestors = ancestors.to_vec();
    child_ancestors.push(element_signature(element));
    let children = element
        .children
        .iter()
        .filter_map(|child| match &child.kind {
            NodeKind::Element(child) => Some(suppressed_counter_event_for_subtree(
                child,
                &child_ancestors,
            )),
            NodeKind::Text(_) => None,
        })
        .collect();
    CounterEventNode {
        element,
        source: CounterEventSource::Principal,
        counter_style: CounterEventStyle::suppressed_display_none(),
        children,
    }
}

fn build_element_box<'a>(
    element: &'a Element,
    signature: ElementSignature,
    style: ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    ancestors: &[ElementSignature],
    font_system: Option<&mut FontSystem>,
) -> Option<BuiltElement<'a>> {
    let mut style = Box::new(style);
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        style.abspos_static_source = css::StaticPositionSource::from_display(style.display);
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
        )
    };
    let BuiltChildren {
        boxes: children,
        footnotes,
        counter_events: mut counter_children,
        suppressed_named_string_events,
    } = built_children;
    let marker = marker_box(&style);
    if let Some(marker) = &marker {
        counter_children.insert(
            0,
            CounterEventNode {
                element,
                source: CounterEventSource::Marker,
                counter_style: CounterEventStyle::from_computed(&marker.style),
                children: Vec::new(),
            },
        );
    }
    let source = BoxSource::Principal;
    // Counter planning observes the computed style before layout-only display
    // fixups (such as replaced-element blockification), but it needs only
    // counter declarations and scope flags rather than another full style.
    let counter_style = CounterEventStyle::from_computed(&style);

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
                core: ElementBoxCoreWith {
                    element,
                    signature,
                    source,
                    style,
                    children,
                },
                marker,
                table_fragment: None,
            }))
        } else {
            Some(MutableFormattingBox::Replaced(MutableReplacedBox {
                core: ElementBoxCoreWith {
                    element,
                    signature,
                    source,
                    style,
                    children,
                },
                marker,
            }))
        }?;
        return Some(BuiltElement {
            box_,
            footnotes,
            counter_event: CounterEventNode {
                element,
                source: CounterEventSource::Principal,
                counter_style,
                children: counter_children,
            },
            suppressed_named_string_events,
        });
    }

    let box_ = if style.display.is_table() && style.display.is_inline_or_run_in_level() {
        let fragment = build_table_fragment(element, &signature, &children);
        MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
            core: ElementBoxCoreWith {
                element,
                signature,
                source,
                style,
                children,
            },
            marker,
            table_fragment: Some(fragment),
        })
    } else if style.display.is_table() {
        let fragment = build_table_fragment(element, &signature, &children);
        MutableFormattingBox::Table(MutableTableBox {
            core: ElementBoxCoreWith {
                element,
                signature,
                source,
                style,
                children,
            },
            marker,
            fragment,
        })
    } else if style.display.is_flex() && style.display.is_block_level() {
        MutableFormattingBox::Flex(MutableFlexBox {
            core: ElementBoxCoreWith {
                element,
                signature,
                source,
                style,
                children,
            },
            marker,
        })
    } else if style.display.is_atomic_inline()
        || (style.display.is_run_in() && !style.display.is_flow())
    {
        MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
            core: ElementBoxCoreWith {
                element,
                signature,
                source,
                style,
                children,
            },
            marker,
            table_fragment: None,
        })
    } else if style.display.is_block_level() {
        let fieldset = fieldset_formatting_box(element, &style, &children);
        MutableFormattingBox::Block(MutableBlockBox {
            core: ElementBoxCoreWith {
                element,
                signature,
                source,
                style,
                children,
            },
            marker,
            run_in_children: Vec::new(),
            fieldset,
        })
    } else {
        MutableFormattingBox::Inline(MutableInlineBox {
            core: ElementBoxCoreWith {
                element,
                signature,
                source,
                style,
                children,
            },
            marker,
            fragment_edges: InlineBoxFragmentEdges::ALL,
        })
    };
    Some(BuiltElement {
        box_,
        footnotes,
        counter_event: CounterEventNode {
            element,
            source: CounterEventSource::Principal,
            counter_style,
            children: counter_children,
        },
        suppressed_named_string_events,
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
        style.abspos_static_source = css::StaticPositionSource::from_display(style.display);
        style.display = style.display.blockified();
    }
    // Tree-abiding generated boxes are direct flex/grid children just like
    // principal element boxes. CSS Display blockifies them before CSS Tables
    // applies anonymous-wrapper fixup; delaying this until flex/grid
    // itemization lets `display: table-row`/`table-cell` manufacture an empty
    // table fragment and loses its generated content.
    // <https://drafts.csswg.org/css-display-3/#transformations>
    // <https://drafts.csswg.org/css-flexbox-1/#flex-items>
    // <https://drafts.csswg.org/css-tables-3/#fixup-algorithm>
    if originating_style.display.is_flex() || originating_style.display.is_grid() {
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
            source: kind.counter_event_source(),
            counter_style: CounterEventStyle::from_computed(pseudo_style),
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
            core: ElementBoxCoreWith {
                element: originating_element,
                signature: originating_signature,
                source,
                style,
                children,
            },
            marker,
            table_fragment: Some(fragment),
        }))
    } else if style.display.is_table() {
        let fragment = build_table_fragment(originating_element, &originating_signature, &children);
        Some(MutableFormattingBox::Table(MutableTableBox {
            core: ElementBoxCoreWith {
                element: originating_element,
                signature: originating_signature,
                source,
                style,
                children,
            },
            marker,
            fragment,
        }))
    } else if style.display.is_flex() && style.display.is_block_level() {
        Some(MutableFormattingBox::Flex(MutableFlexBox {
            core: ElementBoxCoreWith {
                element: originating_element,
                signature: originating_signature,
                source,
                style,
                children,
            },
            marker,
        }))
    } else if style.display.is_atomic_inline()
        || (style.display.is_run_in() && !style.display.is_flow())
    {
        Some(MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
            core: ElementBoxCoreWith {
                element: originating_element,
                signature: originating_signature,
                source,
                style,
                children,
            },
            marker,
            table_fragment: None,
        }))
    } else if style.display.is_block_level() {
        let fieldset = fieldset_formatting_box(originating_element, &style, &children);
        Some(MutableFormattingBox::Block(MutableBlockBox {
            core: ElementBoxCoreWith {
                element: originating_element,
                signature: originating_signature,
                source,
                style,
                children,
            },
            marker,
            run_in_children: Vec::new(),
            fieldset,
        }))
    } else {
        Some(MutableFormattingBox::Inline(MutableInlineBox {
            core: ElementBoxCoreWith {
                element: originating_element,
                signature: originating_signature,
                source,
                style,
                children,
            },
            marker,
            fragment_edges: InlineBoxFragmentEdges::ALL,
        }))
    }
}

/// Select the rendered-legend candidate while the direct child formatting
/// boxes still retain their source order.
///
/// HTML promotes the first direct `legend` box that remains in normal flow;
/// generated pseudo boxes and nested legends are ordinary anonymous fieldset
/// content instead.
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
fn fieldset_formatting_box(
    element: &Element,
    style: &ComputedStyle,
    children: &[MutableFormattingBox<'_>],
) -> Option<FieldsetFormattingBox> {
    if !element.tag.eq_ignore_ascii_case("fieldset") || !style.display.is_block_level() {
        return None;
    }
    Some(FieldsetFormattingBox::from_children(children))
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
    apply_table_fragment_missing_cells_fixup(&mut rows, &columns);
    let grid = table_fragment_grid(&rows);
    MutableTableFragment {
        rows,
        captions,
        columns,
        grid,
    }
}

/// Complete the row/column grid with anonymous cells after its column count is
/// known.
///
/// CSS Tables derives the grid from the HTML table algorithm before requiring
/// every slot to be occupied.  Explicit `col` and `colgroup` tracks therefore
/// participate even when a source row supplies fewer cells; missing cells are
/// appended as anonymous table-cell boxes rather than represented as empty
/// geometry.
/// <https://drafts.csswg.org/css-tables-3/#dimensioning-the-row-column-grid>
/// <https://drafts.csswg.org/css-tables-3/#missing-cells-fixup>
fn apply_table_fragment_missing_cells_fixup(
    rows: &mut [MutableTableFragmentRow<'_>],
    columns: &[MutableTableFragmentColumn<'_>],
) {
    let declared_column_count = table_fragment_definite_column_count(columns);
    let required_column_count = table_fragment_grid(rows)
        .column_count
        .max(declared_column_count);
    if required_column_count == 0 {
        return;
    }

    let row_group_ends = table_fragment_row_group_end_indices(rows);
    let mut active_rowspans = Vec::new();
    for (row_index, row) in rows.iter_mut().enumerate() {
        let mut column = 0usize;
        for cell in &row.cells {
            while active_rowspans.get(column).copied().unwrap_or(0) > 0 {
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
            column = end;
        }

        while (0..required_column_count)
            .any(|index| active_rowspans.get(index).copied().unwrap_or(0) == 0)
        {
            while active_rowspans.get(column).copied().unwrap_or(0) > 0 {
                column += 1;
            }
            if column >= required_column_count {
                break;
            }
            row.cells.push(MutableTableFragmentCell {
                element: None,
                signature: ElementSignature::new("td", HashMap::new()),
                style: None,
                children: Vec::new(),
                anonymous: true,
            });
            if active_rowspans.len() <= column {
                active_rowspans.resize(column + 1, 0);
            }
            active_rowspans[column] = 1;
            column += 1;
        }

        for active in &mut active_rowspans {
            *active = active.saturating_sub(1);
        }
        while active_rowspans.last().copied() == Some(0) {
            active_rowspans.pop();
        }
    }
}

/// Return the grid extent still required by non-zero definite column tracks.
///
/// The HTML formatting algorithm may expose more column tracks than a table
/// needs. In auto layout those unneeded tracks are merged before missing-cell
/// fixup unless their outer measure is definite and non-zero. In particular,
/// percentage and mixed length-percentage declarations remain cyclic while
/// the intrinsic grid is being constructed, and a zero `width`/`min-width`/
/// `max-width` does not keep an otherwise absent trailing track alive.
/// <https://drafts.csswg.org/css-tables-3/#track-merging>
/// <https://drafts.csswg.org/css-tables-3/#computing-column-measures>
fn table_fragment_definite_column_count(columns: &[MutableTableFragmentColumn<'_>]) -> usize {
    let mut column_end = 0;
    let mut required_end = 0;
    for column in columns {
        let span = column.span.max(1);
        column_end += span;
        let group_measure = column
            .group
            .as_ref()
            .and_then(|group| group.style.as_deref())
            .map(table_fragment_column_definite_outer_measure)
            .unwrap_or(0.0);
        let column_measure = column
            .style
            .as_deref()
            .map(table_fragment_column_definite_outer_measure)
            .unwrap_or(0.0);
        if group_measure.max(column_measure) > 0.0 {
            required_end = column_end;
        }
    }
    required_end
}

/// Return a column's non-cyclic outer measure before the table width exists.
///
/// This is the fixed-value portion of CSS Tables' column outer min/max
/// measures. A value involving a percentage is deliberately not partially
/// reduced to its length component: it needs the eventual table width and is
/// not definite at grid-construction time.
fn table_fragment_column_definite_outer_measure(style: &ComputedStyle) -> f32 {
    let (min_width, width, max_width) = if style.writing_mode.has_vertical_lines() {
        (
            &style.box_values.min_height,
            style.box_values.height.value(),
            &style.box_values.max_height,
        )
    } else {
        (
            &style.box_values.min_width,
            &style.box_values.width,
            &style.box_values.max_width,
        )
    };
    let min = table_fragment_definite_length(min_width).unwrap_or(0.0);
    let preferred = table_fragment_definite_length(width).unwrap_or(0.0);
    let maximum = table_fragment_definite_length(max_width).unwrap_or(f32::INFINITY);
    min.max(preferred.min(maximum)).max(0.0)
}

fn table_fragment_definite_length(value: &css::ComputedLengthPercentageOrAuto) -> Option<f32> {
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
            if !value.needs_percentage_basis() =>
        {
            Some(value.length_points())
        }
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
        | css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => None,
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
    // The generated cell is a block container. Normalize its children only
    // after synthesizing that cell, so CSS Tables fixup sees the correct
    // parent and CSS 2.2 can split in-flow blocks out of inline descendants.
    // <https://drafts.csswg.org/css-tables/#fixup-algorithm>
    // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
    let normalized = normalize_block_container_children(std::mem::take(children), &parent_style);
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
    let inherited_parent_style = children
        .first()
        .map(table_fragment_child_style)
        .unwrap_or_else(|| css::default_style_for_tag("td"));
    // CSS Tables' generated cell inherits through the table structure, not
    // by cloning a preceding improper child as its principal box. Retain the
    // inherited typographic values available from that child, but reset all
    // non-inherited decoration before the anonymous cell later wraps a
    // trailing inline run.
    // <https://drafts.csswg.org/css-tables/#fixup-algorithm>
    // <https://www.w3.org/TR/CSS22/visuren.html#anonymous>
    let mut style = css::anonymous_block_style(&inherited_parent_style);
    style.display = Display::TABLE_CELL;
    style
}

fn table_fragment_child_style(child: &MutableFormattingBox<'_>) -> ComputedStyle {
    child.style().clone()
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

fn is_table_caption_box(_element: &Element, style: &ComputedStyle) -> bool {
    style.display.is_table_caption()
}

fn is_table_column_group_box(_element: &Element, style: &ComputedStyle) -> bool {
    style.display.is_table_column_group()
}

fn is_table_column_box(_element: &Element, style: &ComputedStyle) -> bool {
    style.display.is_table_column()
}

fn is_table_row_group_box(_element: &Element, style: &ComputedStyle) -> bool {
    style.display.is_table_row_group()
}

fn is_table_row_box(_element: &Element, style: &ComputedStyle) -> bool {
    style.display.is_table_row()
}

fn is_table_cell_box(_element: &Element, style: &ComputedStyle) -> bool {
    style.display.is_table_cell()
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
