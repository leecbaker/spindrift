use super::sizing::{fixed_height_axis_with_intrinsic, fixed_width_axis_with_intrinsic};
use super::*;
use crate::layout::page_generated::ResolvedPageContent;
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
