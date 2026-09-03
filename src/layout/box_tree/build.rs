use std::rc::Rc;

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
        NodeKind::Element(element) => build_child_boxes_iterative(
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
    style.first_line_overrides = css::ModeledLonghandSet::empty();
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
    build_child_boxes_iterative(
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

/// A completed direct-child traversal, before its principal element box has
/// been attached to its parent's source-order output.
enum IterativeBuildResult<'a> {
    Root(BuiltChildren<'a>),
    Element(Box<IterativeElementBuild<'a>>),
    Contents(BuiltChildren<'a>),
}

struct IterativeElementBuild<'a> {
    element: &'a Element,
    signature: ElementSignature,
    style: Box<ComputedStyle>,
    children: BuiltChildren<'a>,
}

enum IterativeBuildCompletion<'a> {
    Root,
    Element {
        element: &'a Element,
        signature: Box<ElementSignature>,
    },
    Contents,
}

/// Heap-resident state for a single element's direct-child phase.
///
/// Keeping this state in a `Box` is important: a page with deeply nested
/// elements must grow the explicit work list, rather than retain one large
/// Rust stack frame for each ancestor.
struct IterativeBuildFrame<'a> {
    element: &'a Element,
    style: Box<ComputedStyle>,
    normalize_for_parent: bool,
    text_parent_is_flattened_contents: bool,
    completion: IterativeBuildCompletion<'a>,
    sibling_tags: ElementSiblingSignatureList,
    next_child_node_index: usize,
    next_element_index: usize,
    built: BuiltChildren<'a>,
    pending_suppressed_named_string_events: Vec<SuppressedNamedStringEvent>,
}

impl<'a> IterativeBuildFrame<'a> {
    fn new(
        element: &'a Element,
        style: Box<ComputedStyle>,
        normalize_for_parent: bool,
        text_parent_is_flattened_contents: bool,
        completion: IterativeBuildCompletion<'a>,
    ) -> Self {
        let mut built = BuiltChildren::default();
        push_generated_pseudo_box(
            &mut built.boxes,
            &mut built.counter_events,
            element,
            &style,
            style.before_style.as_deref(),
            GeneratedPseudoKind::Before,
        );
        Self {
            element,
            style,
            normalize_for_parent,
            text_parent_is_flattened_contents,
            completion,
            sibling_tags: element_sibling_signature_list(element),
            next_child_node_index: 0,
            next_element_index: 0,
            built,
            pending_suppressed_named_string_events: Vec::new(),
        }
    }

    fn finish(mut self) -> IterativeBuildResult<'a> {
        push_generated_pseudo_box(
            &mut self.built.boxes,
            &mut self.built.counter_events,
            self.element,
            &self.style,
            self.style.after_style.as_deref(),
            GeneratedPseudoKind::After,
        );
        self.built.suppressed_named_string_events.extend(
            self.pending_suppressed_named_string_events
                .drain(..)
                .map(|mut event| {
                    event.target = SuppressedNamedStringEventTarget::AfterElement(self.element.id);
                    event
                }),
        );
        if self.normalize_for_parent {
            self.built.boxes = normalize_block_container_children(self.built.boxes, &self.style);
        }
        match self.completion {
            IterativeBuildCompletion::Root => IterativeBuildResult::Root(self.built),
            IterativeBuildCompletion::Contents => IterativeBuildResult::Contents(self.built),
            IterativeBuildCompletion::Element { element, signature } => {
                IterativeBuildResult::Element(Box::new(IterativeElementBuild {
                    element,
                    signature: *signature,
                    style: self.style,
                    children: self.built,
                }))
            }
        }
    }
}

enum IterativeBuildWork<'a> {
    Frame(Box<IterativeBuildFrame<'a>>),
    AppendElement,
    AppendContents,
    PopAncestor(Box<ElementSignature>),
}

struct IterativeBoxTreeBuilder<'a> {
    ancestors: Vec<ElementSignature>,
    work: Vec<IterativeBuildWork<'a>>,
    result: Option<IterativeBuildResult<'a>>,
}

