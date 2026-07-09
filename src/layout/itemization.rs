use super::*;
use std::borrow::Cow;

/// Child box generated for a flex or grid formatting context.
///
/// CSS Flexbox and CSS Grid both create one item from each in-flow child, wrap
/// non-whitespace text runs in anonymous items, ignore text runs containing
/// only CSS document white space independent of the computed `white-space`
/// value, and exclude absolutely positioned descendants from item layout:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-items> and
/// <https://www.w3.org/TR/css-grid-1/#grid-items>.
#[derive(Debug, Clone)]
pub(in crate::layout) struct FormattingContextChild<'a> {
    pub(in crate::layout) kind: FormattingContextChildKind<'a>,
    pub(in crate::layout) style: ComputedStyle,
}

#[derive(Debug, Clone)]
pub(in crate::layout) enum FormattingContextChildKind<'a> {
    Element {
        element: &'a Element,
        signature: Box<ElementSignature>,
        children: Option<Cow<'a, [box_tree::FormattingBox<'a>]>>,
    },
    AnonymousContent {
        children: Vec<box_tree::FormattingBox<'a>>,
    },
}

impl<'a> FormattingContextChild<'a> {
    pub(in crate::layout) fn element_parts(
        &self,
    ) -> Option<(
        &'a Element,
        &ElementSignature,
        Option<&[box_tree::FormattingBox<'a>]>,
    )> {
        match &self.kind {
            FormattingContextChildKind::Element {
                element,
                signature,
                children,
            } => Some((*element, signature.as_ref(), children.as_deref())),
            FormattingContextChildKind::AnonymousContent { .. } => None,
        }
    }

    pub(in crate::layout) fn anonymous_content(&self) -> Option<&[box_tree::FormattingBox<'a>]> {
        match &self.kind {
            FormattingContextChildKind::AnonymousContent { children } => Some(children),
            FormattingContextChildKind::Element { .. } => None,
        }
    }

    pub(in crate::layout) fn is_replaced_element(&self) -> bool {
        self.element_parts()
            .is_some_and(|(element, _, _)| is_replaced_element(element))
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct ItemizationOptions {
    pub(in crate::layout) anonymous_item_tag: &'static str,
    pub(in crate::layout) strip_blockified_inline_text_paint: bool,
    pub(in crate::layout) establish_independent_formatting_context: bool,
}

pub(in crate::layout) fn itemize_blockified_children<'a>(
    child_boxes: &'a [box_tree::FormattingBox<'a>],
    options: ItemizationOptions,
) -> (
    Vec<FormattingContextChild<'a>>,
    Vec<FormattingContextChild<'a>>,
) {
    let mut in_flow = Vec::new();
    let mut positioned = Vec::new();
    let mut anonymous_run = Vec::new();
    for box_ in child_boxes {
        if matches!(box_, box_tree::FormattingBox::Text(_)) {
            anonymous_run.push(box_.clone());
            continue;
        }
        flush_anonymous_item_run(&mut in_flow, &mut anonymous_run, options);
        let Some((element, signature, style, children)) = box_.element_parts() else {
            continue;
        };
        if style.display.is_none() {
            continue;
        }
        let source_display = style.display;
        let mut style = style.clone();
        style.display = style.display.blockified();
        let children = blockified_item_children(
            source_display,
            &style,
            children,
            options.strip_blockified_inline_text_paint,
        );
        if options.establish_independent_formatting_context {
            style = independent_formatting_context_item_style(style);
        }
        let child = FormattingContextChild {
            kind: FormattingContextChildKind::Element {
                element,
                signature: Box::new(signature.clone()),
                children: Some(children),
            },
            style,
        };
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            positioned.push(child);
        } else {
            in_flow.push(child);
        }
    }
    flush_anonymous_item_run(&mut in_flow, &mut anonymous_run, options);
    in_flow.sort_by_key(|child| child.style.order);
    (in_flow, positioned)
}

