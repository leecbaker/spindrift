use super::*;

#[derive(Clone, Copy)]
pub(in crate::layout) struct PageMarginPaintContext<'a> {
    pub(in crate::layout) page_margins: PageMargins,
    pub(in crate::layout) page_edges: PageBoxEdges,
    pub(in crate::layout) page_number: usize,
    pub(in crate::layout) total_pages: usize,
    pub(in crate::layout) base_url: Option<&'a url::Url>,
    pub(in crate::layout) root_url: Option<&'a url::Url>,
    pub(in crate::layout) resource_cache: &'a ResourceCache,
    pub(in crate::layout) page_index: usize,
    pub(in crate::layout) page_named_strings: &'a [HashMap<String, Vec<NamedStringAssignment>>],
    pub(in crate::layout) page_running_elements:
        &'a [HashMap<String, Vec<NamedStringAssignment>>],
    pub(in crate::layout) page_anchors: &'a HashMap<String, usize>,
    pub(in crate::layout) page_anchor_text: &'a HashMap<String, AnchorText>,
    pub(in crate::layout) counter_styles: &'a HashMap<String, CounterStyleRule>,
    pub(in crate::layout) page_counters: &'a HashMap<String, i32>,
}

/// Computes used page-margin box rectangles for one generated page.
///
/// CSS Paged Media Level 3 defines sixteen margin boxes, generation from the
/// `content` property, coordinated variable dimensions for side triplets, and
/// fixed dimensions in the perpendicular axis:
/// <https://www.w3.org/TR/css-page-3/#margin-boxes> and
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>.
pub(in crate::layout) fn layout_page_margin_boxes<'a>(
    layout_builder: &mut LayoutBuilder<'_>,
    page: &Page,
    boxes: &'a [PageMarginBoxSpec],
    context: PageMarginPaintContext<'_>,
) -> Vec<PageMarginBoxLayout<'a>> {
    let mut generated = boxes
        .iter()
        .filter_map(|spec| {
            resolved_margin_box_content(spec, context)
                .map(|content| GeneratedMarginBox { spec, content })
        })
        .collect::<Vec<_>>();
    if generated.is_empty() {
        return Vec::new();
    }
    let page_margins = context.page_margins;
    let margin_top = page_margins.top();
    let margin_right = page_margins.right();
    let margin_bottom = page_margins.bottom();
    let margin_left = page_margins.left();
    let page_edges = context.page_edges;
    let page_area_width =
        (page.width() - margin_left - margin_right - page_edges.left() - page_edges.right())
            .max(0.0);
    let page_area_height =
        (page.height() - margin_top - margin_bottom - page_edges.top() - page_edges.bottom())
            .max(0.0);
    let available_width = page_area_width + page_edges.left() + page_edges.right();
    let available_height = page_area_height + page_edges.top() + page_edges.bottom();
    let mut layouts = Vec::new();

    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        layout_builder,
        context,
        "top-left-corner",
        paint_space_rect(0.0, page.height() - margin_top, margin_left, margin_top),
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        layout_builder,
        context,
        "top-right-corner",
        paint_space_rect(
            page.width() - margin_right,
            page.height() - margin_top,
            margin_right,
            margin_top,
        ),
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        layout_builder,
        context,
        "bottom-right-corner",
        paint_space_rect(
            page.width() - margin_right,
            0.0,
            margin_right,
            margin_bottom,
        ),
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        layout_builder,
        context,
        "bottom-left-corner",
        paint_space_rect(0.0, 0.0, margin_left, margin_bottom),
    );

    push_horizontal_margin_box_group(
        &mut layouts,
        &mut generated,
        layout_builder,
        ["top-left", "top-center", "top-right"],
        HorizontalMarginGroupGeometry {
            rect: paint_space_rect(
                margin_left,
                page.height() - margin_top,
                available_width,
                margin_top,
            ),
            side: HorizontalPageMarginSide::Top,
        },
        context,
    );
    push_horizontal_margin_box_group(
        &mut layouts,
        &mut generated,
        layout_builder,
        ["bottom-left", "bottom-center", "bottom-right"],
        HorizontalMarginGroupGeometry {
            rect: paint_space_rect(margin_left, 0.0, available_width, margin_bottom),
            side: HorizontalPageMarginSide::Bottom,
        },
        context,
    );
    push_vertical_margin_box_group(
        &mut layouts,
        &mut generated,
        layout_builder,
        ["left-top", "left-middle", "left-bottom"],
        VerticalMarginGroupGeometry {
            rect: paint_space_rect(0.0, margin_bottom, margin_left, available_height),
            side: VerticalPageMarginSide::Left,
        },
        context,
    );
    push_vertical_margin_box_group(
        &mut layouts,
        &mut generated,
        layout_builder,
        ["right-top", "right-middle", "right-bottom"],
        VerticalMarginGroupGeometry {
            rect: paint_space_rect(
                page.width() - margin_right,
                margin_bottom,
                margin_right,
                available_height,
            ),
            side: VerticalPageMarginSide::Right,
        },
        context,
    );

    layouts
}

pub(in crate::layout) struct GeneratedMarginBox<'a> {
    pub(in crate::layout) spec: &'a PageMarginBoxSpec,
    pub(in crate::layout) content: ResolvedPageContent,
}