impl<'a> IterativeBoxTreeBuilder<'a> {
    fn new(
        element: &'a Element,
        parent_style: &ComputedStyle,
        ancestors: &[ElementSignature],
        normalize_for_parent: bool,
        text_parent_is_flattened_contents: bool,
    ) -> Self {
        Self {
            ancestors: ancestors.to_vec(),
            work: vec![IterativeBuildWork::Frame(Box::new(
                IterativeBuildFrame::new(
                    element,
                    Box::new(parent_style.clone()),
                    normalize_for_parent,
                    text_parent_is_flattened_contents,
                    IterativeBuildCompletion::Root,
                ),
            ))],
            result: None,
        }
    }

    fn build(
        mut self,
        stylesheets: &Stylesheets<'_>,
        mut font_system: Option<&mut FontSystem>,
    ) -> BuiltChildren<'a> {
        while let Some(work) = self.work.pop() {
            match work {
                IterativeBuildWork::Frame(frame) => {
                    self.step_frame(frame, stylesheets, font_system.as_deref_mut())
                }
                IterativeBuildWork::AppendElement => self.append_completed_element(),
                IterativeBuildWork::AppendContents => self.append_completed_contents(),
                IterativeBuildWork::PopAncestor(signature) => {
                    let popped = self
                        .ancestors
                        .pop()
                        .expect("descendant traversal must retain its ancestor");
                    debug_assert_eq!(popped, *signature);
                }
            }
        }
        match self
            .result
            .take()
            .expect("root box-tree frame must complete")
        {
            IterativeBuildResult::Root(built) => built,
            IterativeBuildResult::Element(_) | IterativeBuildResult::Contents(_) => {
                unreachable!("only the root frame may complete the build")
            }
        }
    }

    fn step_frame(
        &mut self,
        mut frame: Box<IterativeBuildFrame<'a>>,
        stylesheets: &Stylesheets<'_>,
        font_system: Option<&mut FontSystem>,
    ) {
        let Some(child) = frame.element.children.get(frame.next_child_node_index) else {
            self.result = Some(frame.finish());
            return;
        };
        frame.next_child_node_index += 1;
        match &child.kind {
            NodeKind::Text(text) => {
                if !text.is_empty() && !element_suppresses_direct_text_children(frame.element) {
                    frame
                        .built
                        .boxes
                        .push(MutableFormattingBox::Text(MutableTextBox {
                            text: text.clone(),
                            style: if frame.text_parent_is_flattened_contents {
                                flattened_contents_text_style(&frame.style)
                            } else {
                                inherited_text_style(&frame.style)
                            },
                        }));
                }
                self.work.push(IterativeBuildWork::Frame(frame));
            }
            NodeKind::Element(child_element) => {
                let signature = ElementSignature::from_sibling_snapshot(
                    frame.next_element_index,
                    frame.sibling_tags.clone(),
                )
                .expect("source child must have a cached sibling signature");
                frame.next_element_index += 1;
                if is_html_select_item_element(child_element)
                    && !has_html_select_context(frame.element, &self.ancestors)
                {
                    self.work.push(IterativeBuildWork::Frame(frame));
                    return;
                }
                let mut style = style_for_child_iterative(
                    child_element,
                    signature.clone(),
                    stylesheets,
                    &frame.style,
                    &self.ancestors,
                    font_system,
                );
                if self.ancestors.is_empty() {
                    style = root_display_fixed_style(style);
                }
                prepare_flattened_contents_child_style(
                    &mut style,
                    frame.text_parent_is_flattened_contents,
                );
                if display_contents_computes_to_none_for_css_layout_svg_root(
                    child_element,
                    frame.element,
                    &style,
                ) {
                    style.display = Display::NONE;
                }
                if frame.style.display.is_flex() || frame.style.display.is_grid() {
                    style.display = style.display.blockified();
                }
                if style.display.is_none() {
                    frame.pending_suppressed_named_string_events.extend(
                        suppressed_named_string_events_for_subtree_iterative(
                            child_element,
                            style,
                            stylesheets,
                            &self.ancestors,
                        ),
                    );
                    frame.built.counter_events.push(
                        suppressed_counter_event_for_subtree_iterative(child_element),
                    );
                    self.work.push(IterativeBuildWork::Frame(frame));
                    return;
                }
                self.ancestors.push(signature.clone());
                self.work.push(IterativeBuildWork::Frame(frame));
                if style.display.is_contents() {
                    self.work.push(IterativeBuildWork::AppendContents);
                    self.work
                        .push(IterativeBuildWork::PopAncestor(Box::new(signature)));
                    self.work.push(IterativeBuildWork::Frame(Box::new(
                        IterativeBuildFrame::new(
                            child_element,
                            Box::new(flattened_contents_inheritance_style(&style)),
                            false,
                            true,
                            IterativeBuildCompletion::Contents,
                        ),
                    )));
                } else {
                    if matches!(style.position, Position::Absolute | Position::Fixed) {
                        style.abspos_static_source =
                            css::StaticPositionSource::from_display(style.display);
                        style.display = style.display.blockified();
                    }
                    self.work.push(IterativeBuildWork::AppendElement);
                    self.work
                        .push(IterativeBuildWork::PopAncestor(Box::new(signature.clone())));
                    if matches!(style.content, Content::Replacement { .. })
                        || is_horizontal_rule_element(child_element)
                    {
                        self.result = Some(IterativeBuildResult::Element(Box::new(
                            IterativeElementBuild {
                                element: child_element,
                                signature,
                                style: Box::new(style),
                                children: BuiltChildren::default(),
                            },
                        )));
                    } else {
                        self.work.push(IterativeBuildWork::Frame(Box::new(
                            IterativeBuildFrame::new(
                                child_element,
                                Box::new(style),
                                true,
                                false,
                                IterativeBuildCompletion::Element {
                                    element: child_element,
                                    signature: Box::new(signature),
                                },
                            ),
                        )));
                    }
                }
            }
        }
    }

    fn append_completed_element(&mut self) {
        let IterativeBuildResult::Element(element) = self
            .result
            .take()
            .expect("element child must complete before its continuation")
        else {
            unreachable!("element continuation must receive an element result")
        };
        let Some(IterativeBuildWork::Frame(parent)) = self.work.last_mut() else {
            unreachable!("element continuation must resume a parent frame")
        };
        append_iterative_element_build(parent, *element);
    }

    fn append_completed_contents(&mut self) {
        let IterativeBuildResult::Contents(mut contents) = self
            .result
            .take()
            .expect("contents child must complete before its continuation")
        else {
            unreachable!("contents continuation must receive a contents result")
        };
        let Some(IterativeBuildWork::Frame(parent)) = self.work.last_mut() else {
            unreachable!("contents continuation must resume a parent frame")
        };
        parent.built.boxes.append(&mut contents.boxes);
        parent.built.footnotes.append(&mut contents.footnotes);
        parent
            .built
            .counter_events
            .append(&mut contents.counter_events);
        parent
            .built
            .suppressed_named_string_events
            .append(&mut contents.suppressed_named_string_events);
    }
}

