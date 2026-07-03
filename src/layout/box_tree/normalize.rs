use super::*;
use crate::text::{trim_end_css_collapsible_whitespace, trim_start_css_collapsible_whitespace};

pub(crate) fn normalize_block_container_children<'a>(
    children: Vec<MutableFormattingBox<'a>>,
    parent_style: &ComputedStyle,
) -> Vec<MutableFormattingBox<'a>> {
    let children = normalize_orphan_table_internal_boxes(children, parent_style);
    if parent_style.display.is_table()
        || parent_style.display.is_table_row_group()
        || parent_style.display.is_table_row()
        || parent_style.display.is_flex()
        || parent_style.display.is_grid()
    {
        return children;
    }
    if !parent_style.display.establishes_block_formatting_context()
        && !parent_style.display.is_block_level()
    {
        return children;
    }
    let children = normalize_run_in_children(children, parent_style);
    let mut children = split_block_in_inline_children(children);
    trim_block_container_line_boundary_whitespace(&mut children);

    let has_inline = children
        .iter()
        .filter(|child| !is_out_of_flow_box(child))
        .any(is_inline_level_box);
    let has_block = children
        .iter()
        .filter(|child| !is_out_of_flow_box(child))
        .any(is_block_level_box);
    if !has_inline || !has_block {
        return children;
    }

    let mut normalized = Vec::new();
    let mut inline_run = Vec::new();
    for child in children {
        if is_out_of_flow_box(&child) || is_floated_box(&child) || is_inline_level_box(&child) {
            inline_run.push(child);
        } else {
            flush_anonymous_block(&mut normalized, &mut inline_run, parent_style);
            normalized.push(child);
        }
    }
    flush_anonymous_block(&mut normalized, &mut inline_run, parent_style);
    normalized
}

/// Trim collapsible whitespace at block-container line boundaries.
///
/// CSS Text removes collapsible whitespace at the start and end of each line.
/// Box-tree normalization can expose a whole block-container child list as one
/// inline run when it only contains inline-level content and out-of-flow
/// positioned boxes, so indentation whitespace at the container edges must be
/// removed before it can create a phantom line box or shift a positioned
/// descendant's hypothetical static position:
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
fn trim_block_container_line_boundary_whitespace(children: &mut Vec<MutableFormattingBox<'_>>) {
    trim_block_container_line_boundary_whitespace_start(children);
    trim_block_container_line_boundary_whitespace_end(children);
}

fn trim_block_container_line_boundary_whitespace_start(
    children: &mut Vec<MutableFormattingBox<'_>>,
) {
    loop {
        if children.first().is_some_and(formatting_box_is_empty_text) {
            children.remove(0);
            continue;
        }
        let Some(MutableFormattingBox::Text(first)) = children.first_mut() else {
            return;
        };
        if !first.style.white_space.collapses_spaces() {
            return;
        }
        first.text = trim_start_css_collapsible_whitespace(&first.text).to_string();
        if first.text.is_empty() {
            children.remove(0);
            continue;
        }
        return;
    }
}

fn trim_block_container_line_boundary_whitespace_end(children: &mut Vec<MutableFormattingBox<'_>>) {
    loop {
        if children.last().is_some_and(formatting_box_is_empty_text) {
            children.pop();
            continue;
        }
        let Some(MutableFormattingBox::Text(last)) = children.last_mut() else {
            return;
        };
        if !last.style.white_space.collapses_spaces() {
            return;
        }
        last.text = trim_end_css_collapsible_whitespace(&last.text).to_string();
        if last.text.is_empty() {
            children.pop();
            continue;
        }
        return;
    }
}

