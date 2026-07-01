use super::*;

pub(super) fn positioned_grid_static_probe_child<'a>(child: &GridChild<'a>) -> GridChild<'a> {
    let mut style = child.style.clone();
    style.position = Position::Static;
    if style.display.is_inline_level() {
        style.display = style.display.blockified();
    }
    style.margin = css::Edges::ZERO;
    style.box_values.width = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.height = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.min_width = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.min_height = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.max_width = css::ComputedLengthPercentageOrAuto::ZERO;
    style.box_values.max_height = css::ComputedLengthPercentageOrAuto::ZERO;
    GridChild {
        kind: GridChildKind::AnonymousContent {
            children: Vec::new(),
        },
        style,
    }
}

#[derive(Debug, Clone)]
pub(super) struct GridChild<'a> {
    kind: GridChildKind<'a>,
    pub(super) style: ComputedStyle,
}

#[derive(Debug, Clone)]
enum GridChildKind<'a> {
    Element {
        element: &'a Element,
        signature: Box<ElementSignature>,
        children: Option<std::borrow::Cow<'a, [box_tree::FormattingBox<'a>]>>,
    },
    AnonymousContent {
        children: Vec<box_tree::FormattingBox<'a>>,
    },
}

impl<'a> GridChild<'a> {
    pub(super) fn element_parts(
        &self,
    ) -> Option<(
        &'a Element,
        &ElementSignature,
        Option<&[box_tree::FormattingBox<'a>]>,
    )> {
        match &self.kind {
            GridChildKind::Element {
                element,
                signature,
                children,
            } => Some((*element, signature.as_ref(), children.as_deref())),
            GridChildKind::AnonymousContent { .. } => None,
        }
    }

    pub(super) fn anonymous_content(&self) -> Option<&[box_tree::FormattingBox<'a>]> {
        match &self.kind {
            GridChildKind::AnonymousContent { children } => Some(children),
            GridChildKind::Element { .. } => None,
        }
    }
}

pub(super) fn grid_child_lists_from_boxes<'a>(
    child_boxes: &'a [box_tree::FormattingBox<'a>],
) -> (Vec<GridChild<'a>>, Vec<GridChild<'a>>) {
    let mut in_flow = Vec::new();
    let mut positioned = Vec::new();
    let mut anonymous_run = Vec::new();
    for box_ in child_boxes {
        if matches!(box_, box_tree::FormattingBox::Text(_)) {
            anonymous_run.push(box_.clone());
            continue;
        }
        flush_anonymous_grid_run(&mut in_flow, &mut anonymous_run);
        let Some((element, signature, style, children)) = box_.element_parts() else {
            continue;
        };
        if style.display.is_none() {
            continue;
        }
        let source_display = style.display;
        let mut style = style.clone();
        style.display = style.display.blockified();
        let children = grid_item_children(source_display, &style, children);
        let child = GridChild {
            kind: GridChildKind::Element {
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
    flush_anonymous_grid_run(&mut in_flow, &mut anonymous_run);
    in_flow.sort_by_key(|child| child.style.order);
    (in_flow, positioned)
}

fn grid_item_children<'a>(
    source_display: Display,
    grid_item_style: &ComputedStyle,
    children: &'a [box_tree::FormattingBox<'a>],
) -> std::borrow::Cow<'a, [box_tree::FormattingBox<'a>]> {
    if source_display.is_inline_or_run_in_level()
        && source_display.is_flow()
        && grid_item_style.display.is_block_level()
        && grid_item_style.display.is_flow()
    {
        return std::borrow::Cow::Owned(box_tree::normalize_block_container_children(
            children.to_vec(),
            grid_item_style,
        ));
    }
    std::borrow::Cow::Borrowed(children)
}

fn flush_anonymous_grid_run<'a>(
    children: &mut Vec<GridChild<'a>>,
    anonymous_run: &mut Vec<box_tree::FormattingBox<'a>>,
) {
    if anonymous_run.is_empty() {
        return;
    }
    if anonymous_run
        .iter()
        .all(box_tree::formatting_box_is_collapsible_space)
    {
        anonymous_run.clear();
        return;
    }
    let style = anonymous_grid_item_style(&anonymous_run[0]);
    children.push(GridChild {
        kind: GridChildKind::AnonymousContent {
            children: std::mem::take(anonymous_run),
        },
        style,
    });
}

fn anonymous_grid_item_style(source: &box_tree::FormattingBox<'_>) -> ComputedStyle {
    let mut style = css::style_for_element_with_signature(
        ElementSignature::new("__quire_anonymous_grid_item", HashMap::new()),
        None,
        &[],
        Some(formatting_box_style(source)),
        &[],
    );
    style.display = Display::BLOCK;
    style
}

fn formatting_box_style<'a>(box_: &'a box_tree::FormattingBox<'a>) -> &'a ComputedStyle {
    match box_ {
        box_tree::FormattingBox::Block(box_) => &box_.style,
        box_tree::FormattingBox::Inline(box_) => &box_.style,
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => &box_.style,
        box_tree::FormattingBox::AnonymousBlock(box_) => &box_.style,
        box_tree::FormattingBox::AtomicInline(box_) => &box_.style,
        box_tree::FormattingBox::Line(box_) => &box_.style,
        box_tree::FormattingBox::Text(box_) => &box_.style,
        box_tree::FormattingBox::Table(box_) => &box_.style,
        box_tree::FormattingBox::Flex(box_) => &box_.style,
        box_tree::FormattingBox::Replaced(box_) => &box_.style,
    }
}