fn build_child_boxes_iterative<'a>(
    element: &'a Element,
    stylesheets: &Stylesheets<'_>,
    parent_style: &ComputedStyle,
    ancestors: &[ElementSignature],
    normalize_for_parent: bool,
    text_parent_is_flattened_contents: bool,
    font_system: Option<&mut FontSystem>,
) -> BuiltChildren<'a> {
    IterativeBoxTreeBuilder::new(
        element,
        parent_style,
        ancestors,
        normalize_for_parent,
        text_parent_is_flattened_contents,
    )
    .build(stylesheets, font_system)
}

fn style_for_child_iterative(
    element: &Element,
    signature: ElementSignature,
    stylesheets: &Stylesheets<'_>,
    parent_style: &ComputedStyle,
    ancestors: &[ElementSignature],
    font_system: Option<&mut FontSystem>,
) -> ComputedStyle {
    match font_system {
        Some(font_system) => {
            let parent_ch_advance = font_system.ch_advance(parent_style);
            let mut style = style_for_layout_element_with_parent_ch_advance(
                element,
                signature.clone(),
                stylesheets,
                Some(parent_style),
                ancestors,
                parent_ch_advance,
            );
            let pseudo_parent_ch_advance = font_system.ch_advance(&style);
            let pseudo_signature = layout_element_signature(element, signature, Some(parent_style));
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
            element,
            signature,
            stylesheets,
            Some(parent_style),
            ancestors,
        ),
    }
}

fn prepare_flattened_contents_child_style(style: &mut ComputedStyle, flattened: bool) {
    if !flattened {
        return;
    }
    if matches!(style.deferred_font_size, css::DeferredFontSize::Inherit) {
        style.deferred_font_size = css::DeferredFontSize::Absolute(style.font_size);
    }
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
        pseudo.deferred_font_size = css::DeferredFontSize::Absolute(style.font_size);
    }
}