/// Resolves CSS Display run-in boxes before anonymous block wrapping.
///
/// CSS Display defines a run-in sequence as consecutive run-in siblings plus
/// intervening whitespace and out-of-flow boxes. The sequence runs into a
/// following in-flow block that does not establish a new BFC, otherwise it
/// falls back into an anonymous block with following inline-level content:
/// <https://www.w3.org/TR/css-display-3/#run-in-layout>.
fn normalize_run_in_children<'a>(
    children: Vec<MutableFormattingBox<'a>>,
    parent_style: &ComputedStyle,
) -> Vec<MutableFormattingBox<'a>> {
    if !children.iter().any(is_run_in_box) {
        return children;
    }

    let mut input = children.into_iter().peekable();
    let mut output = Vec::new();
    while let Some(child) = input.next() {
        if !is_run_in_box(&child) {
            output.push(child);
            continue;
        }

        let mut sequence = vec![inlinified_run_in_box(child)];
        while input.peek().is_some_and(|child| {
            formatting_box_is_collapsible_space(child)
                || is_out_of_flow_box(child)
                || is_run_in_box(child)
        }) {
            let child = input.next().expect("peeked child");
            sequence.push(inlinified_run_in_box(child));
        }

        if let Some(next) = input.peek()
            && run_in_target_is_eligible(next)
        {
            let mut target = input.next().expect("peeked target");
            insert_run_in_sequence(&mut target, sequence);
            output.push(target);
            continue;
        }

        let mut fallback_children = sequence;
        while input.peek().is_some_and(|child| {
            !is_run_in_box(child)
                && (is_out_of_flow_box(child)
                    || formatting_box_is_collapsible_space(child)
                    || is_inline_level_box(child))
        }) {
            fallback_children.push(input.next().expect("peeked inline fallback child"));
        }
        flush_anonymous_block(&mut output, &mut fallback_children, parent_style);
    }
    output
}

fn is_run_in_box(box_: &MutableFormattingBox<'_>) -> bool {
    let Some((_, _, style, _)) = box_.element_parts() else {
        return false;
    };
    style.display.is_run_in() && !is_out_of_flow_box(box_)
}

fn inlinified_run_in_box(mut box_: MutableFormattingBox<'_>) -> MutableFormattingBox<'_> {
    inlinify_run_in_box(&mut box_);
    box_
}

fn inlinify_run_in_box(box_: &mut MutableFormattingBox<'_>) {
    match box_ {
        MutableFormattingBox::Block(box_) => {
            box_.style.display = box_.style.display.run_in_inlinified();
            for child in &mut box_.run_in_children {
                inlinify_run_in_box(child);
            }
            inlinify_run_in_children(&mut box_.children);
        }
        MutableFormattingBox::Inline(box_) => {
            box_.style.display = box_.style.display.run_in_inlinified();
            inlinify_run_in_children(&mut box_.children);
        }
        MutableFormattingBox::InlineSplitBlockContext(box_) => {
            for child in &mut box_.children {
                inlinify_run_in_box(child);
            }
        }
        MutableFormattingBox::AtomicInline(box_) => {
            box_.style.display = box_.style.display.run_in_inlinified();
            inlinify_run_in_children(&mut box_.children);
        }
        MutableFormattingBox::Table(box_) => {
            box_.style.display = box_.style.display.run_in_inlinified();
        }
        MutableFormattingBox::Flex(box_) => {
            box_.style.display = box_.style.display.run_in_inlinified();
        }
        MutableFormattingBox::Replaced(box_) => {
            box_.style.display = box_.style.display.run_in_inlinified();
        }
        MutableFormattingBox::AnonymousBlock(_) | MutableFormattingBox::Text(_) => {}
    }
}

fn inlinify_run_in_children(children: &mut Vec<MutableFormattingBox<'_>>) {
    let original = std::mem::take(children);
    for child in original {
        for part in split_block_in_inline_child(child) {
            if is_in_flow_block_level_box(&part) {
                children.push(inlinified_block_descendant(part));
            } else {
                children.push(part);
            }
        }
    }
}

fn inlinified_block_descendant(box_: MutableFormattingBox<'_>) -> MutableFormattingBox<'_> {
    match box_ {
        MutableFormattingBox::Block(mut box_) => {
            box_.style.display =
                Display::INLINE_BLOCK.with_list_item(box_.style.display.is_list_item());
            MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
                element: box_.element,
                signature: box_.signature,
                source: box_.source,
                style: box_.style,
                marker: box_.marker,
                children: box_.children,
                table_fragment: None,
            })
        }
        box_ => box_,
    }
}

fn run_in_target_is_eligible(box_: &MutableFormattingBox<'_>) -> bool {
    if is_out_of_flow_box(box_) {
        return false;
    }
    let MutableFormattingBox::Block(box_) = box_ else {
        return false;
    };
    box_.style.display.is_block_level()
        && !box_.style.display.establishes_block_formatting_context()
}