pub(in crate::layout) fn resolved_margin_box_content(
    spec: &PageMarginBoxSpec,
    context: PageMarginPaintContext<'_>,
) -> Option<ResolvedPageContent> {
    let value = spec.declarations.get("content")?;
    let trimmed = css::trim_css_value(value);
    if trimmed.eq_ignore_ascii_case("normal") || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    let mut box_counters = context.page_counters.clone();
    apply_page_margin_box_counter_scope(&mut box_counters, &spec.style);
    resolve_page_content_parts(
        value,
        PageContentResolveContext {
            page_number: context.page_number,
            total_pages: context.total_pages,
            page_index: context.page_index,
            base_url: context.base_url,
            root_url: context.root_url,
            page_named_strings: context.page_named_strings,
            page_running_elements: context.page_running_elements,
            page_anchors: context.page_anchors,
            page_anchor_text: context.page_anchor_text,
            counter_styles: context.counter_styles,
            page_counters: &box_counters,
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum HorizontalPageMarginSide {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct HorizontalMarginGroupGeometry {
    /// Page-local outer area shared by the top or bottom margin-box triplet.
    pub(in crate::layout) rect: PaintRect,
    pub(in crate::layout) side: HorizontalPageMarginSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum VerticalPageMarginSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct VerticalMarginGroupGeometry {
    /// Page-local outer area shared by the left or right margin-box triplet.
    pub(in crate::layout) rect: PaintRect,
    pub(in crate::layout) side: VerticalPageMarginSide,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct MarginOuterRect {
    /// Outer margin box in page-local paint coordinates.
    pub(in crate::layout) rect: PaintRect,
    pub(in crate::layout) edges: PageMarginBoxEdges,
}

pub(in crate::layout) fn push_corner_margin_box_layout<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    generated: &[GeneratedMarginBox<'a>],
    layout_builder: &mut LayoutBuilder<'_>,
    context: PageMarginPaintContext<'_>,
    name: &str,
    rect: PaintRect,
) {
    let Some(box_) = generated.iter().find(|box_| box_.spec.name == name) else {
        return;
    };
    let inline_intrinsic = (box_.spec.style.writing_mode == WritingMode::HorizontalTb).then(|| {
        margin_box_intrinsic_inline_sizes(
            &mut layout_builder.font_system,
            &box_.content,
            &box_.spec.style,
            rect.width(),
            context.base_url,
            context.root_url,
            context.resource_cache,
        )
    });
    let horizontal = fixed_width_axis_with_intrinsic(
        box_.spec,
        rect.width(),
        PercentageBasis::definite(layout_pt(rect.height())),
        corner_horizontal_side(name),
        inline_intrinsic,
    );
    let horizontal_margin = horizontal.margin.to_css_edges();
    let horizontal_padding = horizontal.padding.to_css_edges();
    let content_width = (rect.width()
        - horizontal_margin.left
        - horizontal_margin.right
        - horizontal.border.left
        - horizontal.border.right
        - horizontal_padding.left
        - horizontal_padding.right)
        .max(0.0);
    let block_intrinsic = match box_.spec.style.writing_mode {
        WritingMode::HorizontalTb => layout_builder
            .page_margin_inline_sequence_with_replay(
                &box_.content,
                &box_.spec.style,
                content_width.max(1.0),
                rect.height().max(box_.spec.style.line_height),
                context,
            )
            .map(|sequence| {
                let block_size = sequence.total_height();
                (block_size, block_size)
            }),
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => Some(margin_box_intrinsic_inline_sizes(
            &mut layout_builder.font_system,
            &box_.content,
            &box_.spec.style,
            rect.height(),
            context.base_url,
            context.root_url,
            context.resource_cache,
        )),
    };
    let vertical = fixed_height_axis_with_intrinsic(
        box_.spec,
        rect.height(),
        PercentageBasis::definite(layout_pt(rect.width())),
        corner_vertical_side(name),
        block_intrinsic,
    );
    push_layout_from_outer_rect(
        layouts,
        box_,
        MarginOuterRect {
            rect,
            edges: merge_fixed_axis_edges(horizontal, vertical),
        },
    );
}

pub(in crate::layout) fn push_horizontal_margin_box_group<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    generated: &mut [GeneratedMarginBox<'a>],
    layout_builder: &mut LayoutBuilder<'_>,
    names: [&str; 3],
    geometry: HorizontalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) {
    let measures = names.map(|name| {
        generated
            .iter()
            .find(|box_| box_.spec.name == name)
            .map(|box_| horizontal_margin_box_measure(layout_builder, box_, geometry, context))
            .unwrap_or_else(PageMarginBoxMeasure::not_generated)
    });
    let widths = resolve_variable_outer_sizes(geometry.rect.width(), measures);
    for (index, name) in names.iter().enumerate() {
        let Some(box_) = generated.iter().find(|box_| box_.spec.name == *name) else {
            continue;
        };
        let outer_width = widths[index].max(0.0);
        let outer_x = match index {
            0 => geometry.rect.min_x(),
            1 => geometry.rect.min_x() + ((geometry.rect.width() - outer_width) / 2.0),
            _ => geometry.rect.max_x() - outer_width,
        };
        let intrinsic_content_height = horizontal_margin_box_fixed_block_intrinsic(
            layout_builder,
            box_,
            outer_width,
            geometry,
            context,
        );
        let vertical = fixed_height_axis_with_intrinsic(
            box_.spec,
            geometry.rect.height(),
            PercentageBasis::definite(layout_pt(geometry.rect.width())),
            geometry.side,
            intrinsic_content_height,
        );
        push_layout_from_outer_rect(
            layouts,
            box_,
            MarginOuterRect {
                rect: paint_space_rect(
                    outer_x,
                    geometry.rect.min_y(),
                    outer_width,
                    geometry.rect.height(),
                ),
                edges: vertical,
            },
        );
    }
}

/// Returns the laid-out logical block contribution for a horizontal-writing
/// top/bottom margin box with a resolved variable outer width.
///
/// CSS Page fixes the physical height of top/bottom boxes, but CSS Sizing
/// intrinsic `height` keywords select the generated content's block-size at
/// that resolved inline width. Reusing the final line sequence keeps the
/// sizing equation and painting line breaks identical.
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>
/// <https://www.w3.org/TR/css-sizing-3/#sizing-values>
fn horizontal_margin_box_fixed_block_intrinsic(
    layout_builder: &mut LayoutBuilder<'_>,
    box_: &GeneratedMarginBox<'_>,
    outer_width: f32,
    geometry: HorizontalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) -> Option<(f32, f32)> {
    match box_.spec.style.writing_mode {
        WritingMode::HorizontalTb => {
            let edges = used_margin_box_edges(
                box_.spec,
                PercentageBasis::definite(layout_pt(geometry.rect.width())),
                PercentageBasis::definite(layout_pt(geometry.rect.height())),
            );
            let margin = edges.margin.to_css_edges();
            let padding = edges.padding.to_css_edges();
            let inline_size = (outer_width
                - margin.left
                - margin.right
                - edges.border.left
                - edges.border.right
                - padding.left
                - padding.right)
                .max(0.0);
            layout_builder
                .page_margin_inline_sequence_with_replay(
                    &box_.content,
                    &box_.spec.style,
                    inline_size.max(1.0),
                    geometry.rect.height().max(box_.spec.style.line_height),
                    context,
                )
                .map(|sequence| {
                    let block_size = sequence.total_height();
                    (block_size, block_size)
                })
        }
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => Some(margin_box_intrinsic_inline_sizes(
            &mut layout_builder.font_system,
            &box_.content,
            &box_.spec.style,
            geometry.rect.height(),
            context.base_url,
            context.root_url,
            context.resource_cache,
        )),
    }
}

pub(in crate::layout) fn push_vertical_margin_box_group<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    generated: &mut [GeneratedMarginBox<'a>],
    layout_builder: &mut LayoutBuilder<'_>,
    names: [&str; 3],
    geometry: VerticalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) {
    let measures = names.map(|name| {
        generated
            .iter()
            .find(|box_| box_.spec.name == name)
            .map(|box_| vertical_margin_box_measure(layout_builder, box_, geometry, context))
            .unwrap_or_else(PageMarginBoxMeasure::not_generated)
    });
    let heights = resolve_variable_outer_sizes(geometry.rect.height(), measures);
    for (index, name) in names.iter().enumerate() {
        let Some(box_) = generated.iter().find(|box_| box_.spec.name == *name) else {
            continue;
        };
        let outer_height = heights[index].max(0.0);
        let outer_y = match index {
            0 => geometry.rect.max_y() - outer_height,
            1 => geometry.rect.min_y() + ((geometry.rect.height() - outer_height) / 2.0),
            _ => geometry.rect.min_y(),
        };
        let intrinsic_content_widths = vertical_margin_box_fixed_block_intrinsic(
            layout_builder,
            box_,
            outer_height,
            geometry,
            context,
        );
        let horizontal = fixed_width_axis_with_intrinsic(
            box_.spec,
            geometry.rect.width(),
            PercentageBasis::definite(layout_pt(geometry.rect.height())),
            geometry.side,
            intrinsic_content_widths,
        );
        push_layout_from_outer_rect(
            layouts,
            box_,
            MarginOuterRect {
                rect: paint_space_rect(
                    geometry.rect.min_x(),
                    outer_y,
                    geometry.rect.width(),
                    outer_height,
                ),
                edges: horizontal,
            },
        );
    }
}

/// Returns physical-width intrinsic sizes for a left/right margin box.
///
/// In vertical writing the physical width is the logical block axis, so its
/// content contribution is the final sequence's line-stack width. In
/// horizontal writing it is the normal inline intrinsic contribution.
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
fn vertical_margin_box_fixed_block_intrinsic(
    layout_builder: &mut LayoutBuilder<'_>,
    box_: &GeneratedMarginBox<'_>,
    outer_height: f32,
    geometry: VerticalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) -> Option<(f32, f32)> {
    match box_.spec.style.writing_mode {
        WritingMode::HorizontalTb => Some(margin_box_intrinsic_inline_sizes(
            &mut layout_builder.font_system,
            &box_.content,
            &box_.spec.style,
            geometry.rect.width(),
            context.base_url,
            context.root_url,
            context.resource_cache,
        )),
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => {
            let edges = used_margin_box_edges(
                box_.spec,
                PercentageBasis::definite(layout_pt(geometry.rect.width())),
                PercentageBasis::definite(layout_pt(outer_height)),
            );
            let margin = edges.margin.to_css_edges();
            let padding = edges.padding.to_css_edges();
            let inline_size = (outer_height
                - margin.top
                - margin.bottom
                - edges.border.top
                - edges.border.bottom
                - padding.top
                - padding.bottom)
                .max(0.0);
            layout_builder
                .page_margin_inline_sequence_with_replay(
                    &box_.content,
                    &box_.spec.style,
                    inline_size.max(1.0),
                    geometry.rect.width().max(box_.spec.style.line_height),
                    context,
                )
                .map(|sequence| {
                    let block_size = sequence.total_height();
                    (block_size, block_size)
                })
        }
    }
}

pub(in crate::layout) fn push_layout_from_outer_rect<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    box_: &GeneratedMarginBox<'a>,
    outer: MarginOuterRect,
) {
    let edges = outer.edges;
    let margin = edges.margin.to_css_edges();
    let padding = edges.padding.to_css_edges();
    let mut border_rect = inset_paint_rect(
        outer.rect,
        css::Edges {
            top: margin.top,
            right: margin.right,
            bottom: margin.bottom,
            left: margin.left,
        },
    );
    if let (Some(content_width), Some(side)) = (edges.fixed_content_width, edges.fixed_width_side) {
        let width =
            (content_width + edges.border.left + edges.border.right + padding.left + padding.right)
                .max(0.0);
        border_rect.origin.x = match side {
            VerticalPageMarginSide::Left => outer.rect.min_x() + margin.left,
            VerticalPageMarginSide::Right => outer.rect.max_x() - margin.right - width,
        };
        border_rect.size.width = width;
    }
    if let (Some(content_height), Some(side)) =
        (edges.fixed_content_height, edges.fixed_height_side)
    {
        let height = (content_height
            + edges.border.top
            + edges.border.bottom
            + padding.top
            + padding.bottom)
            .max(0.0);
        border_rect.origin.y = match side {
            HorizontalPageMarginSide::Top => outer.rect.max_y() - margin.top - height,
            HorizontalPageMarginSide::Bottom => outer.rect.min_y() + margin.bottom,
        };
        border_rect.size.height = height;
    }
    let content_rect = inset_paint_rect(
        border_rect,
        css::Edges {
            top: edges.border.top + padding.top,
            right: edges.border.right + padding.right,
            bottom: edges.border.bottom + padding.bottom,
            left: edges.border.left + padding.left,
        },
    );
    layouts.push(PageMarginBoxLayout {
        spec: box_.spec,
        content: box_.content.clone(),
        border_rect,
        content_rect,
    });
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginBoxEdges {
    pub(in crate::layout) margin: UsedEdges,
    pub(in crate::layout) border: css::Edges,
    pub(in crate::layout) padding: UsedEdges,
    pub(in crate::layout) fixed_content_width: Option<f32>,
    pub(in crate::layout) fixed_width_side: Option<VerticalPageMarginSide>,
    pub(in crate::layout) fixed_content_height: Option<f32>,
    pub(in crate::layout) fixed_height_side: Option<HorizontalPageMarginSide>,
}

type PageMarginPercentageBasis = PercentageBasis<LayoutLength>;

/// Resolves a page-margin box's fixed-height dimension.
///
/// CSS Paged Media Level 3 §5.3.3 gives top/bottom margin boxes a fixed
/// height equation over `margin-top`, borders, padding, `height`, and
/// `margin-bottom`; top boxes ignore `margin-top` when overconstrained, while
/// bottom boxes ignore `margin-bottom`:
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>.
pub(in crate::layout) fn fixed_height_axis(
    box_: &PageMarginBoxSpec,
    containing_height: f32,
    horizontal_basis: PageMarginPercentageBasis,
    side: HorizontalPageMarginSide,
) -> PageMarginBoxEdges {
    fixed_height_axis_with_intrinsic(box_, containing_height, horizontal_basis, side, None)
}

/// Resolves a fixed page-margin height with an optional laid-out content block
/// contribution for CSS Sizing intrinsic height keywords.
fn fixed_height_axis_with_intrinsic(
    box_: &PageMarginBoxSpec,
    containing_height: f32,
    horizontal_basis: PageMarginPercentageBasis,
    side: HorizontalPageMarginSide,
    intrinsic_content_heights: Option<(f32, f32)>,
) -> PageMarginBoxEdges {
    let mut edges = used_margin_box_edges(
        box_,
        horizontal_basis,
        PercentageBasis::definite(layout_pt(containing_height)),
    );
    let style = &box_.style;
    let padding = edges.padding.to_css_edges();
    let non_content = edges.border.top + edges.border.bottom + padding.top + padding.bottom;
    let content_height = used_content_box_height_or_auto(
        style,
        layout_pt(containing_height),
        non_content_pt(non_content),
    )
    .map(SemanticLengthExt::points)
    .or_else(|| {
        intrinsic_content_heights.and_then(|(min_content, max_content)| {
            intrinsic::intrinsic_content_box_width_keyword(
                style.box_values.height.clone(),
                content_box_pt(min_content),
                content_box_pt(max_content),
                layout_pt(containing_height),
                non_content_pt(non_content),
            )
            .map(SemanticLengthExt::points)
        })
    });
    let (top, bottom) = resolve_fixed_margin_axis(
        containing_height,
        non_content,
        content_height,
        style.box_values.margin.top.clone(),
        style.box_values.margin.bottom.clone(),
        // CSS Paged Media resolves fixed-axis percentages against the
        // corresponding page-margin dimension. This is distinct from the
        // CSS 2.2 block-margin percentage basis used by ordinary boxes.
        PercentageBasis::definite(layout_pt(containing_height)),
        match side {
            HorizontalPageMarginSide::Top => FixedAxisAutoMargin::Start,
            HorizontalPageMarginSide::Bottom => FixedAxisAutoMargin::End,
        },
    );
    edges.margin.top = layout_pt(top);
    edges.margin.bottom = layout_pt(bottom);
    edges.fixed_content_height = content_height;
    edges.fixed_height_side = Some(side);
    edges
}

/// Resolves a page-margin box's fixed-width dimension.
///
/// CSS Paged Media Level 3 §5.3.3 applies the same fixed-dimension equation to
/// left/right margin boxes with width and horizontal margins; left boxes ignore
/// `margin-left` when overconstrained, while right boxes ignore
/// `margin-right`:
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>.
pub(in crate::layout) fn fixed_width_axis(
    box_: &PageMarginBoxSpec,
    containing_width: f32,
    vertical_basis: PageMarginPercentageBasis,
    side: VerticalPageMarginSide,
) -> PageMarginBoxEdges {
    fixed_width_axis_with_intrinsic(box_, containing_width, vertical_basis, side, None)
}

/// Resolves a fixed page-margin width with optional min/max inline content
/// contributions for CSS Sizing intrinsic width keywords.
fn fixed_width_axis_with_intrinsic(
    box_: &PageMarginBoxSpec,
    containing_width: f32,
    vertical_basis: PageMarginPercentageBasis,
    side: VerticalPageMarginSide,
    intrinsic_content_widths: Option<(f32, f32)>,
) -> PageMarginBoxEdges {
    let mut edges = used_margin_box_edges(
        box_,
        PercentageBasis::definite(layout_pt(containing_width)),
        vertical_basis,
    );
    let style = &box_.style;
    let padding = edges.padding.to_css_edges();
    let non_content = edges.border.left + edges.border.right + padding.left + padding.right;
    let content_width = used_content_box_width_or_auto(
        style,
        layout_pt(containing_width),
        non_content_pt(non_content),
    )
    .map(SemanticLengthExt::points)
    .or_else(|| {
        intrinsic_content_widths.and_then(|(min_content, max_content)| {
            intrinsic::intrinsic_content_box_width_keyword(
                style.box_values.width.clone(),
                content_box_pt(min_content),
                content_box_pt(max_content),
                layout_pt(containing_width),
                non_content_pt(non_content),
            )
            .map(SemanticLengthExt::points)
        })
    });
    let (left, right) = resolve_fixed_margin_axis(
        containing_width,
        non_content,
        content_width,
        style.box_values.margin.left.clone(),
        style.box_values.margin.right.clone(),
        // See the matching fixed-height calculation above: use the fixed
        // page-margin dimension rather than the orthogonal-axis basis.
        PercentageBasis::definite(layout_pt(containing_width)),
        match side {
            VerticalPageMarginSide::Left => FixedAxisAutoMargin::Start,
            VerticalPageMarginSide::Right => FixedAxisAutoMargin::End,
        },
    );
    edges.margin.left = layout_pt(left);
    edges.margin.right = layout_pt(right);
    edges.fixed_content_width = content_width;
    edges.fixed_width_side = Some(side);
    edges
}

pub(in crate::layout) fn corner_horizontal_side(name: &str) -> VerticalPageMarginSide {
    if name.contains("left") {
        VerticalPageMarginSide::Left
    } else {
        VerticalPageMarginSide::Right
    }
}

pub(in crate::layout) fn corner_vertical_side(name: &str) -> HorizontalPageMarginSide {
    if name.starts_with("top") {
        HorizontalPageMarginSide::Top
    } else {
        HorizontalPageMarginSide::Bottom
    }
}

pub(in crate::layout) fn merge_fixed_axis_edges(
    horizontal: PageMarginBoxEdges,
    vertical: PageMarginBoxEdges,
) -> PageMarginBoxEdges {
    PageMarginBoxEdges {
        margin: UsedEdges {
            top: vertical.margin.top,
            right: horizontal.margin.right,
            bottom: vertical.margin.bottom,
            left: horizontal.margin.left,
        },
        border: horizontal.border,
        padding: UsedEdges {
            top: vertical.padding.top,
            right: horizontal.padding.right,
            bottom: vertical.padding.bottom,
            left: horizontal.padding.left,
        },
        fixed_content_width: horizontal.fixed_content_width,
        fixed_width_side: horizontal.fixed_width_side,
        fixed_content_height: vertical.fixed_content_height,
        fixed_height_side: vertical.fixed_height_side,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum FixedAxisAutoMargin {
    Start,
    End,
}

/// Solves the fixed page-margin box axis equality.
///
/// CSS Paged Media Level 3 §5.3.3 defines a six-step used-value algorithm for
/// fixed dimensions. Auto margins share remaining space, auto sizes fill after
/// non-auto margins, and overconstrained explicit sizes can force the ignored
/// margin side negative to preserve the specified content size:
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>.
#[allow(clippy::too_many_arguments)]
pub(in crate::layout) fn resolve_fixed_margin_axis(
    containing_size: f32,
    non_content: f32,
    content_size: Option<f32>,
    start_margin: css::ComputedLengthPercentageOrAuto,
    end_margin: css::ComputedLengthPercentageOrAuto,
    margin_basis: PageMarginPercentageBasis,
    overconstrained_auto: FixedAxisAutoMargin,
) -> (f32, f32) {
    let containing_size = containing_size.max(0.0);
    let non_content = non_content.max(0.0);
    let size_auto = content_size.is_none();
    let mut size = content_size.unwrap_or(0.0).max(0.0);
    let (mut start_auto, mut start) = fixed_axis_margin_component(start_margin, margin_basis);
    let (mut end_auto, mut end) = fixed_axis_margin_component(end_margin, margin_basis);
    let outer_margin_was_auto = match overconstrained_auto {
        FixedAxisAutoMargin::Start => start_auto,
        FixedAxisAutoMargin::End => end_auto,
    };

    let specified_sum = non_content
        + if size_auto { 0.0 } else { size }
        + if start_auto { 0.0 } else { start }
        + if end_auto { 0.0 } else { end };
    if specified_sum > containing_size {
        if start_auto {
            start_auto = false;
            start = 0.0;
        }
        if end_auto {
            end_auto = false;
            end = 0.0;
        }
    }

    // The ignored outside margin is re-solved whenever it was explicitly
    // specified. Margins made zero by the preceding auto-margin clamp remain
    // zero when the outside margin itself was authored as `auto`; an authored
    // `auto` must not become a negative used margin.
    if !size_auto && !outer_margin_was_auto && !start_auto && !end_auto {
        match overconstrained_auto {
            FixedAxisAutoMargin::Start => {
                start_auto = true;
                start = 0.0;
            }
            FixedAxisAutoMargin::End => {
                end_auto = true;
                end = 0.0;
            }
        }
    }

    let auto_count = usize::from(size_auto) + usize::from(start_auto) + usize::from(end_auto);
    if auto_count == 1 {
        let remaining = containing_size
            - non_content
            - if size_auto { 0.0 } else { size }
            - if start_auto { 0.0 } else { start }
            - if end_auto { 0.0 } else { end };
        if size_auto {
            size = remaining.max(0.0);
        } else if start_auto {
            start = remaining;
            start_auto = false;
        } else {
            end = remaining;
            end_auto = false;
        }
    }

    if size_auto {
        if start_auto {
            start = 0.0;
            start_auto = false;
        }
        if end_auto {
            end = 0.0;
            end_auto = false;
        }
        size = (containing_size - non_content - start - end).max(0.0);
    }

    if start_auto && end_auto {
        let remaining = containing_size - non_content - size;
        start = remaining / 2.0;
        end = remaining / 2.0;
    }

    (start, end)
}

pub(in crate::layout) fn fixed_axis_margin_component(
    value: css::ComputedLengthPercentageOrAuto,
    basis: PageMarginPercentageBasis,
) -> (bool, f32) {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => (true, 0.0),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            (false, used_length_percentage(value, basis).points())
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => (false, 0.0),
    }
}

pub(in crate::layout) fn used_margin_box_edges(
    box_: &PageMarginBoxSpec,
    horizontal_basis: PageMarginPercentageBasis,
    vertical_basis: PageMarginPercentageBasis,
) -> PageMarginBoxEdges {
    let style = &box_.style;
    let margin = style.box_values.margin.clone();
    PageMarginBoxEdges {
        margin: UsedEdges {
            top: layout_pt(margin_edge_for_page_margin_box(margin.top, vertical_basis)),
            right: layout_pt(margin_edge_for_page_margin_box(
                margin.right,
                horizontal_basis,
            )),
            bottom: layout_pt(margin_edge_for_page_margin_box(
                margin.bottom,
                vertical_basis,
            )),
            left: layout_pt(margin_edge_for_page_margin_box(
                margin.left,
                horizontal_basis,
            )),
        },
        border: used_border_widths(style),
        padding: UsedEdges {
            top: layout_pt(
                used_length_percentage(style.box_values.padding.top.clone(), vertical_basis)
                    .points(),
            ),
            right: layout_pt(
                used_length_percentage(style.box_values.padding.right.clone(), horizontal_basis)
                    .points(),
            ),
            bottom: layout_pt(
                used_length_percentage(style.box_values.padding.bottom.clone(), vertical_basis)
                    .points(),
            ),
            left: layout_pt(
                used_length_percentage(style.box_values.padding.left.clone(), horizontal_basis)
                    .points(),
            ),
        },
        fixed_content_width: None,
        fixed_width_side: None,
        fixed_content_height: None,
        fixed_height_side: None,
    }
}

pub(in crate::layout) fn margin_edge_for_page_margin_box(
    value: css::ComputedLengthPercentageOrAuto,
    basis: PageMarginPercentageBasis,
) -> f32 {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => 0.0,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            used_length_percentage(value, basis).points()
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => 0.0,
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginBoxMeasure {
    pub(in crate::layout) generated: bool,
    pub(in crate::layout) specified_outer: Option<f32>,
    pub(in crate::layout) min_outer: f32,
    pub(in crate::layout) max_outer: f32,
    pub(in crate::layout) min_constraint: Option<f32>,
    pub(in crate::layout) max_constraint: Option<f32>,
}

impl PageMarginBoxMeasure {
    pub(in crate::layout) fn not_generated() -> Self {
        Self {
            generated: false,
            specified_outer: Some(0.0),
            min_outer: 0.0,
            max_outer: 0.0,
            min_constraint: Some(0.0),
            max_constraint: Some(0.0),
        }
    }

    pub(in crate::layout) fn auto_outer(self) -> bool {
        self.generated && self.specified_outer.is_none()
    }

    pub(in crate::layout) fn resolved_or_zero(self) -> f32 {
        if !self.generated {
            0.0
        } else {
            self.specified_outer.unwrap_or(0.0)
        }
    }

    /// Turn a min/max-saturated allocation into a definite outer size for
    /// the next CSS Page variable-dimension pass.
    pub(in crate::layout) fn with_definite_outer(self, outer: f32) -> Self {
        Self {
            specified_outer: Some(outer.max(0.0)),
            min_constraint: None,
            max_constraint: None,
            ..self
        }
    }
}

pub(in crate::layout) fn horizontal_margin_box_measure(
    layout_builder: &mut LayoutBuilder<'_>,
    box_: &GeneratedMarginBox<'_>,
    geometry: HorizontalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) -> PageMarginBoxMeasure {
    let style = &box_.spec.style;
    let available_width = geometry.rect.width();
    let edges = used_margin_box_edges(
        box_.spec,
        PercentageBasis::definite(layout_pt(available_width)),
        PercentageBasis::definite(layout_pt(available_width)),
    );
    let margin = edges.margin.to_css_edges();
    let padding = edges.padding.to_css_edges();
    let non_content = margin.left
        + margin.right
        + edges.border.left
        + edges.border.right
        + padding.left
        + padding.right;
    let intrinsic_widths = match style.writing_mode {
        WritingMode::HorizontalTb => margin_box_intrinsic_inline_sizes(
            &mut layout_builder.font_system,
            &box_.content,
            style,
            available_width,
            context.base_url,
            context.root_url,
            context.resource_cache,
        ),
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => {
            let fixed = fixed_height_axis(
                box_.spec,
                geometry.rect.height(),
                PercentageBasis::definite(layout_pt(geometry.rect.width())),
                geometry.side,
            );
            let fixed_margin = fixed.margin.to_css_edges();
            let fixed_edges = fixed.padding.to_css_edges();
            let inline_size = (geometry.rect.height()
                - fixed_margin.top
                - fixed_margin.bottom
                - fixed.border.top
                - fixed.border.bottom
                - fixed_edges.top
                - fixed_edges.bottom)
                .max(0.0);
            let block_size = layout_builder
                .page_margin_inline_sequence_with_replay(
                    &box_.content,
                    style,
                    inline_size.max(1.0),
                    available_width.max(style.line_height),
                    context,
                )
                .map(|sequence| sequence.total_height())
                .unwrap_or(0.0);
            (block_size, block_size)
        }
    };
    let specified_content = used_content_box_width_or_auto(
        style,
        layout_pt(available_width),
        non_content_pt(non_content),
    )
    .map(SemanticLengthExt::points)
    .or_else(|| {
        intrinsic::intrinsic_content_box_width_keyword(
            style.box_values.width.clone(),
            content_box_pt(intrinsic_widths.0),
            content_box_pt(intrinsic_widths.1),
            layout_pt(available_width),
            non_content_pt(non_content),
        )
        .map(SemanticLengthExt::points)
    });
    PageMarginBoxMeasure {
        generated: true,
        specified_outer: specified_content.map(|width| width + non_content),
        min_outer: intrinsic_widths.0 + non_content,
        max_outer: intrinsic_widths.1 + non_content,
        min_constraint: used_min_width(
            style,
            PercentageBasis::definite(layout_pt(available_width)),
        )
        .map(|value| value.points() + non_content),
        max_constraint: used_max_width(
            style,
            PercentageBasis::definite(layout_pt(available_width)),
        )
        .map(|value| value.points() + non_content),
    }
}

pub(in crate::layout) fn vertical_margin_box_measure(
    layout_builder: &mut LayoutBuilder<'_>,
    box_: &GeneratedMarginBox<'_>,
    geometry: VerticalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) -> PageMarginBoxMeasure {
    let style = &box_.spec.style;
    let available_height = geometry.rect.height();
    let edges = used_margin_box_edges(
        box_.spec,
        PercentageBasis::definite(layout_pt(available_height)),
        PercentageBasis::definite(layout_pt(available_height)),
    );
    let margin = edges.margin.to_css_edges();
    let padding = edges.padding.to_css_edges();
    let non_content = margin.top
        + margin.bottom
        + edges.border.top
        + edges.border.bottom
        + padding.top
        + padding.bottom;
    let (min_intrinsic, max_intrinsic) = match style.writing_mode {
        WritingMode::HorizontalTb => {
            let fixed = fixed_width_axis(
                box_.spec,
                geometry.rect.width(),
                PercentageBasis::definite(layout_pt(geometry.rect.height())),
                geometry.side,
            );
            let fixed_margin = fixed.margin.to_css_edges();
            let fixed_edges = fixed.padding.to_css_edges();
            let inline_size = (geometry.rect.width()
                - fixed_margin.left
                - fixed_margin.right
                - fixed.border.left
                - fixed.border.right
                - fixed_edges.left
                - fixed_edges.right)
                .max(0.0);
            let intrinsic = layout_builder
                .page_margin_inline_sequence_with_replay(
                    &box_.content,
                    style,
                    inline_size.max(1.0),
                    available_height.max(style.line_height),
                    context,
                )
                .map(|sequence| sequence.total_height())
                .unwrap_or(0.0);
            (intrinsic, intrinsic)
        }
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => {
            // The physical height of a left/right margin box maps to its
            // logical inline axis in vertical writing. Its variable-dimension
            // contribution is therefore min/max inline content, not the
            // number of physical horizontal lines.
            // https://www.w3.org/TR/css-page-3/#margin-dimension
            // https://www.w3.org/TR/css-writing-modes-4/#abstract-box
            margin_box_intrinsic_inline_sizes(
                &mut layout_builder.font_system,
                &box_.content,
                style,
                available_height,
                context.base_url,
                context.root_url,
                context.resource_cache,
            )
        }
    };
    PageMarginBoxMeasure {
        generated: true,
        specified_outer: used_content_box_height_or_auto(
            style,
            layout_pt(available_height),
            non_content_pt(non_content),
        )
        .map(|height| height.points() + non_content),
        min_outer: min_intrinsic + non_content,
        max_outer: max_intrinsic + non_content,
        min_constraint: used_min_height(
            style,
            PercentageBasis::definite(layout_pt(available_height)),
        )
        .map(|value| value.points() + non_content),
        max_constraint: used_max_height(
            style,
            PercentageBasis::definite(layout_pt(available_height)),
        )
        .map(|value| value.points() + non_content),
    }
}

/// Resolves the variable dimension for one three-box page-margin side.
///
/// CSS Paged Media Level 3 §5.3.2 coordinates top/bottom and left/right
/// triplets so the center box remains centered when generated, and otherwise
/// the side boxes share the available variable dimension.
pub(in crate::layout) fn resolve_variable_outer_sizes(
    available: f32,
    measures: [PageMarginBoxMeasure; 3],
) -> [f32; 3] {
    // CSS Page requires min/max violations to be made definite and the
    // allocation repeated.  Clamping each result independently is not
    // equivalent: it can make the three boxes exceed their available
    // dimension and, in particular, moves a centred box off its symmetric
    // imaginary-side solution.
    // https://www.w3.org/TR/css-page-3/#margin-dimension
    let mut saturated = measures;
    loop {
        let sizes = resolve_variable_outer_sizes_unconstrained(available, saturated);
        let Some((index, value)) = saturated.iter().enumerate().find_map(|(index, measure)| {
            measure
                .max_constraint
                .filter(|maximum| sizes[index] > *maximum)
                .map(|maximum| (index, maximum))
        }) else {
            break;
        };
        saturated[index] = saturated[index].with_definite_outer(value);
    }
    loop {
        let sizes = resolve_variable_outer_sizes_unconstrained(available, saturated);
        let Some((index, value)) = saturated.iter().enumerate().find_map(|(index, measure)| {
            measure
                .min_constraint
                .filter(|minimum| sizes[index] < *minimum)
                .map(|minimum| (index, minimum))
        }) else {
            return sizes;
        };
        saturated[index] = saturated[index].with_definite_outer(value);
    }
}

/// Allocate the variable axis once, before CSS min/max saturation.
///
/// Kept separate from [`resolve_variable_outer_sizes`] so each saturation
/// pass uses the same §5.3.2 algorithm with its newly definite box rather
/// than mixing independently-clamped candidates.
fn resolve_variable_outer_sizes_unconstrained(
    available: f32,
    measures: [PageMarginBoxMeasure; 3],
) -> [f32; 3] {
    let mut sizes = [
        measures[0].resolved_or_zero(),
        measures[1].resolved_or_zero(),
        measures[2].resolved_or_zero(),
    ];
    if !measures[1].generated {
        let fixed_sum = measures
            .iter()
            .filter(|measure| measure.generated && !measure.auto_outer())
            .map(|measure| measure.resolved_or_zero())
            .sum::<f32>();
        let auto_indexes = [0usize, 2usize]
            .into_iter()
            .filter(|index| measures[*index].auto_outer())
            .collect::<Vec<_>>();
        match auto_indexes.as_slice() {
            [index] => sizes[*index] = (available - fixed_sum).max(0.0),
            [left, right] => {
                let distributed = resolve_two_outer_sizes(
                    available - fixed_sum,
                    [measures[*left], measures[*right]],
                );
                sizes[*left] = distributed[0];
                sizes[*right] = distributed[1];
            }
            _ => {}
        }
    } else {
        if measures[1].auto_outer() {
            if measures.iter().all(|measure| {
                measure.generated
                    && measure.auto_outer()
                    && measure.min_outer == 0.0
                    && measure.max_outer == 0.0
            }) {
                // The three generated boxes have no intrinsic preference.
                // Keep the center box centered and distribute the available
                // size evenly, rather than first splitting a zero-sized
                // center/imaginary-side pair and then halving the remainder.
                // https://www.w3.org/TR/css-page-3/#margin-dimension
                sizes = [available / 3.0; 3];
            } else if !measures[0].generated && !measures[2].generated {
                sizes[1] = available.max(0.0);
            } else {
                let center_proxy = measures[1];
                // CSS Page evaluates the imaginary symmetric `AC` box once
                // for each real side, then uses the candidate occupying more
                // space. Taking the larger min-content value from one side
                // and the larger max-content value from the other constructs
                // an impossible hybrid box and under-sizes the center.
                // https://www.w3.org/TR/css-page-3/#margin-dimension
                let candidate = |side: PageMarginBoxMeasure| {
                    let side_outer = if side.auto_outer() {
                        PageMarginBoxMeasure {
                            generated: true,
                            specified_outer: None,
                            min_outer: side.min_outer * 2.0,
                            max_outer: side.max_outer * 2.0,
                            min_constraint: None,
                            max_constraint: None,
                        }
                    } else {
                        let outer = side.resolved_or_zero() * 2.0;
                        PageMarginBoxMeasure {
                            generated: true,
                            specified_outer: Some(outer),
                            min_outer: outer,
                            max_outer: outer,
                            min_constraint: None,
                            max_constraint: None,
                        }
                    };
                    let resolved = resolve_two_outer_sizes_with_constraints(
                        available,
                        [center_proxy, side_outer],
                    );
                    (resolved[0], resolved[1])
                };
                let left = measures[0].generated.then(|| candidate(measures[0]));
                let right = measures[2].generated.then(|| candidate(measures[2]));
                sizes[1] = match (left, right) {
                    (Some(left), Some(right)) if left.1 >= right.1 => left.0,
                    (Some(_), Some(right)) => right.0,
                    (Some(left), None) => left.0,
                    (None, Some(right)) => right.0,
                    (None, None) => available.max(0.0),
                };
            }
        }
        let remaining_side = ((available - sizes[1]).max(0.0)) / 2.0;
        if measures[0].auto_outer() {
            sizes[0] = remaining_side;
        }
        if measures[2].auto_outer() {
            sizes[2] = remaining_side;
        }
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(min_outer: f32, max_outer: f32) -> PageMarginBoxMeasure {
        PageMarginBoxMeasure {
            generated: true,
            specified_outer: None,
            min_outer,
            max_outer,
            min_constraint: None,
            max_constraint: None,
        }
    }

    #[test]
    fn variable_axis_reallocates_after_maximum_saturation() {
        let mut left = auto(10.0, 20.0);
        left.max_constraint = Some(15.0);
        let sizes = resolve_variable_outer_sizes(
            100.0,
            [
                left,
                PageMarginBoxMeasure::not_generated(),
                auto(10.0, 20.0),
            ],
        );

        assert_eq!(sizes, [15.0, 0.0, 85.0]);
    }

    #[test]
    fn variable_axis_keeps_an_auto_center_symmetric() {
        let sizes = resolve_variable_outer_sizes(
            180.0,
            [auto(20.0, 40.0), auto(10.0, 30.0), auto(40.0, 80.0)],
        );

        assert_eq!(sizes[0], sizes[2]);
        assert_eq!(sizes[1] + sizes[0] * 2.0, 180.0);
    }

    #[test]
    fn fixed_axis_centers_two_auto_margins_when_space_is_available() {
        let (start, end) = resolve_fixed_margin_axis(
            100.0,
            10.0,
            Some(20.0),
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::Auto,
            PercentageBasis::definite(layout_pt(100.0)),
            FixedAxisAutoMargin::Start,
        );

        assert_eq!((start, end), (35.0, 35.0));
    }

    #[test]
    fn fixed_axis_clamps_auto_margins_before_an_overflowing_auto_size() {
        let (start, end) = resolve_fixed_margin_axis(
            50.0,
            10.0,
            Some(50.0),
            css::ComputedLengthPercentageOrAuto::Auto,
            css::ComputedLengthPercentageOrAuto::Auto,
            PercentageBasis::definite(layout_pt(50.0)),
            FixedAxisAutoMargin::End,
        );

        assert_eq!((start, end), (0.0, 0.0));
    }

    #[test]
    fn fixed_axis_assigns_explicit_overconstraint_to_the_away_margin() {
        let length = |points| {
            css::ComputedLengthPercentageOrAuto::LengthPercentage(
                css::ComputedLengthPercentage::from_points(points),
            )
        };
        let (start, end) = resolve_fixed_margin_axis(
            100.0,
            10.0,
            Some(100.0),
            length(3.0),
            length(3.0),
            PercentageBasis::definite(layout_pt(100.0)),
            FixedAxisAutoMargin::Start,
        );

        assert_eq!((start, end), (-13.0, 3.0));
    }

    #[test]
    fn fixed_axis_reallocates_an_explicit_outside_margin_after_auto_clamping() {
        let start = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(67.5),
        );
        let (start, end) = resolve_fixed_margin_axis(
            75.0,
            4.5,
            Some(18.75),
            start,
            css::ComputedLengthPercentageOrAuto::Auto,
            PercentageBasis::definite(layout_pt(75.0)),
            FixedAxisAutoMargin::Start,
        );

        assert_eq!((start, end), (51.75, 0.0));
    }

    #[test]
    fn fixed_axis_percentages_use_the_margin_area_being_solved() {
        let half = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_percent(0.5),
        );
        let (outside, page_facing) = resolve_fixed_margin_axis(
            72.0,
            0.0,
            None,
            css::ComputedLengthPercentageOrAuto::Auto,
            half,
            PercentageBasis::definite(layout_pt(72.0)),
            FixedAxisAutoMargin::Start,
        );

        assert_eq!((outside, page_facing), (0.0, 36.0));
    }
}