fn append_iterative_element_build<'a>(
    parent: &mut IterativeBuildFrame<'a>,
    element: IterativeElementBuild<'a>,
) {
    let mut built = materialize_iterative_element_build(element);
    let element = built.counter_event.element;
    parent.built.suppressed_named_string_events.extend(
        parent
            .pending_suppressed_named_string_events
            .drain(..)
            .map(|mut event| {
                event.target = SuppressedNamedStringEventTarget::BeforeElement(element.id);
                event
            }),
    );
    let (
        is_footnote,
        footnote_display,
        footnote_policy,
        footnote_call_style,
        footnote_marker_style,
    ) = {
        let style = built.box_.style();
        (
            style.float == Float::Footnote,
            style.footnote_display,
            style.footnote_policy,
            style.footnote_call_style.clone(),
            style.footnote_marker_style.clone(),
        )
    };
    if is_footnote {
        let mut call_boxes = Vec::new();
        let mut call_counter_events = Vec::new();
        push_generated_pseudo_box(
            &mut call_boxes,
            &mut call_counter_events,
            element,
            &parent.style,
            footnote_call_style.as_deref(),
            GeneratedPseudoKind::FootnoteCall,
        );
        built
            .counter_event
            .counter_style
            .counter_increments
            .push(css::CounterChange {
                name: "footnote".to_string(),
                value: css::CounterValue::new(1),
            });
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
                    element,
                    source: CounterEventSource::FootnoteMarker,
                    counter_style: CounterEventStyle::from_computed(marker_style),
                    children: Vec::new(),
                },
            );
        }
        built.box_.style_mut().float = Float::None;
        parent.built.boxes.extend(call_boxes);
        parent.built.footnotes.push(MutableFootnoteBox {
            element,
            body: built.box_,
            display: footnote_display,
            policy: footnote_policy,
        });
    } else {
        let scroll_marker_group = build_scroll_marker_group(&built.box_);
        let placement = built
            .box_
            .style()
            .scroll_marker_group
            .map(|group| group.placement);
        match (placement, scroll_marker_group) {
            (Some(css::ScrollMarkerGroupPlacement::Before), Some(group)) => {
                parent.built.boxes.push(group);
                parent.built.boxes.push(built.box_);
            }
            (Some(css::ScrollMarkerGroupPlacement::After), Some(group)) => {
                parent.built.boxes.push(built.box_);
                parent.built.boxes.push(group);
            }
            _ => parent.built.boxes.push(built.box_),
        }
    }
    parent.built.footnotes.extend(built.footnotes);
    parent.built.counter_events.push(built.counter_event);
    parent
        .built
        .suppressed_named_string_events
        .extend(built.suppressed_named_string_events);
}