fn insert_run_in_sequence<'a>(
    target: &mut MutableFormattingBox<'a>,
    mut sequence: Vec<MutableFormattingBox<'a>>,
) {
    if let Some(index) = first_eligible_run_in_descendant_index(target) {
        let MutableFormattingBox::Block(box_) = target else {
            return;
        };
        insert_run_in_sequence(&mut box_.children[index], sequence);
        return;
    }
    if let MutableFormattingBox::Block(box_) = target {
        sequence.append(&mut box_.run_in_children);
        box_.run_in_children = sequence;
    }
}

fn first_eligible_run_in_descendant_index(target: &MutableFormattingBox<'_>) -> Option<usize> {
    let MutableFormattingBox::Block(box_) = target else {
        return None;
    };
    box_.children
        .iter()
        .position(|child| !is_out_of_flow_box(child))
        .filter(|index| run_in_target_is_eligible(&box_.children[*index]))
}

/// Splits inline boxes that contain block-level descendants.
///
/// CSS 2.2 says an inline box containing an in-flow block-level box is split
/// around that block. The containing block then sees anonymous block boxes for
/// surrounding inline runs and the in-flow block participates as a sibling,
/// which allows normal block formatting and adjoining margin collapsing:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level> and
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>.
///
/// Absolutely positioned block-level descendants remain out-of-flow, but CSS
/// 2.2 computes their auto vertical static position from the hypothetical box
/// position they would have occupied in normal flow. A block-level positioned
/// child inside an inline therefore uses the same split point as an in-flow
/// block for static-position measurement:
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height>.
fn split_block_in_inline_children<'a>(
    children: Vec<MutableFormattingBox<'a>>,
) -> Vec<MutableFormattingBox<'a>> {
    children
        .into_iter()
        .flat_map(split_block_in_inline_child)
        .collect()
}

fn split_block_in_inline_child<'a>(
    child: MutableFormattingBox<'a>,
) -> Vec<MutableFormattingBox<'a>> {
    match child {
        MutableFormattingBox::Inline(box_) if inline_box_contains_block_split_boundary(&box_) => {
            split_inline_box_around_blocks(box_)
        }
        child => vec![child],
    }
}

fn split_inline_box_around_blocks<'a>(
    mut box_: MutableInlineBox<'a>,
) -> Vec<MutableFormattingBox<'a>> {
    let mut output = Vec::new();
    let mut inline_run = Vec::new();
    let mut saw_block = false;
    let children = std::mem::take(&mut box_.children);
    for child in children {
        for part in split_block_in_inline_child(child) {
            if is_block_in_inline_split_boundary(&part) {
                let fragment_edges = InlineBoxFragmentEdges {
                    owns_start: !saw_block && box_.fragment_edges.owns_start,
                    owns_end: false,
                };
                flush_split_inline_run(&mut output, &mut inline_run, &box_, fragment_edges);
                output.push(split_inline_block_context_or_part(&box_, part));
                saw_block = true;
            } else {
                inline_run.push(part);
            }
        }
    }
    let fragment_edges = InlineBoxFragmentEdges {
        owns_start: !saw_block && box_.fragment_edges.owns_start,
        owns_end: box_.fragment_edges.owns_end,
    };
    flush_split_inline_run(&mut output, &mut inline_run, &box_, fragment_edges);
    output
}

fn split_inline_block_context_or_part<'a>(
    box_: &MutableInlineBox<'a>,
    part: MutableFormattingBox<'a>,
) -> MutableFormattingBox<'a> {
    if !split_inline_style_needs_block_context(&box_.style) {
        return part;
    }
    MutableFormattingBox::InlineSplitBlockContext(MutableInlineSplitBlockContextBox {
        element: box_.element,
        signature: box_.signature.clone(),
        source: box_.source.clone(),
        style: box_.style.clone(),
        children: vec![part],
    })
}

