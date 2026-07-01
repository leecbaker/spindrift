use super::parse::parse_stylesheet;
use super::types::{
    ComputedLengthPercentage, ComputedLengthPercentageOrAuto, ComputedStyle, Css, Declarations,
    Direction, Edges, PhysicalSide, WritingMode, block_end_side, block_start_side, inline_end_side,
    inline_start_side,
};
use super::values::{
    fallback_ch_advance_for_style, parse_computed_length_percentage,
    parse_computed_length_percentage_auto, parse_computed_line_height, parse_font_size,
    parse_line_height, trim_css_value,
};
use crate::{PageMargins, PageSize, RenderOptions};

pub(crate) fn apply_stylesheet_options(css: &Css, options: &mut RenderOptions) {
    let stylesheet = parse_stylesheet(css);
    if stylesheet.page_declarations.get("size").is_some() {
        options.page_size = page_size_from(&stylesheet.page_declarations, options.page_size);
    }
    options.set_page_margins(page_margins_from_for_size(
        &stylesheet.page_declarations,
        options.page_margins(),
        options.page_size,
    ));

    for selector in ["body", "html", "p"] {
        for rule in stylesheet
            .rules
            .iter()
            .filter(|rule| rule.selector_text.eq_ignore_ascii_case(selector))
        {
            if let Some(value) = rule.declarations.get("font-size")
                && !font_size_value_depends_on_ch(value, options.font_size)
                && let Some(font_size) = parse_font_size(value, options.font_size)
            {
                options.font_size = font_size;
                options.line_height = font_size * 1.2;
            }
            if let Some(value) = rule.declarations.get("line-height")
                && !line_height_value_depends_on_ch(value, options.font_size)
                && let Some(line_height) =
                    parse_line_height(value, options.font_size).map(|item| item.0)
            {
                options.line_height = line_height;
            }
        }
    }
}

fn font_size_value_depends_on_ch(value: &str, parent_font_size: f32) -> bool {
    parse_computed_length_percentage(value, parent_font_size).is_some_and(|length| length.ch != 0.0)
}

fn line_height_value_depends_on_ch(value: &str, font_size: f32) -> bool {
    match parse_computed_line_height(value, font_size) {
        Some(super::types::ComputedLineHeight::Length(length)) => length.ch != 0.0,
        _ => false,
    }
}

pub(crate) fn page_size_from(declarations: &Declarations, base: PageSize) -> PageSize {
    let style = page_style_for_declarations(declarations);
    page_size_from_with_ch_advance(declarations, base, fallback_ch_advance_for_style(&style))
}

pub(crate) fn page_size_from_with_ch_advance(
    declarations: &Declarations,
    base: PageSize,
    ch_advance: f32,
) -> PageSize {
    let style = page_style_for_declarations(declarations);
    let mut page_size = base;
    if let Some(size) = declarations.get("size") {
        apply_page_size_to_with_metrics(size, &mut page_size, style.font_size, ch_advance);
        return page_size;
    }
    descriptor_page_size_from_width_height(declarations, ch_advance).unwrap_or(page_size)
}

/// Resolves a sheet size from `@page width`/`height` and fixed margins.
///
/// CSS Paged Media applies CSS box-model sizing to page boxes. When the
/// `size` descriptor is omitted, `width` and `height` describe the page
/// area/content box; fixed margins expand that to the sheet/page-box size:
/// <https://www.w3.org/TR/css-page-3/#page-properties> and
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
fn descriptor_page_size_from_width_height(
    declarations: &Declarations,
    ch_advance: f32,
) -> Option<PageSize> {
    let style = page_style_for_declarations(declarations);
    let width = declarations
        .get("width")
        .and_then(|value| parse_page_fixed_length(value, style.font_size, ch_advance))
        .filter(|value| *value >= 0.0)?;
    let height = declarations
        .get("height")
        .and_then(|value| parse_page_fixed_length(value, style.font_size, ch_advance))
        .filter(|value| *value >= 0.0)?;
    let margins =
        fixed_page_margin_lengths_without_size(declarations, style.font_size, ch_advance)?;
    Some(PageSize {
        width: width + margins.left + margins.right,
        height: height + margins.top + margins.bottom,
    })
}