fn materialize_iterative_element_build(element: IterativeElementBuild<'_>) -> BuiltElement<'_> {
    let IterativeElementBuild {
        element,
        signature,
        mut style,
        children,
    } = element;
    let content_replacement = matches!(style.content, Content::Replacement { .. });
    let BuiltChildren {
        boxes: children,
        footnotes,
        counter_events: mut counter_children,
        suppressed_named_string_events,
    } = children;
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
    let counter_style = CounterEventStyle::from_computed(&style);
    let source = BoxSource::Principal;
    let box_ = if content_replacement || is_replaced_element(element) {
        style.display = if style.display.is_block_level() {
            Display::BLOCK_REPLACED.with_list_item(style.display.is_list_item())
        } else if style.display.is_run_in() {
            style.display.with_inner(DisplayInner::Replaced)
        } else {
            Display::INLINE_REPLACED.with_list_item(style.display.is_list_item())
        };
        if style.display.is_inline_or_run_in_level() {
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
        } else {
            MutableFormattingBox::Replaced(MutableReplacedBox {
                core: ElementBoxCoreWith {
                    element,
                    signature,
                    source,
                    style,
                    children,
                },
                marker,
            })
        }
    } else if style.display.is_table() && style.display.is_inline_or_run_in_level() {
        let fragment = build_table_fragment(element, &signature, &style, &children);
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
        let fragment = build_table_fragment(element, &signature, &style, &children);
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
    BuiltElement {
        box_,
        footnotes,
        counter_event: CounterEventNode {
            element,
            source: CounterEventSource::Principal,
            counter_style,
            children: counter_children,
        },
        suppressed_named_string_events,
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn build_child_boxes_recursive<'a>(
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
                    let built = build_child_boxes_recursive(
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
                    let Some(mut built) = build_element_box_recursive(
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

enum SuppressedNamedStringWork<'a> {
    Visit {
        element: &'a Element,
        style: Rc<ComputedStyle>,
    },
    Child {
        element: &'a Element,
        parent_style: Rc<ComputedStyle>,
    },
    PopAncestor(Box<ElementSignature>),
}

fn suppressed_named_string_events_for_subtree_iterative(
    element: &Element,
    style: ComputedStyle,
    stylesheets: &Stylesheets<'_>,
    initial_ancestors: &[ElementSignature],
) -> Vec<SuppressedNamedStringEvent> {
    let mut ancestors = initial_ancestors.to_vec();
    let mut work = vec![SuppressedNamedStringWork::Visit {
        element,
        style: Rc::new(style),
    }];
    let mut events = Vec::new();
    while let Some(work_item) = work.pop() {
        match work_item {
            SuppressedNamedStringWork::Visit { element, style } => {
                if !style.string_sets.is_empty() {
                    events.push(SuppressedNamedStringEvent {
                        element: element.clone(),
                        style: (*style).clone(),
                        target: SuppressedNamedStringEventTarget::AfterElement(element.id),
                    });
                }
                let signature = element_signature(element);
                ancestors.push(signature.clone());
                work.push(SuppressedNamedStringWork::PopAncestor(Box::new(signature)));
                for child in element.children.iter().rev() {
                    let NodeKind::Element(child) = &child.kind else {
                        continue;
                    };
                    work.push(SuppressedNamedStringWork::Child {
                        element: child,
                        parent_style: Rc::clone(&style),
                    });
                }
            }
            SuppressedNamedStringWork::Child {
                element,
                parent_style,
            } => {
                let signature = element_signature(element);
                let style = style_for_layout_element(
                    element,
                    signature,
                    stylesheets,
                    Some(&parent_style),
                    &ancestors,
                );
                work.push(SuppressedNamedStringWork::Visit {
                    element,
                    style: Rc::new(style),
                });
            }
            SuppressedNamedStringWork::PopAncestor(signature) => {
                let popped = ancestors
                    .pop()
                    .expect("suppressed traversal must retain its ancestor");
                debug_assert_eq!(popped, *signature);
            }
        }
    }
    events
}

enum SuppressedCounterWork<'a> {
    Visit(&'a Element),
    Finish {
        element: &'a Element,
        child_count: usize,
    },
}

fn suppressed_counter_event_for_subtree_iterative<'a>(
    element: &'a Element,
) -> CounterEventNode<'a> {
    let mut work = vec![SuppressedCounterWork::Visit(element)];
    let mut completed = Vec::new();
    while let Some(work_item) = work.pop() {
        match work_item {
            SuppressedCounterWork::Visit(element) => {
                let children = element
                    .children
                    .iter()
                    .filter_map(|child| match &child.kind {
                        NodeKind::Element(child) => Some(child),
                        NodeKind::Text(_) => None,
                    })
                    .collect::<Vec<_>>();
                work.push(SuppressedCounterWork::Finish {
                    element,
                    child_count: children.len(),
                });
                work.extend(children.into_iter().rev().map(SuppressedCounterWork::Visit));
            }
            SuppressedCounterWork::Finish {
                element,
                child_count,
            } => {
                let children = completed.split_off(completed.len() - child_count);
                completed.push(CounterEventNode {
                    element,
                    source: CounterEventSource::Principal,
                    counter_style: CounterEventStyle::suppressed_display_none(),
                    children,
                });
            }
        }
    }
    completed
        .pop()
        .expect("suppressed counter traversal must complete its root")
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

#[allow(dead_code)]
fn build_element_box_recursive<'a>(
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
        build_child_boxes_recursive(
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
        let fragment = build_table_fragment(element, &signature, &style, &children);
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
        let fragment = build_table_fragment(element, &signature, &style, &children);
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
    style.marker_counter_origin = match kind {
        GeneratedPseudoKind::Before => css::MarkerCounterOrigin::Before,
        GeneratedPseudoKind::After => css::MarkerCounterOrigin::After,
        GeneratedPseudoKind::FootnoteCall
        | GeneratedPseudoKind::ScrollMarkerGroup
        | GeneratedPseudoKind::ScrollMarker => css::MarkerCounterOrigin::Principal,
    };
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
    build_generated_pseudo_box_with_children(
        originating_element,
        originating_signature,
        originating_clear,
        style,
        kind,
        Vec::new(),
    )
}

fn build_generated_pseudo_box_with_children<'a>(
    originating_element: &'a Element,
    originating_signature: ElementSignature,
    originating_clear: Clear,
    style: Box<ComputedStyle>,
    kind: GeneratedPseudoKind,
    children: Vec<MutableFormattingBox<'a>>,
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

    if style.display.is_table() && style.display.is_inline_or_run_in_level() {
        let fragment = build_table_fragment(
            originating_element,
            &originating_signature,
            &style,
            &children,
        );
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
        let fragment = build_table_fragment(
            originating_element,
            &originating_signature,
            &style,
            &children,
        );
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

/// An automatic marker's immutable source relationship. The marker box is an
/// external child of its owner's generated group, but its CSS inheritance,
/// generated content, and counters continue to originate at this element.
struct AutomaticScrollMarker<'a> {
    element: &'a Element,
    signature: ElementSignature,
    originating_clear: Clear,
    style: ComputedStyle,
}

fn is_scroll_container(style: &ComputedStyle) -> bool {
    style.overflow_x.is_scrollable() || style.overflow_y.is_scrollable()
}

fn collect_automatic_scroll_markers<'a>(
    box_: &MutableFormattingBox<'a>,
    is_owner: bool,
    output: &mut Vec<AutomaticScrollMarker<'a>>,
) {
    if let Some(core) = box_.element_core() {
        // A nested scrolling box owns (or discards) its own automatic markers;
        // they must never leak to an outer scroll container's generated group.
        if !is_owner && is_scroll_container(&core.style) {
            return;
        }
        if !is_owner
            && matches!(core.source, BoxSource::Principal)
            && let Some(style) = core.style.scroll_marker_style.as_deref()
            && style.content.is_generated()
        {
            output.push(AutomaticScrollMarker {
                element: core.element,
                signature: core.signature.clone(),
                originating_clear: core.style.clear,
                style: style.clone(),
            });
        }
    }
    for child in box_.children() {
        collect_automatic_scroll_markers(child, false, output);
    }
}

/// Materialize the external generated sibling required by CSS Overflow 5.
/// This deliberately runs before the parent's anonymous-box normalization, so
/// the group becomes a real flex/grid/table sibling of the scroll container.
fn build_scroll_marker_group<'a>(
    scroll_container: &MutableFormattingBox<'a>,
) -> Option<MutableFormattingBox<'a>> {
    let core = scroll_container.element_core()?;
    if !matches!(core.source, BoxSource::Principal)
        || !is_scroll_container(&core.style)
        || core.style.scroll_marker_group.is_none()
    {
        return None;
    }
    let group_style = core.style.scroll_marker_group_style.as_deref()?;
    if group_style.display.is_none() {
        return None;
    }

    let mut sources = Vec::new();
    collect_automatic_scroll_markers(scroll_container, true, &mut sources);
    let markers = sources
        .into_iter()
        .filter_map(|marker| {
            let mut style = Box::new(marker.style);
            style.marker_counter_origin = css::MarkerCounterOrigin::Principal;
            build_generated_pseudo_box_with_children(
                marker.element,
                marker.signature,
                marker.originating_clear,
                style,
                GeneratedPseudoKind::ScrollMarker,
                Vec::new(),
            )
        })
        .collect();

    let mut style = Box::new(group_style.clone());
    // The group is a sibling in the scroll container's parent. A standalone
    // inline box cannot contain the ordered marker list in normal flow.
    style.display = style.display.blockified();
    style.marker_counter_origin = css::MarkerCounterOrigin::Principal;
    build_generated_pseudo_box_with_children(
        core.element,
        core.signature.clone(),
        core.style.clear,
        style,
        GeneratedPseudoKind::ScrollMarkerGroup,
        markers,
    )
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
    table_style: &ComputedStyle,
    children: &[MutableFormattingBox<'a>],
) -> MutableTableFragment<'a> {
    let captions = table_fragment_captions(children);
    let columns = table_fragment_columns(children);
    let mut rows = Vec::new();
    collect_table_fragment_rows(
        children,
        &mut rows,
        std::slice::from_ref(signature),
        &[],
        table_style,
    );
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
    table_style: &ComputedStyle,
    children: &[FrozenFormattingBox<'a>],
) -> FrozenTableFragment<'a> {
    let mutable_children = clone_frozen_child_boxes_as_mutable(children);
    freeze_table_fragment(build_table_fragment(
        element,
        signature,
        table_style,
        &mutable_children,
    ))
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
    parent_style: &ComputedStyle,
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
            flush_anonymous_table_fragment_cell(
                &mut anonymous_cells,
                &mut anonymous_cell_children,
                &anonymous_table_fragment_row_style(parent_style),
            );
            flush_anonymous_table_fragment_row(
                rows,
                &mut anonymous_cells,
                ancestors,
                row_groups,
                parent_style,
            );
            let cells = table_fragment_row_child_cells(descendants, style);
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
            flush_anonymous_table_fragment_cell(
                &mut anonymous_cells,
                &mut anonymous_cell_children,
                &anonymous_table_fragment_row_style(parent_style),
            );
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
            flush_anonymous_table_fragment_cell(
                &mut anonymous_cells,
                &mut anonymous_cell_children,
                &anonymous_table_fragment_row_style(parent_style),
            );
            flush_anonymous_table_fragment_row(
                rows,
                &mut anonymous_cells,
                ancestors,
                row_groups,
                parent_style,
            );
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(signature.clone());
            let mut child_row_groups = row_groups.to_vec();
            child_row_groups.push(MutableTableFragmentRowGroup {
                element,
                signature: signature.clone(),
                style: Some(Box::new(style.clone())),
            });
            collect_table_fragment_rows(
                descendants,
                rows,
                &child_ancestors,
                &child_row_groups,
                style,
            );
            continue;
        }
        anonymous_cell_children.push(child.clone());
    }
    flush_anonymous_table_fragment_cell(
        &mut anonymous_cells,
        &mut anonymous_cell_children,
        &anonymous_table_fragment_row_style(parent_style),
    );
    flush_anonymous_table_fragment_row(
        rows,
        &mut anonymous_cells,
        ancestors,
        row_groups,
        parent_style,
    );
}