fn split_inline_style_needs_block_context(style: &ComputedStyle) -> bool {
    matches!(style.position, Position::Relative | Position::Sticky)
        || style.z_index.is_some()
        || style.opacity < 1.0
        || !style.transform.is_empty()
        || style.isolation == Isolation::Isolate
        || style.mix_blend_mode != MixBlendMode::Normal
        || !matches!(style.filter, FilterValue::None)
        || style.clip_path != ClipPath::None
        || style.mask != MaskValue::None
        || style.contain.paint
        || matches!(
            style.content_visibility,
            ContentVisibility::Auto | ContentVisibility::Hidden
        )
        || style.will_change.opacity
        || style.will_change.transform
        || style.will_change.filter
        || style.will_change.clip_path
        || style.will_change.mask
        || style.will_change.mix_blend_mode
        || style.will_change.isolation
        || style.will_change.contain
}

fn flush_split_inline_run<'a>(
    output: &mut Vec<MutableFormattingBox<'a>>,
    inline_run: &mut Vec<MutableFormattingBox<'a>>,
    box_: &MutableInlineBox<'a>,
    fragment_edges: InlineBoxFragmentEdges,
) {
    trim_split_inline_run_edges(inline_run);
    if inline_run.is_empty() && !split_inline_fragment_has_owned_edge(&box_.style, fragment_edges) {
        return;
    }
    output.push(MutableFormattingBox::Inline(MutableInlineBox {
        element: box_.element,
        signature: box_.signature.clone(),
        source: box_.source.clone(),
        style: box_.style.clone(),
        marker: box_.marker.clone(),
        fragment_edges,
        children: std::mem::take(inline_run),
    }));
}

fn split_inline_fragment_has_owned_edge(
    style: &ComputedStyle,
    fragment_edges: InlineBoxFragmentEdges,
) -> bool {
    (fragment_edges.owns_start
        && inline_box_edge_has_nonzero_component(style, InlineBoxEdge::Start))
        || (fragment_edges.owns_end
            && inline_box_edge_has_nonzero_component(style, InlineBoxEdge::End))
}

#[derive(Debug, Clone, Copy)]
enum InlineBoxEdge {
    Start,
    End,
}

fn inline_box_edge_has_nonzero_component(style: &ComputedStyle, edge: InlineBoxEdge) -> bool {
    let borders = used_border_widths(style);
    let (margin, border, padding) = match (style.direction, edge) {
        (Direction::Ltr, InlineBoxEdge::Start) => {
            (style.margin.left, borders.left, style.padding.left)
        }
        (Direction::Ltr, InlineBoxEdge::End) => {
            (style.margin.right, borders.right, style.padding.right)
        }
        (Direction::Rtl, InlineBoxEdge::Start) => {
            (style.margin.right, borders.right, style.padding.right)
        }
        (Direction::Rtl, InlineBoxEdge::End) => {
            (style.margin.left, borders.left, style.padding.left)
        }
    };
    margin.abs() > 0.001 || border.abs() > 0.001 || padding.abs() > 0.001
}

/// Trim collapsible whitespace exposed by splitting an inline around a block.
///
/// CSS 2.2 splits inline boxes containing in-flow block boxes into separate
/// inline fragments around the block, and CSS Text removes collapsible
/// whitespace at line boundaries. The split creates new anonymous block
/// boundaries, so indentation whitespace next to the block must not contribute
/// an extra line-box advance:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level> and
/// <https://www.w3.org/TR/css-text-3/#white-space-phase-1>.
fn trim_split_inline_run_edges(inline_run: &mut Vec<MutableFormattingBox<'_>>) {
    trim_split_inline_run_start(inline_run);
    trim_split_inline_run_end(inline_run);
}

fn trim_split_inline_run_start(inline_run: &mut Vec<MutableFormattingBox<'_>>) {
    loop {
        if inline_run.first().is_some_and(formatting_box_is_empty_text) {
            inline_run.remove(0);
            continue;
        }
        let Some(first) = inline_run.first_mut() else {
            return;
        };
        let removed = trim_formatting_box_start_collapsible_whitespace(first);
        if removed || formatting_box_is_empty_text(first) {
            inline_run.remove(0);
            continue;
        }
        return;
    }
}

fn trim_split_inline_run_end(inline_run: &mut Vec<MutableFormattingBox<'_>>) {
    loop {
        if inline_run.last().is_some_and(formatting_box_is_empty_text) {
            inline_run.pop();
            continue;
        }
        let Some(last) = inline_run.last_mut() else {
            return;
        };
        let removed = trim_formatting_box_end_collapsible_whitespace(last);
        if removed || formatting_box_is_empty_text(last) {
            inline_run.pop();
            continue;
        }
        return;
    }
}