fn fixed_page_margin_lengths_without_size(
    declarations: &Declarations,
    font_size: f32,
    ch_advance: f32,
) -> Option<PageMargins> {
    let mut margins = PageMarginEdges::from_lengths(PageMargins::all(0.0));
    for (name, value) in declarations {
        let value = trim_css_value(value);
        match name.as_str() {
            "margin" => margins = parse_page_margin_shorthand(value, font_size)?,
            "margin-top" => margins.top = parse_page_margin_edge(value, font_size)?,
            "margin-right" => margins.right = parse_page_margin_edge(value, font_size)?,
            "margin-bottom" => margins.bottom = parse_page_margin_edge(value, font_size)?,
            "margin-left" => margins.left = parse_page_margin_edge(value, font_size)?,
            _ => {}
        }
    }
    Some(PageMargins {
        top: fixed_page_margin_edge_length(margins.top, ch_advance)?,
        right: fixed_page_margin_edge_length(margins.right, ch_advance)?,
        bottom: fixed_page_margin_edge_length(margins.bottom, ch_advance)?,
        left: fixed_page_margin_edge_length(margins.left, ch_advance)?,
    })
}

fn fixed_page_margin_edge_length(edge: PageMarginEdge, ch_advance: f32) -> Option<f32> {
    match edge {
        PageMarginEdge::LengthPercentage(value) if value.percent == 0.0 => {
            Some(resolve_length_with_ch(value, ch_advance))
        }
        PageMarginEdge::LengthPercentage(_) | PageMarginEdge::Auto => None,
    }
}

/// Resolves the CSS Paged Media `page-orientation` descriptor to PDF rotation.
///
/// `page-orientation` rotates the page after layout. PDF page dictionaries
/// express this viewer rotation with `/Rotate` in clockwise degrees:
/// <https://www.w3.org/TR/css-page-3/#page-orientation-prop> and
/// ISO 32000-1:2008 §7.7.3.3.
pub(crate) fn page_rotation_from(declarations: &Declarations, base: i32) -> i32 {
    declarations
        .get("page-orientation")
        .and_then(|value| page_orientation_rotation(value))
        .unwrap_or(base)
}

/// Resolves CSS page padding against a concrete page box size.
///
/// CSS Paged Media applies padding to page boxes. Percentages on page margin
/// and padding properties are relative to the corresponding page-box
/// dimension, so top/bottom use page height and left/right use page width:
/// <https://www.w3.org/TR/css-page-3/#page-properties>.
#[cfg(test)]
pub(crate) fn page_padding_from_for_size(
    declarations: &Declarations,
    page_size: PageSize,
) -> Edges {
    let style = page_style_for_declarations(declarations);
    page_padding_from_for_size_with_ch_advance(
        declarations,
        page_size,
        fallback_ch_advance_for_style(&style),
    )
}

pub(crate) fn page_padding_from_for_size_with_ch_advance(
    declarations: &Declarations,
    page_size: PageSize,
    ch_advance: f32,
) -> Edges {
    if declarations.is_empty() {
        return Edges::ZERO;
    }
    let style = page_style_for_declarations(declarations);
    let mut padding = style.box_values.padding;
    for (name, value) in declarations {
        let value = trim_css_value(value);
        match name.as_str() {
            "padding" => {
                if let Some(parsed) = parse_page_padding_shorthand(value, style.font_size) {
                    padding = parsed;
                }
            }
            "padding-top" => {
                if let Some(edge) = parse_computed_length_percentage(value, style.font_size) {
                    padding.top = edge;
                }
            }
            "padding-right" => {
                if let Some(edge) = parse_computed_length_percentage(value, style.font_size) {
                    padding.right = edge;
                }
            }
            "padding-bottom" => {
                if let Some(edge) = parse_computed_length_percentage(value, style.font_size) {
                    padding.bottom = edge;
                }
            }
            "padding-left" => {
                if let Some(edge) = parse_computed_length_percentage(value, style.font_size) {
                    padding.left = edge;
                }
            }
            "padding-block" | "padding-inline" => {
                if let Some(edges) = parse_page_padding_axis(value, style.font_size)
                    && let Some([start, end]) =
                        logical_page_axis_sides(name, style.direction, style.writing_mode)
                {
                    set_page_padding_edge(&mut padding, start, edges[0]);
                    set_page_padding_edge(&mut padding, end, edges[1]);
                }
            }
            "padding-block-start"
            | "padding-block-end"
            | "padding-inline-start"
            | "padding-inline-end" => {
                if let Some(edge) = parse_computed_length_percentage(value, style.font_size)
                    && let Some(side) = logical_page_side(name, style.direction, style.writing_mode)
                {
                    set_page_padding_edge(&mut padding, side, edge);
                }
            }
            _ => {}
        }
    }
    Edges {
        top: resolve_page_length_percentage(padding.top, PhysicalSide::Top, page_size, ch_advance),
        right: resolve_page_length_percentage(
            padding.right,
            PhysicalSide::Right,
            page_size,
            ch_advance,
        ),
        bottom: resolve_page_length_percentage(
            padding.bottom,
            PhysicalSide::Bottom,
            page_size,
            ch_advance,
        ),
        left: resolve_page_length_percentage(
            padding.left,
            PhysicalSide::Left,
            page_size,
            ch_advance,
        ),
    }
}

