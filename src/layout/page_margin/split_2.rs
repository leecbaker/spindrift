use super::*;

#[derive(Clone, Copy)]
pub(in crate::layout) struct PageMarginPaintContext<'a> {
    pub(in crate::layout) page_margins: PageMargins,
    pub(in crate::layout) page_edges: PageBoxEdges,
    pub(in crate::layout) page_number: usize,
    pub(in crate::layout) total_pages: usize,
    pub(in crate::layout) base_url: Option<&'a std::path::Path>,
    pub(in crate::layout) root_url: Option<&'a std::path::Path>,
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
    page: &Page,
    boxes: &'a [PageMarginBoxSpec],
    context: PageMarginPaintContext<'_>,
    font_system: &mut FontSystem,
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
        "top-left-corner",
        0.0,
        page.height() - margin_top,
        margin_left,
        margin_top,
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        "top-right-corner",
        page.width() - margin_right,
        page.height() - margin_top,
        margin_right,
        margin_top,
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        "bottom-right-corner",
        page.width() - margin_right,
        0.0,
        margin_right,
        margin_bottom,
    );
    push_corner_margin_box_layout(
        &mut layouts,
        &generated,
        "bottom-left-corner",
        0.0,
        0.0,
        margin_left,
        margin_bottom,
    );

    push_horizontal_margin_box_group(
        &mut layouts,
        &mut generated,
        font_system,
        ["top-left", "top-center", "top-right"],
        HorizontalMarginGroupGeometry {
            x: margin_left,
            y: page.height() - margin_top,
            available_width,
            row_height: margin_top,
            side: HorizontalPageMarginSide::Top,
        },
        context,
    );
    push_horizontal_margin_box_group(
        &mut layouts,
        &mut generated,
        font_system,
        ["bottom-left", "bottom-center", "bottom-right"],
        HorizontalMarginGroupGeometry {
            x: margin_left,
            y: 0.0,
            available_width,
            row_height: margin_bottom,
            side: HorizontalPageMarginSide::Bottom,
        },
        context,
    );
    push_vertical_margin_box_group(
        &mut layouts,
        &mut generated,
        font_system,
        ["left-top", "left-middle", "left-bottom"],
        VerticalMarginGroupGeometry {
            x: 0.0,
            y: margin_bottom,
            column_width: margin_left,
            available_height,
            side: VerticalPageMarginSide::Left,
        },
        context,
    );
    push_vertical_margin_box_group(
        &mut layouts,
        &mut generated,
        font_system,
        ["right-top", "right-middle", "right-bottom"],
        VerticalMarginGroupGeometry {
            x: page.width() - margin_right,
            y: margin_bottom,
            column_width: margin_right,
            available_height,
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
            page_counters: context.page_counters,
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
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) available_width: f32,
    pub(in crate::layout) row_height: f32,
    pub(in crate::layout) side: HorizontalPageMarginSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum VerticalPageMarginSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct VerticalMarginGroupGeometry {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) column_width: f32,
    pub(in crate::layout) available_height: f32,
    pub(in crate::layout) side: VerticalPageMarginSide,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct MarginOuterRect {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
    pub(in crate::layout) edges: PageMarginBoxEdges,
}

pub(in crate::layout) fn push_corner_margin_box_layout<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    generated: &[GeneratedMarginBox<'a>],
    name: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) {
    let Some(box_) = generated.iter().find(|box_| box_.spec.name == name) else {
        return;
    };
    let horizontal = fixed_width_axis(box_.spec, width, height, corner_horizontal_side(name));
    let vertical = fixed_height_axis(box_.spec, height, width, corner_vertical_side(name));
    push_layout_from_outer_rect(
        layouts,
        box_,
        MarginOuterRect {
            x,
            y,
            width,
            height,
            edges: merge_fixed_axis_edges(horizontal, vertical),
        },
    );
}

pub(in crate::layout) fn push_horizontal_margin_box_group<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    generated: &mut [GeneratedMarginBox<'a>],
    font_system: &mut FontSystem,
    names: [&str; 3],
    geometry: HorizontalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) {
    let measures = names.map(|name| {
        generated
            .iter()
            .find(|box_| box_.spec.name == name)
            .map(|box_| {
                horizontal_margin_box_measure(box_, font_system, geometry.available_width, context)
            })
            .unwrap_or_else(PageMarginBoxMeasure::not_generated)
    });
    let widths = resolve_variable_outer_sizes(geometry.available_width, measures);
    for (index, name) in names.iter().enumerate() {
        let Some(box_) = generated.iter().find(|box_| box_.spec.name == *name) else {
            continue;
        };
        let outer_width = widths[index].max(0.0);
        let outer_x = match index {
            0 => geometry.x,
            1 => geometry.x + ((geometry.available_width - outer_width) / 2.0),
            _ => geometry.x + geometry.available_width - outer_width,
        };
        let vertical = fixed_height_axis(
            box_.spec,
            geometry.row_height,
            geometry.available_width,
            geometry.side,
        );
        push_layout_from_outer_rect(
            layouts,
            box_,
            MarginOuterRect {
                x: outer_x,
                y: geometry.y,
                width: outer_width,
                height: geometry.row_height,
                edges: vertical,
            },
        );
    }
}

pub(in crate::layout) fn push_vertical_margin_box_group<'a>(
    layouts: &mut Vec<PageMarginBoxLayout<'a>>,
    generated: &mut [GeneratedMarginBox<'a>],
    font_system: &mut FontSystem,
    names: [&str; 3],
    geometry: VerticalMarginGroupGeometry,
    context: PageMarginPaintContext<'_>,
) {
    let measures = names.map(|name| {
        generated
            .iter()
            .find(|box_| box_.spec.name == name)
            .map(|box_| {
                vertical_margin_box_measure(box_, font_system, geometry.available_height, context)
            })
            .unwrap_or_else(PageMarginBoxMeasure::not_generated)
    });
    let heights = resolve_variable_outer_sizes(geometry.available_height, measures);
    for (index, name) in names.iter().enumerate() {
        let Some(box_) = generated.iter().find(|box_| box_.spec.name == *name) else {
            continue;
        };
        let outer_height = heights[index].max(0.0);
        let outer_y = match index {
            0 => geometry.y + geometry.available_height - outer_height,
            1 => geometry.y + ((geometry.available_height - outer_height) / 2.0),
            _ => geometry.y,
        };
        let horizontal = fixed_width_axis(
            box_.spec,
            geometry.column_width,
            geometry.available_height,
            geometry.side,
        );
        push_layout_from_outer_rect(
            layouts,
            box_,
            MarginOuterRect {
                x: geometry.x,
                y: outer_y,
                width: geometry.column_width,
                height: outer_height,
                edges: horizontal,
            },
        );
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
    let border_x = outer.x + margin.left;
    let border_y = outer.y + margin.bottom;
    let border_width = (outer.width - margin.left - margin.right).max(0.0);
    let border_height = (outer.height - margin.top - margin.bottom).max(0.0);
    layouts.push(PageMarginBoxLayout {
        spec: box_.spec,
        content: box_.content.clone(),
        border_rect: paint_space_rect(border_x, border_y, border_width, border_height),
        content_rect: paint_space_rect(
            border_x + edges.border.left + padding.left,
            border_y + edges.border.bottom + padding.bottom,
            border_width - edges.border.left - edges.border.right - padding.left - padding.right,
            border_height - edges.border.top - edges.border.bottom - padding.top - padding.bottom,
        ),
    });
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PageMarginBoxEdges {
    pub(in crate::layout) margin: UsedEdges,
    pub(in crate::layout) border: css::Edges,
    pub(in crate::layout) padding: UsedEdges,
}

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
    horizontal_basis: f32,
    side: HorizontalPageMarginSide,
) -> PageMarginBoxEdges {
    let mut edges = used_margin_box_edges(box_, horizontal_basis, containing_height);
    let style = &box_.style;
    let padding = edges.padding.to_css_edges();
    let non_content = edges.border.top + edges.border.bottom + padding.top + padding.bottom;
    let content_height = used_content_height_or_auto(style, containing_height, non_content);
    let (top, bottom) = resolve_fixed_margin_axis(
        containing_height,
        non_content,
        content_height,
        style.box_values.margin.top,
        style.margin.top,
        style.box_values.margin.bottom,
        style.margin.bottom,
        containing_height,
        match side {
            HorizontalPageMarginSide::Top => FixedAxisAutoMargin::Start,
            HorizontalPageMarginSide::Bottom => FixedAxisAutoMargin::End,
        },
    );
    edges.margin.top = layout_pt(top);
    edges.margin.bottom = layout_pt(bottom);
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
    vertical_basis: f32,
    side: VerticalPageMarginSide,
) -> PageMarginBoxEdges {
    let mut edges = used_margin_box_edges(box_, containing_width, vertical_basis);
    let style = &box_.style;
    let padding = edges.padding.to_css_edges();
    let non_content = edges.border.left + edges.border.right + padding.left + padding.right;
    let content_width = used_content_width_or_auto(style, containing_width, non_content);
    let (left, right) = resolve_fixed_margin_axis(
        containing_width,
        non_content,
        content_width,
        style.box_values.margin.left,
        style.margin.left,
        style.box_values.margin.right,
        style.margin.right,
        containing_width,
        match side {
            VerticalPageMarginSide::Left => FixedAxisAutoMargin::Start,
            VerticalPageMarginSide::Right => FixedAxisAutoMargin::End,
        },
    );
    edges.margin.left = layout_pt(left);
    edges.margin.right = layout_pt(right);
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
    start_legacy: f32,
    end_margin: css::ComputedLengthPercentageOrAuto,
    end_legacy: f32,
    margin_basis: f32,
    overconstrained_auto: FixedAxisAutoMargin,
) -> (f32, f32) {
    let containing_size = containing_size.max(0.0);
    let non_content = non_content.max(0.0);
    let size_auto = content_size.is_none();
    let mut size = content_size.unwrap_or(0.0).max(0.0);
    let (mut start_auto, mut start) =
        fixed_axis_margin_component(start_margin, start_legacy, margin_basis);
    let (mut end_auto, mut end) = fixed_axis_margin_component(end_margin, end_legacy, margin_basis);

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

    if !size_auto && !start_auto && !end_auto {
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
    legacy_length: f32,
    basis: f32,
) -> (bool, f32) {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => (true, 0.0),
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => (
            false,
            if value.percent != 0.0 {
                used_length_percentage(value, basis)
            } else {
                legacy_length
            },
        ),
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => (false, legacy_length),
    }
}

pub(in crate::layout) fn used_margin_box_edges(
    box_: &PageMarginBoxSpec,
    horizontal_basis: f32,
    vertical_basis: f32,
) -> PageMarginBoxEdges {
    let style = &box_.style;
    let margin = style.box_values.margin;
    PageMarginBoxEdges {
        margin: UsedEdges {
            top: layout_pt(margin_edge_for_page_margin_box(
                margin.top,
                style.margin.top,
                vertical_basis,
            )),
            right: layout_pt(margin_edge_for_page_margin_box(
                margin.right,
                style.margin.right,
                horizontal_basis,
            )),
            bottom: layout_pt(margin_edge_for_page_margin_box(
                margin.bottom,
                style.margin.bottom,
                vertical_basis,
            )),
            left: layout_pt(margin_edge_for_page_margin_box(
                margin.left,
                style.margin.left,
                horizontal_basis,
            )),
        },
        border: used_border_widths(style),
        padding: UsedEdges {
            top: layout_pt(
                used_length_percentage(style.box_values.padding.top, vertical_basis).max(0.0),
            ),
            right: layout_pt(
                used_length_percentage(style.box_values.padding.right, horizontal_basis).max(0.0),
            ),
            bottom: layout_pt(
                used_length_percentage(style.box_values.padding.bottom, vertical_basis).max(0.0),
            ),
            left: layout_pt(
                used_length_percentage(style.box_values.padding.left, horizontal_basis).max(0.0),
            ),
        },
    }
}

pub(in crate::layout) fn margin_edge_for_page_margin_box(
    value: css::ComputedLengthPercentageOrAuto,
    legacy_length: f32,
    basis: f32,
) -> f32 {
    match value {
        css::ComputedLengthPercentageOrAuto::Auto => 0.0,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            if value.percent != 0.0 {
                used_length_percentage(value, basis)
            } else {
                legacy_length
            }
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::Stretch => legacy_length,
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

    pub(in crate::layout) fn clamp(self, value: f32) -> f32 {
        let mut value = value.max(0.0);
        if let Some(max) = self.max_constraint {
            value = value.min(max);
        }
        if let Some(min) = self.min_constraint {
            value = value.max(min);
        }
        value
    }
}

pub(in crate::layout) fn horizontal_margin_box_measure(
    box_: &GeneratedMarginBox<'_>,
    font_system: &mut FontSystem,
    available_width: f32,
    context: PageMarginPaintContext<'_>,
) -> PageMarginBoxMeasure {
    let style = &box_.spec.style;
    let edges = used_margin_box_edges(box_.spec, available_width, available_width);
    let margin = edges.margin.to_css_edges();
    let padding = edges.padding.to_css_edges();
    let non_content = margin.left
        + margin.right
        + edges.border.left
        + edges.border.right
        + padding.left
        + padding.right;
    let intrinsic_widths = margin_box_intrinsic_inline_sizes(
        font_system,
        &box_.content,
        style,
        available_width,
        context.base_url,
        context.root_url,
        context.resource_cache,
    );
    let specified_content = used_content_width_or_auto(style, available_width, non_content)
        .or_else(|| {
            intrinsic::intrinsic_width_keyword(
                style.box_values.width,
                intrinsic_widths.0,
                intrinsic_widths.1,
                available_width,
                non_content,
            )
        });
    PageMarginBoxMeasure {
        generated: true,
        specified_outer: specified_content.map(|width| width + non_content),
        min_outer: intrinsic_widths.0 + non_content,
        max_outer: intrinsic_widths.1 + non_content,
        min_constraint: used_min_width(style, available_width).map(|value| value + non_content),
        max_constraint: used_max_width(style, available_width).map(|value| value + non_content),
    }
}

pub(in crate::layout) fn vertical_margin_box_measure(
    box_: &GeneratedMarginBox<'_>,
    _font_system: &mut FontSystem,
    available_height: f32,
    context: PageMarginPaintContext<'_>,
) -> PageMarginBoxMeasure {
    let style = &box_.spec.style;
    let edges = used_margin_box_edges(box_.spec, available_height, available_height);
    let margin = edges.margin.to_css_edges();
    let padding = edges.padding.to_css_edges();
    let non_content = margin.top
        + margin.bottom
        + edges.border.top
        + edges.border.bottom
        + padding.top
        + padding.bottom;
    let content = page_margin_intrinsic_inline_items(
        &box_.content,
        style,
        available_height,
        context.base_url,
        context.root_url,
        context.resource_cache,
    );
    let line_count = content
        .iter()
        .filter(|item| matches!(item, InlineItem::Break(_)))
        .count()
        + 1;
    let atomic_height = content.iter().fold(style.line_height, |height, item| {
        height.max(match item {
            InlineItem::Atom(atom) => atom.height,
            InlineItem::Word(_)
            | InlineItem::Float(_)
            | InlineItem::Break(_)
            | InlineItem::PageScopeStart(_)
            | InlineItem::PageScopeEnd => style.line_height,
        })
    });
    let intrinsic = line_count as f32 * atomic_height;
    PageMarginBoxMeasure {
        generated: true,
        specified_outer: used_content_height_or_auto(style, available_height, non_content)
            .map(|height| height + non_content),
        min_outer: intrinsic + non_content,
        max_outer: intrinsic + non_content,
        min_constraint: used_min_height(style, available_height).map(|value| value + non_content),
        max_constraint: used_max_height(style, available_height).map(|value| value + non_content),
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
                let distributed = distribute_two_auto_sizes(
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
            if !measures[0].generated && !measures[2].generated {
                sizes[1] = available.max(0.0);
            } else {
                let side_max = measures[0].max_outer.max(measures[2].max_outer);
                let side_min = measures[0].min_outer.max(measures[2].min_outer);
                let center_proxy = measures[1];
                let side_proxy = PageMarginBoxMeasure {
                    generated: true,
                    specified_outer: None,
                    min_outer: side_min * 2.0,
                    max_outer: side_max * 2.0,
                    min_constraint: None,
                    max_constraint: None,
                };
                sizes[1] = distribute_two_auto_sizes(available, [center_proxy, side_proxy])[0];
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
    [
        measures[0].clamp(sizes[0]),
        measures[1].clamp(sizes[1]),
        measures[2].clamp(sizes[2]),
    ]
}