fn trim_formatting_box_start_collapsible_whitespace(box_: &mut MutableFormattingBox<'_>) -> bool {
    match box_ {
        MutableFormattingBox::Text(text) if text.style.white_space.collapses_spaces() => {
            text.text = trim_start_css_collapsible_whitespace(&text.text).to_string();
            text.text.is_empty()
        }
        MutableFormattingBox::Inline(box_) => {
            trim_split_inline_run_start(&mut box_.children);
            box_.children.is_empty() && split_trim_empty_inline_box_is_discardable(box_)
        }
        _ => false,
    }
}

fn trim_formatting_box_end_collapsible_whitespace(box_: &mut MutableFormattingBox<'_>) -> bool {
    match box_ {
        MutableFormattingBox::Text(text) if text.style.white_space.collapses_spaces() => {
            text.text = trim_end_css_collapsible_whitespace(&text.text).to_string();
            text.text.is_empty()
        }
        MutableFormattingBox::Inline(box_) => {
            trim_split_inline_run_end(&mut box_.children);
            box_.children.is_empty() && split_trim_empty_inline_box_is_discardable(box_)
        }
        _ => false,
    }
}

/// Return whether an empty inline box at a split boundary can be discarded.
///
/// CSS 2.2 block-in-inline splitting exposes new anonymous block boundaries,
/// where CSS Text trimming can remove collapsible whitespace. Empty inline
/// boxes are only ignorable if they have no owned edge decoration and cannot
/// produce generated content. This preserves HTML `br` rendering through the
/// UA `br::before` generated newline while still allowing author
/// `br::before { content: none }` to suppress the break:
/// <https://www.w3.org/TR/CSS22/visuren.html#anonymous-block-level>,
/// <https://www.w3.org/TR/css-content-3/#content-property>, and
/// <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>.
fn split_trim_empty_inline_box_is_discardable(box_: &MutableInlineBox<'_>) -> bool {
    !split_inline_fragment_has_owned_edge(&box_.style, box_.fragment_edges)
        && !box_.style.content.is_generated()
        && !pseudo_style_has_generated_content(box_.style.before_style.as_deref())
        && !pseudo_style_has_generated_content(box_.style.after_style.as_deref())
}

fn pseudo_style_has_generated_content(style: Option<&ComputedStyle>) -> bool {
    style.is_some_and(|style| style.content.is_generated())
}

fn formatting_box_is_empty_text(box_: &MutableFormattingBox<'_>) -> bool {
    matches!(box_, MutableFormattingBox::Text(text) if text.text.is_empty())
}

fn inline_box_contains_block_split_boundary(box_: &MutableInlineBox<'_>) -> bool {
    box_.children.iter().any(|child| match child {
        MutableFormattingBox::Inline(child) => inline_box_contains_block_split_boundary(child),
        child => is_block_in_inline_split_boundary(child),
    })
}

fn is_block_in_inline_split_boundary(box_: &MutableFormattingBox<'_>) -> bool {
    if !is_block_level_box(box_) {
        return false;
    }
    if is_floated_box(box_) {
        return false;
    }
    if !is_out_of_flow_box(box_) {
        return true;
    }
    let Some((_, _, style, _)) = box_.element_parts() else {
        return false;
    };
    !style.abspos_static_source_was_inline_level
}

fn is_in_flow_block_level_box(box_: &MutableFormattingBox<'_>) -> bool {
    is_block_level_box(box_) && !is_out_of_flow_box(box_)
}

pub(crate) fn normalize_orphan_table_internal_boxes<'a>(
    children: Vec<MutableFormattingBox<'a>>,
    parent_style: &ComputedStyle,
) -> Vec<MutableFormattingBox<'a>> {
    if parent_style.display.is_table()
        || parent_style.display.is_table_column_group()
        || parent_style.display.is_table_row_group()
        || parent_style.display.is_table_row()
    {
        return children;
    }

    let mut normalized = Vec::with_capacity(children.len());
    let mut table_run = Vec::new();
    for child in children {
        if is_table_internal_box(&child) {
            table_run.push(child);
        } else {
            flush_anonymous_table(&mut normalized, &mut table_run, parent_style);
            normalized.push(child);
        }
    }
    flush_anonymous_table(&mut normalized, &mut table_run, parent_style);
    normalized
}