/// Build the style used when a flex/grid item lays out ordinary flow contents.
///
/// Flex and grid items establish independent formatting contexts. For ordinary
/// flow children, Quire models that by changing the item inner display to
/// `flow-root`, which keeps descendant margins and floats contained by the item:
/// <https://www.w3.org/TR/css-display-3/#independent-formatting-context>.
pub(in crate::layout) fn independent_formatting_context_item_style(
    mut style: ComputedStyle,
) -> ComputedStyle {
    if style.display.is_inline_level() {
        style.display = style.display.blockified();
    }
    if style.display.is_flow() {
        style.display = style.display.with_inner(DisplayInner::FlowRoot);
    }
    style
}

/// Returns the child box list appropriate for a blockified flex/grid item.
///
/// If an inline flow source becomes a block flow item, its children were built
/// for an inline parent and need anonymous block-container fixup before block
/// layout consumes them:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>.
pub(in crate::layout) fn blockified_item_children<'a>(
    source_display: Display,
    item_style: &ComputedStyle,
    children: &'a [box_tree::FormattingBox<'a>],
    strip_inline_text_paint: bool,
) -> Cow<'a, [box_tree::FormattingBox<'a>]> {
    if source_display.is_inline_or_run_in_level()
        && source_display.is_flow()
        && item_style.display.is_block_level()
        && item_style.display.is_flow()
    {
        let mut children = box_tree::clone_frozen_child_boxes_as_mutable(children);
        if strip_inline_text_paint {
            strip_blockified_inline_text_paint(&mut children);
        }
        return Cow::Owned(box_tree::freeze_child_boxes(
            box_tree::normalize_block_container_children(children, item_style),
        ));
    }
    Cow::Borrowed(children)
}

fn flush_anonymous_item_run<'a>(
    children: &mut Vec<FormattingContextChild<'a>>,
    anonymous_run: &mut Vec<box_tree::FormattingBox<'a>>,
    options: ItemizationOptions,
) {
    if anonymous_run.is_empty() {
        return;
    }
    if anonymous_run.iter().all(formatting_box_is_document_space) {
        anonymous_run.clear();
        return;
    }
    let mut style = anonymous_item_style(options.anonymous_item_tag, &anonymous_run[0]);
    if options.establish_independent_formatting_context {
        style = independent_formatting_context_item_style(style);
    }
    children.push(FormattingContextChild {
        kind: FormattingContextChildKind::AnonymousContent {
            children: std::mem::take(anonymous_run),
        },
        style,
    });
}

fn formatting_box_is_document_space(box_: &box_tree::FormattingBox<'_>) -> bool {
    matches!(
        box_,
        box_tree::FormattingBox::Text(text) if text.text.chars().all(is_css_collapsible_whitespace)
    )
}

fn anonymous_item_style(tag: &'static str, source: &box_tree::FormattingBox<'_>) -> ComputedStyle {
    let mut style = css::style_for_element_with_signature(
        ElementSignature::new(tag, HashMap::new()),
        None,
        &[],
        Some(formatting_box_style(source)),
        &[],
    );
    style.display = Display::BLOCK;
    style
}

pub(in crate::layout) fn formatting_box_style<'a>(
    box_: &'a box_tree::FormattingBox<'a>,
) -> &'a ComputedStyle {
    match box_ {
        box_tree::FormattingBox::Block(box_) => &box_.style,
        box_tree::FormattingBox::Inline(box_) => &box_.style,
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => &box_.style,
        box_tree::FormattingBox::AnonymousBlock(box_) => &box_.style,
        box_tree::FormattingBox::AtomicInline(box_) => &box_.style,
        box_tree::FormattingBox::Text(box_) => &box_.style,
        box_tree::FormattingBox::Table(box_) => &box_.style,
        box_tree::FormattingBox::Flex(box_) => &box_.style,
        box_tree::FormattingBox::Replaced(box_) => &box_.style,
    }
}

fn strip_blockified_inline_text_paint(children: &mut [box_tree::MutableFormattingBox<'_>]) {
    for child in children {
        if let box_tree::MutableFormattingBox::Text(text) = child {
            strip_text_fragment_paint(&mut text.style);
        }
    }
}

fn strip_text_fragment_paint(style: &mut ComputedStyle) {
    style.background_color = None;
    style.background_image = None;
    style.background_layers.clear();
    style.border_width = 0.0;
    style.border_widths = css::Edges::ZERO;
    style.border_styles = css::BorderStyles::NONE;
    style.border_image = css::BorderImage::initial();
}