fn table_fragment_row_child_cells<'a>(
    children: &[MutableFormattingBox<'a>],
    row_style: &ComputedStyle,
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
            flush_anonymous_table_fragment_cell(
                &mut cells,
                &mut anonymous_cell_children,
                row_style,
            );
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
    flush_anonymous_table_fragment_cell(&mut cells, &mut anonymous_cell_children, row_style);
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
    parent_style: &ComputedStyle,
) {
    if children.is_empty() {
        return;
    }
    let (style, children) =
        anonymous_table_fragment_cell_style_and_children(children, parent_style);
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
    parent_style: &ComputedStyle,
) -> (ComputedStyle, Vec<MutableFormattingBox<'a>>) {
    // The generated cell is a block container. Normalize its children only
    // after synthesizing that cell, so CSS Tables fixup sees the correct
    // parent and CSS 2.2 can split in-flow blocks out of inline descendants.
    // <https://drafts.csswg.org/css-tables/#fixup-algorithm>
    // <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>
    let mut style = css::anonymous_block_style(parent_style);
    style.display = Display::TABLE_CELL;
    let normalized = normalize_block_container_children(std::mem::take(children), &style);
    (style, normalized)
}

/// Construct the anonymous row generated around improper table-root or
/// row-group children. The generated row inherits from its actual table
/// parent; it does not inherit non-inherited layout or paint properties from
/// the first child it encloses.
///
/// <https://drafts.csswg.org/css-tables-3/#fixup-algorithm>
fn anonymous_table_fragment_row_style(parent_style: &ComputedStyle) -> ComputedStyle {
    let mut style = css::anonymous_block_style(parent_style);
    style.display = Display::TABLE_ROW;
    style
}

fn flush_anonymous_table_fragment_row<'a>(
    rows: &mut Vec<MutableTableFragmentRow<'a>>,
    cells: &mut Vec<MutableTableFragmentCell<'a>>,
    ancestors: &[ElementSignature],
    row_groups: &[MutableTableFragmentRowGroup<'a>],
    parent_style: &ComputedStyle,
) {
    if cells.is_empty() {
        return;
    }
    let style = anonymous_table_fragment_row_style(parent_style);
    rows.push(MutableTableFragmentRow {
        element: None,
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