pub(crate) fn flush_anonymous_table<'a>(
    normalized: &mut Vec<MutableFormattingBox<'a>>,
    table_run: &mut Vec<MutableFormattingBox<'a>>,
    parent_style: &ComputedStyle,
) {
    if table_run.is_empty() {
        return;
    }
    let children = std::mem::take(table_run);
    let Some((element, signature, _, _)) = children
        .iter()
        .find_map(MutableFormattingBox::element_parts)
    else {
        return;
    };
    let mut style = anonymous_table_style(parent_style);
    if parent_style.display.is_inline_level() {
        style.display = Display::INLINE_TABLE;
    }
    let fragment = build_table_fragment(element, signature, &children);
    // CSS 2.2 table model: internal table boxes that lack a table parent
    // generate an anonymous `table` wrapper, or an `inline-table` wrapper when
    // the missing parent is generated inside an inline box.
    if style.display.is_inline_level() {
        normalized.push(MutableFormattingBox::AtomicInline(MutableAtomicInlineBox {
            element,
            signature: signature.clone(),
            source: BoxSource::Principal,
            style: Box::new(style),
            marker: None,
            children,
            table_fragment: Some(fragment),
        }));
    } else {
        normalized.push(MutableFormattingBox::Table(MutableTableBox {
            element,
            signature: signature.clone(),
            source: BoxSource::Principal,
            style: Box::new(style),
            marker: None,
            children,
            fragment,
        }));
    }
}

pub(crate) fn anonymous_table_style(parent_style: &ComputedStyle) -> ComputedStyle {
    let mut style = css::default_style_for_tag("table");
    style.custom_properties = parent_style.custom_properties.clone();
    style.color = parent_style.color;
    style.text_align = parent_style.text_align;
    style.text_align_last = parent_style.text_align_last;
    style.text_justify = parent_style.text_justify;
    style.font_style = parent_style.font_style;
    style.font_width = parent_style.font_width;
    style.text_decoration = parent_style.text_decoration;
    style.font_family = parent_style.font_family.clone();
    style.font_feature_settings = parent_style.font_feature_settings.clone();
    style.font_kerning = parent_style.font_kerning;
    style.font_variant_ligatures = parent_style.font_variant_ligatures;
    style.font_variant_position = parent_style.font_variant_position;
    style.font_variant_caps = parent_style.font_variant_caps;
    style.font_variant_numeric = parent_style.font_variant_numeric.clone();
    style.font_variant_alternates = parent_style.font_variant_alternates.clone();
    style.font_variant_east_asian = parent_style.font_variant_east_asian.clone();
    style.font_variant_emoji = parent_style.font_variant_emoji;
    style.language = parent_style.language.clone();
    style.line_height_value = parent_style.line_height_value;
    style.line_height_multiplier = parent_style.line_height_multiplier;
    style.line_height_is_normal = parent_style.line_height_is_normal;
    style.word_spacing = parent_style.word_spacing;
    style.text_transform = parent_style.text_transform;
    style.tab_size = parent_style.tab_size;
    style.white_space = parent_style.white_space;
    style.word_break = parent_style.word_break;
    style.overflow_wrap = parent_style.overflow_wrap;
    style.line_break = parent_style.line_break;
    style.hyphens = parent_style.hyphens;
    style.hyphenate_limit_chars = parent_style.hyphenate_limit_chars;
    style.visibility = parent_style.visibility;
    style.list_style_type = parent_style.list_style_type.clone();
    style.list_style_position = parent_style.list_style_position;
    style.list_style_image = parent_style.list_style_image.clone();
    style.list_style_image_base_url = parent_style.list_style_image_base_url.clone();
    style.list_style_image_root_url = parent_style.list_style_image_root_url.clone();
    style.font_size = parent_style.font_size;
    style.font_size_adjust = parent_style.font_size_adjust;
    style.line_height = parent_style.line_height;
    style.font_weight = parent_style.font_weight;
    style.border_collapse = parent_style.border_collapse;
    style.caption_side = parent_style.caption_side;
    style.empty_cells = parent_style.empty_cells;
    style.border_spacing = parent_style.border_spacing;
    style.border_spacing_explicit = parent_style.border_spacing_explicit;
    style
}

