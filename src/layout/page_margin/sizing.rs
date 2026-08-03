use super::*;
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
pub(super) fn fixed_height_axis_with_intrinsic(
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
                style.box_values.height.value().clone(),
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
pub(super) fn fixed_width_axis_with_intrinsic(
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

/// Resolves the outer sizes of the two boxes used by a variable-axis step.
///
/// When exactly one box is automatic, it receives the space left by its
/// definite peer. When both are automatic, CSS Paged Media distributes the
/// space by their intrinsic outer dimensions.
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>
pub(in crate::layout) fn resolve_two_outer_sizes(
    available: f32,
    measures: [PageMarginBoxMeasure; 2],
) -> [f32; 2] {
    let available = available.max(0.0);
    let mut sizes = [
        measures[0].resolved_or_zero(),
        measures[1].resolved_or_zero(),
    ];
    match (measures[0].auto_outer(), measures[1].auto_outer()) {
        (false, false) => return sizes,
        (true, false) => {
            sizes[0] = (available - sizes[1]).max(0.0);
            return sizes;
        }
        (false, true) => {
            sizes[1] = (available - sizes[0]).max(0.0);
            return sizes;
        }
        (true, true) => {}
    }
    let max_sum = measures[0].max_outer + measures[1].max_outer;
    let min_sum = measures[0].min_outer + measures[1].min_outer;
    if max_sum < available {
        let flex_space = available - max_sum;
        let factors = normalized_flex_factors([measures[0].max_outer, measures[1].max_outer]);
        [
            measures[0].max_outer + flex_space * factors[0],
            measures[1].max_outer + flex_space * factors[1],
        ]
    } else if min_sum < available {
        let flex_space = available - min_sum;
        let factors = normalized_flex_factors([
            (measures[0].max_outer - measures[0].min_outer).max(0.0),
            (measures[1].max_outer - measures[1].min_outer).max(0.0),
        ]);
        [
            measures[0].min_outer + flex_space * factors[0],
            measures[1].min_outer + flex_space * factors[1],
        ]
    } else {
        let factors = normalized_flex_factors([measures[0].min_outer, measures[1].min_outer]);
        [available * factors[0], available * factors[1]]
    }
}

/// Resolve a two-box CSS Page allocation, repeating it for saturated min/max
/// constraints instead of independently clamping the first result.
///
/// CSS Paged Media §5.3.2 says a violated maximum is used as the computed
/// dimension and the allocation is rerun; minima are handled by the same
/// mechanism after maximums. This is also used for the centre box and each
/// separate imaginary symmetric side candidate.
/// <https://www.w3.org/TR/css-page-3/#margin-dimension>
pub(in crate::layout) fn resolve_two_outer_sizes_with_constraints(
    available: f32,
    measures: [PageMarginBoxMeasure; 2],
) -> [f32; 2] {
    let mut saturated = measures;
    loop {
        let sizes = resolve_two_outer_sizes(available, saturated);
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
        let sizes = resolve_two_outer_sizes(available, saturated);
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

pub(in crate::layout) fn normalized_flex_factors(values: [f32; 2]) -> [f32; 2] {
    let sum = values[0] + values[1];
    if sum <= 0.0 {
        [0.5, 0.5]
    } else {
        [values[0] / sum, values[1] / sum]
    }
}