fn page_orientation_rotation(value: &str) -> Option<i32> {
    match trim_css_value(value).to_ascii_lowercase().as_str() {
        "upright" => Some(0),
        "rotate-right" => Some(90),
        "rotate-left" => Some(270),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn page_margins_from(declarations: &Declarations, base: PageMargins) -> PageMargins {
    page_margins_from_for_size(declarations, base, PageSize::A4_POINTS)
}

/// Resolves CSS Paged Media page margins against a concrete page box size.
///
/// Page boxes use the CSS 2.2 block width/height equations in both axes:
/// `width`/`height`, `margin-*`, and `auto` margins determine the page area
/// inside the containing page sheet:
/// <https://www.w3.org/TR/css-page-3/#page-properties> and
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(crate) fn page_margins_from_for_size(
    declarations: &Declarations,
    base: PageMargins,
    page_size: PageSize,
) -> PageMargins {
    page_margins_from_for_size_and_edges(declarations, base, page_size, Edges::ZERO)
}

/// Resolves page margins with already-resolved page border and padding edges.
///
/// CSS Paged Media applies normal box-model properties to page boxes. When a
/// `width` or `height` descriptor gives the page area's content size, the CSS
/// 2.2 block-size equations subtract border and padding before resolving
/// `auto` margins:
/// <https://www.w3.org/TR/css-page-3/#page-properties> and
/// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
pub(crate) fn page_margins_from_for_size_and_edges(
    declarations: &Declarations,
    base: PageMargins,
    page_size: PageSize,
    non_margin_edges: Edges,
) -> PageMargins {
    let style = page_style_for_declarations(declarations);
    page_margins_from_for_size_and_edges_with_ch_advance(
        declarations,
        base,
        page_size,
        non_margin_edges,
        fallback_ch_advance_for_style(&style),
    )
}

pub(crate) fn page_margins_from_for_size_and_edges_with_ch_advance(
    declarations: &Declarations,
    base: PageMargins,
    page_size: PageSize,
    non_margin_edges: Edges,
    ch_advance: f32,
) -> PageMargins {
    let style = page_style_for_declarations(declarations);
    let mut margins = PageMarginEdges::from_lengths(base);
    let mut width = None;
    let mut height = None;
    let mut changed = false;
    for (name, value) in declarations {
        let value = trim_css_value(value);
        match name.as_str() {
            "margin" => {
                if let Some(parsed) = parse_page_margin_shorthand(value, style.font_size) {
                    margins = parsed;
                    changed = true;
                }
            }
            "margin-top" => {
                if let Some(edge) = parse_page_margin_edge(value, style.font_size) {
                    margins.top = edge;
                    changed = true;
                }
            }
            "margin-right" => {
                if let Some(edge) = parse_page_margin_edge(value, style.font_size) {
                    margins.right = edge;
                    changed = true;
                }
            }
            "margin-bottom" => {
                if let Some(edge) = parse_page_margin_edge(value, style.font_size) {
                    margins.bottom = edge;
                    changed = true;
                }
            }
            "margin-left" => {
                if let Some(edge) = parse_page_margin_edge(value, style.font_size) {
                    margins.left = edge;
                    changed = true;
                }
            }
            "margin-block" | "margin-inline" => {
                if let Some(edges) = parse_page_margin_axis(value, style.font_size)
                    && let Some([start, end]) =
                        logical_page_axis_sides(name, style.direction, style.writing_mode)
                {
                    set_page_margin_edge(&mut margins, start, edges[0]);
                    set_page_margin_edge(&mut margins, end, edges[1]);
                    changed = true;
                }
            }
            "margin-block-start"
            | "margin-block-end"
            | "margin-inline-start"
            | "margin-inline-end" => {
                if let Some(edge) = parse_page_margin_edge(value, style.font_size)
                    && let Some(side) = logical_page_side(name, style.direction, style.writing_mode)
                {
                    set_page_margin_edge(&mut margins, side, edge);
                    changed = true;
                }
            }
            "width" => {
                if let Some(length) = parse_page_fixed_length(value, style.font_size, ch_advance) {
                    width = Some(length);
                    changed = true;
                }
            }
            "height" => {
                if let Some(length) = parse_page_fixed_length(value, style.font_size, ch_advance) {
                    height = Some(length);
                    changed = true;
                }
            }
            _ => {}
        }
    }
    if changed {
        PageMargins {
            top: resolve_page_margin_axis(
                page_size.height,
                height,
                non_margin_edges.top + non_margin_edges.bottom,
                page_size.height,
                margins.top,
                margins.bottom,
                ch_advance,
            )
            .0,
            right: resolve_page_margin_axis(
                page_size.width,
                width,
                non_margin_edges.left + non_margin_edges.right,
                page_size.width,
                margins.left,
                margins.right,
                ch_advance,
            )
            .1,
            bottom: resolve_page_margin_axis(
                page_size.height,
                height,
                non_margin_edges.top + non_margin_edges.bottom,
                page_size.height,
                margins.top,
                margins.bottom,
                ch_advance,
            )
            .1,
            left: resolve_page_margin_axis(
                page_size.width,
                width,
                non_margin_edges.left + non_margin_edges.right,
                page_size.width,
                margins.left,
                margins.right,
                ch_advance,
            )
            .0,
        }
    } else {
        base
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PageMarginEdges {
    top: PageMarginEdge,
    right: PageMarginEdge,
    bottom: PageMarginEdge,
    left: PageMarginEdge,
}

impl PageMarginEdges {
    fn from_lengths(margins: PageMargins) -> Self {
        Self {
            top: PageMarginEdge::LengthPercentage(ComputedLengthPercentage::from_length(
                margins.top,
            )),
            right: PageMarginEdge::LengthPercentage(ComputedLengthPercentage::from_length(
                margins.right,
            )),
            bottom: PageMarginEdge::LengthPercentage(ComputedLengthPercentage::from_length(
                margins.bottom,
            )),
            left: PageMarginEdge::LengthPercentage(ComputedLengthPercentage::from_length(
                margins.left,
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PageMarginEdge {
    LengthPercentage(ComputedLengthPercentage),
    Auto,
}

pub(crate) fn page_style_for_declarations(declarations: &Declarations) -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    super::apply_declarations(&mut style, declarations);
    style
        .quotes
        .resolve_auto_language(style.language.as_deref());
    style
}

fn parse_page_margin_shorthand(value: &str, font_size: f32) -> Option<PageMarginEdges> {
    let values = value
        .split_whitespace()
        .filter_map(|part| parse_page_margin_edge(part, font_size))
        .collect::<Vec<_>>();
    match values.as_slice() {
        [all] => Some(PageMarginEdges {
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [vertical, horizontal] => Some(PageMarginEdges {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(PageMarginEdges {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(PageMarginEdges {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

fn parse_page_margin_axis(value: &str, font_size: f32) -> Option<[PageMarginEdge; 2]> {
    let values = value
        .split_whitespace()
        .filter_map(|part| parse_page_margin_edge(part, font_size))
        .collect::<Vec<_>>();
    match values.as_slice() {
        [both] => Some([*both, *both]),
        [start, end] => Some([*start, *end]),
        _ => None,
    }
}

fn parse_page_margin_edge(value: &str, font_size: f32) -> Option<PageMarginEdge> {
    let value = parse_computed_length_percentage_auto(value, font_size)?;
    match value {
        ComputedLengthPercentageOrAuto::Auto => PageMarginEdge::Auto,
        ComputedLengthPercentageOrAuto::LengthPercentage(value) => {
            PageMarginEdge::LengthPercentage(value)
        }
        ComputedLengthPercentageOrAuto::MinContent
        | ComputedLengthPercentageOrAuto::MaxContent
        | ComputedLengthPercentageOrAuto::FitContent(_) => return None,
    }
    .into()
}

fn parse_page_padding_shorthand(
    value: &str,
    font_size: f32,
) -> Option<super::types::CssEdges<ComputedLengthPercentage>> {
    let values = value
        .split_whitespace()
        .filter_map(|part| parse_computed_length_percentage(part, font_size))
        .collect::<Vec<_>>();
    match values.as_slice() {
        [all] => Some(super::types::CssEdges {
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [vertical, horizontal] => Some(super::types::CssEdges {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(super::types::CssEdges {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(super::types::CssEdges {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

fn parse_page_padding_axis(value: &str, font_size: f32) -> Option<[ComputedLengthPercentage; 2]> {
    let values = value
        .split_whitespace()
        .filter_map(|part| parse_computed_length_percentage(part, font_size))
        .collect::<Vec<_>>();
    match values.as_slice() {
        [both] => Some([*both, *both]),
        [start, end] => Some([*start, *end]),
        _ => None,
    }
}

fn logical_page_axis_sides(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<[PhysicalSide; 2]> {
    let [start, end] = match name {
        "margin-block" | "padding-block" => ["block-start", "block-end"],
        "margin-inline" | "padding-inline" => ["inline-start", "inline-end"],
        _ => return None,
    };
    Some([
        logical_page_side(start, direction, writing_mode)?,
        logical_page_side(end, direction, writing_mode)?,
    ])
}

fn logical_page_side(
    name: &str,
    direction: Direction,
    writing_mode: WritingMode,
) -> Option<PhysicalSide> {
    match name {
        "block-start" | "margin-block-start" | "padding-block-start" => {
            Some(block_start_side(writing_mode))
        }
        "block-end" | "margin-block-end" | "padding-block-end" => {
            Some(block_end_side(writing_mode))
        }
        "inline-start" | "margin-inline-start" | "padding-inline-start" => {
            Some(inline_start_side(writing_mode, direction))
        }
        "inline-end" | "margin-inline-end" | "padding-inline-end" => {
            Some(inline_end_side(writing_mode, direction))
        }
        _ => None,
    }
}

fn set_page_margin_edge(margins: &mut PageMarginEdges, side: PhysicalSide, edge: PageMarginEdge) {
    match side {
        PhysicalSide::Top => margins.top = edge,
        PhysicalSide::Right => margins.right = edge,
        PhysicalSide::Bottom => margins.bottom = edge,
        PhysicalSide::Left => margins.left = edge,
    }
}

fn set_page_padding_edge(
    padding: &mut super::types::CssEdges<ComputedLengthPercentage>,
    side: PhysicalSide,
    edge: ComputedLengthPercentage,
) {
    match side {
        PhysicalSide::Top => padding.top = edge,
        PhysicalSide::Right => padding.right = edge,
        PhysicalSide::Bottom => padding.bottom = edge,
        PhysicalSide::Left => padding.left = edge,
    }
}

fn resolve_page_margin_axis(
    containing_size: f32,
    specified_content_size: Option<f32>,
    non_margin_size: f32,
    percentage_basis: f32,
    start: PageMarginEdge,
    end: PageMarginEdge,
    ch_advance: f32,
) -> (f32, f32) {
    let containing_size = containing_size.max(0.0);
    let non_margin_size = non_margin_size.max(0.0);
    let mut start_auto = start == PageMarginEdge::Auto;
    let mut end_auto = end == PageMarginEdge::Auto;
    let mut start = page_margin_edge_length(start, percentage_basis, ch_advance);
    let mut end = page_margin_edge_length(end, percentage_basis, ch_advance);

    if let Some(content_size) = specified_content_size {
        let content_size = content_size.max(0.0);
        let remaining = containing_size - content_size - non_margin_size - start - end;
        match (start_auto, end_auto) {
            (true, true) => {
                start = remaining / 2.0;
                end = remaining / 2.0;
            }
            (true, false) => {
                start = remaining;
            }
            (false, true) => {
                end = remaining;
            }
            (false, false) => {}
        }
        return (start, end);
    }

    if start_auto {
        start = 0.0;
        start_auto = false;
    }
    if end_auto {
        end = 0.0;
        end_auto = false;
    }
    let _content_size = (containing_size - non_margin_size - start - end).max(0.0);
    debug_assert!(!start_auto && !end_auto);
    (start, end)
}

fn page_margin_edge_length(edge: PageMarginEdge, percentage_basis: f32, ch_advance: f32) -> f32 {
    match edge {
        PageMarginEdge::LengthPercentage(value) => resolve_page_length_percentage(
            value,
            PhysicalSide::Left,
            PageSize {
                width: percentage_basis,
                height: percentage_basis,
            },
            ch_advance,
        ),
        PageMarginEdge::Auto => 0.0,
    }
}

fn resolve_page_length_percentage(
    mut value: ComputedLengthPercentage,
    side: PhysicalSide,
    page_size: PageSize,
    ch_advance: f32,
) -> f32 {
    let basis = match side {
        PhysicalSide::Top | PhysicalSide::Bottom => page_size.height,
        PhysicalSide::Right | PhysicalSide::Left => page_size.width,
    };
    value.resolve_font_metric_lengths(ch_advance);
    value
        .used_length_with_percentage_basis(basis)
        .unwrap_or(value.length + value.percent * basis)
}

pub(crate) fn apply_page_size_to_with_metrics(
    value: &str,
    page_size: &mut PageSize,
    font_size: f32,
    ch_advance: f32,
) {
    if let Some(parsed) = parse_page_size_descriptor(value, *page_size, font_size, ch_advance) {
        *page_size = parsed;
    }
}

/// Parses the CSS Paged Media `size` descriptor as one atomic value.
///
/// CSS Paged Media Level 3 defines `size` as either `auto`, one or two
/// non-negative lengths, or a named page size optionally combined with one
/// orientation keyword. Invalid extra tokens invalidate the descriptor instead
/// of allowing partial application:
/// <https://www.w3.org/TR/css-page-3/#page-size-prop>.
fn parse_page_size_descriptor(
    value: &str,
    base: PageSize,
    font_size: f32,
    ch_advance: f32,
) -> Option<PageSize> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    let mut parts = value.split_whitespace().collect::<Vec<_>>();
    let mut orientation = None;
    parts.retain(|part| {
        if matches!(*part, "portrait" | "landscape") {
            if orientation.replace(*part).is_some() {
                orientation = Some("");
            }
            false
        } else {
            true
        }
    });
    if orientation == Some("") {
        return None;
    }
    if parts.is_empty() {
        let orientation = orientation?;
        let (width, height) = oriented_page_size(base.width, base.height, orientation);
        return Some(PageSize { width, height });
    }
    if parts == ["auto"] {
        return orientation.is_none().then_some(base);
    }
    if let Some((width, height)) = named_page_size(parts.as_slice()) {
        let (width, height) = orientation
            .map(|orientation| oriented_page_size(width, height, orientation))
            .unwrap_or((width, height));
        return Some(PageSize { width, height });
    }
    if orientation.is_some() {
        return None;
    }

    let lengths = parts
        .iter()
        .map(|part| {
            parse_page_fixed_length(part, font_size, ch_advance).filter(|length| *length >= 0.0)
        })
        .collect::<Option<Vec<_>>>()?;
    match lengths.as_slice() {
        [side] => Some(PageSize {
            width: *side,
            height: *side,
        }),
        [width, height] => Some(PageSize {
            width: *width,
            height: *height,
        }),
        _ => None,
    }
}

fn parse_page_fixed_length(value: &str, font_size: f32, ch_advance: f32) -> Option<f32> {
    let value = parse_computed_length_percentage(value, font_size)?;
    (value.percent == 0.0).then(|| resolve_length_with_ch(value, ch_advance))
}

fn resolve_length_with_ch(value: ComputedLengthPercentage, ch_advance: f32) -> f32 {
    value.length + value.ch * ch_advance
}

fn oriented_page_size(width: f32, height: f32, orientation: &str) -> (f32, f32) {
    match orientation {
        "landscape" if width < height => (height, width),
        "portrait" if width > height => (height, width),
        _ => (width, height),
    }
}

fn named_page_size(parts: &[&str]) -> Option<(f32, f32)> {
    match parts {
        ["a3"] => Some((mm(297.0), mm(420.0))),
        ["a4"] => Some((mm(210.0), mm(297.0))),
        ["a5"] => Some((mm(148.0), mm(210.0))),
        ["b4"] => Some((mm(250.0), mm(353.0))),
        ["b5"] => Some((mm(176.0), mm(250.0))),
        ["jis-b4"] => Some((mm(257.0), mm(364.0))),
        ["jis-b5"] => Some((mm(182.0), mm(257.0))),
        ["letter"] => Some((inch(8.5), inch(11.0))),
        ["legal"] => Some((inch(8.5), inch(14.0))),
        ["ledger"] => Some((inch(11.0), inch(17.0))),
        _ => None,
    }
}

fn inch(value: f32) -> f32 {
    value * 72.0
}

fn mm(value: f32) -> f32 {
    inch(value / 25.4)
}