pub(crate) fn is_table_internal_box(box_: &MutableFormattingBox<'_>) -> bool {
    let Some((_, _, style, _)) = box_.element_parts() else {
        return false;
    };
    style.display.is_table_caption()
        || style.display.is_table_column_group()
        || style.display.is_table_column()
        || style.display.is_table_row_group()
        || style.display.is_table_row()
        || style.display.is_table_cell()
}

pub(crate) fn flush_anonymous_block<'a>(
    normalized: &mut Vec<MutableFormattingBox<'a>>,
    inline_run: &mut Vec<MutableFormattingBox<'a>>,
    parent_style: &ComputedStyle,
) {
    trim_split_inline_run_edges(inline_run);
    if inline_run.is_empty() {
        return;
    }
    if inline_run.iter().all(formatting_box_is_collapsible_space) {
        inline_run.clear();
        return;
    }
    normalized.push(MutableFormattingBox::AnonymousBlock(
        MutableAnonymousBlockBox {
            style: Box::new(parent_style.clone()),
            children: std::mem::take(inline_run),
        },
    ));
}

pub(crate) fn formatting_box_is_collapsible_space<S>(box_: &FormattingBoxWith<'_, S>) -> bool
where
    S: AsRef<ComputedStyle>,
{
    matches!(box_, FormattingBoxWith::Text(text) if text_is_css_collapsible_space(&text.text, text.style.as_ref()))
}

pub(crate) fn is_inline_level_box<S>(box_: &FormattingBoxWith<'_, S>) -> bool
where
    S: AsRef<ComputedStyle>,
{
    match box_ {
        FormattingBoxWith::Inline(_) | FormattingBoxWith::Text(_) => true,
        FormattingBoxWith::AtomicInline(box_) => {
            box_.style.as_ref().display.is_atomic_inline()
                || box_.style.as_ref().display.is_replaced()
        }
        FormattingBoxWith::Block(_)
        | FormattingBoxWith::InlineSplitBlockContext(_)
        | FormattingBoxWith::AnonymousBlock(_)
        | FormattingBoxWith::Table(_)
        | FormattingBoxWith::Flex(_)
        | FormattingBoxWith::Replaced(_) => false,
    }
}

pub(crate) fn is_block_level_box<S>(box_: &FormattingBoxWith<'_, S>) -> bool
where
    S: AsRef<ComputedStyle>,
{
    match box_ {
        FormattingBoxWith::Block(_)
        | FormattingBoxWith::InlineSplitBlockContext(_)
        | FormattingBoxWith::AnonymousBlock(_)
        | FormattingBoxWith::Table(_)
        | FormattingBoxWith::Flex(_) => true,
        FormattingBoxWith::Replaced(box_) => box_.style.as_ref().display.is_block_level(),
        FormattingBoxWith::Inline(_)
        | FormattingBoxWith::AtomicInline(_)
        | FormattingBoxWith::Text(_) => false,
    }
}

/// Returns whether a formatting box is a CSS float.
///
/// CSS 2.2 removes floats from normal block flow, but a float generated in an
/// inline run is still positioned relative to the current line and then
/// recorded as a float exclusion:
/// <https://www.w3.org/TR/CSS22/visuren.html#floats>.
pub(crate) fn is_floated_box<S>(box_: &FormattingBoxWith<'_, S>) -> bool
where
    S: AsRef<ComputedStyle>,
{
    let Some((_, _, style, _)) = box_.element_parts() else {
        return false;
    };
    style.float != Float::None
}

/// Returns whether a formatting box is removed from normal flow.
///
/// CSS Positioned Layout makes absolutely positioned and fixed positioned
/// boxes out-of-flow. Such boxes still generate boxes and positioned paint,
/// but they do not participate in normal-flow block/inline normalization:
/// <https://www.w3.org/TR/css-position-3/#absolute-positioning>.
pub(crate) fn is_out_of_flow_box<S>(box_: &FormattingBoxWith<'_, S>) -> bool
where
    S: AsRef<ComputedStyle>,
{
    let Some((_, _, style, _)) = box_.element_parts() else {
        return false;
    };
    matches!(style.position, Position::Absolute | Position::Fixed)
}
