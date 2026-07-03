use super::*;

/// Input geometry for an abspos flex child's static-position calculation.
///
/// CSS Flexbox derives the static position of an absolutely positioned flex
/// child from the flex container's content box and hypothetical sole-item flex
/// placement:
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
pub(in crate::layout::flex) struct PositionedFlexStaticContext<'a> {
    pub(in crate::layout::flex) container_style: &'a ComputedStyle,
    pub(in crate::layout::flex) stylesheets: &'a [Stylesheet],
    pub(in crate::layout::flex) available: FlexAvailableSpace,
    pub(in crate::layout::flex) inner_x: f32,
    pub(in crate::layout::flex) inner_width: f32,
    pub(in crate::layout::flex) content_top: f32,
}

/// One flex fragmentation boundary in the physical block direction.
///
/// CSS Flexbox fragments row containers by flex line and column containers by
/// item progression in paged media:
/// <https://www.w3.org/TR/css-flexbox-1/#pagination>.
#[derive(Debug, Clone)]
pub(in crate::layout::flex) struct FlexBreakUnit {
    pub(in crate::layout::flex) item_indices: Vec<usize>,
    pub(in crate::layout::flex) line_start: usize,
    pub(in crate::layout::flex) line_end: usize,
    pub(in crate::layout::flex) block_start: f32,
    pub(in crate::layout::flex) block_end: f32,
    pub(in crate::layout::flex) break_before: PageBreak,
    pub(in crate::layout::flex) break_after: PageBreak,
    pub(in crate::layout::flex) break_inside_avoid: bool,
}

impl FlexBreakUnit {
    pub(in crate::layout::flex) fn block_size(&self) -> f32 {
        (self.block_end - self.block_start).max(0.0)
    }

    pub(in crate::layout::flex) fn slice(&self, block_start: f32, block_end: f32) -> Self {
        Self {
            item_indices: self.item_indices.clone(),
            line_start: self.line_start,
            line_end: self.line_end,
            block_start,
            block_end,
            break_before: self.break_before,
            break_after: self.break_after,
            break_inside_avoid: self.break_inside_avoid,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct FlexFragmentBuildContext {
    pub(in crate::layout::flex) page_index: usize,
    pub(in crate::layout::flex) outer_x: f32,
    pub(in crate::layout::flex) outer_width: f32,
    pub(in crate::layout::flex) content_top: f32,
    pub(in crate::layout::flex) block_offset: f32,
    pub(in crate::layout::flex) starts_page_fragment: bool,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout::flex) struct SplitFlexItemPaintContext {
    pub(in crate::layout::flex) item_width: f32,
    pub(in crate::layout::flex) item_height: f32,
    pub(in crate::layout::flex) slice_border_box: PaintClip,
    pub(in crate::layout::flex) source_item_top: f32,
}

pub(in crate::layout::flex) fn placed_flex_item_style(
    child_style: &ComputedStyle,
    item_width: f32,
    item_height: f32,
    container_flex_direction: FlexDirection,
) -> ComputedStyle {
    let mut placed_style = independent_formatting_context_item_style(child_style.clone());
    placed_style.margin = css::Edges::ZERO;
    placed_style.page_name_specified = false;
    placed_style.page_name = None;
    suppress_flex_item_fragmentation_breaks(&mut placed_style);
    set_style_used_width(&mut placed_style, item_width);
    set_style_used_height(&mut placed_style, item_height);
    if container_flex_direction.is_row_axis() {
        set_style_used_width_bounds(&mut placed_style, item_width);
    } else {
        set_style_used_height_bounds(&mut placed_style, item_height);
    }
    placed_style.box_sizing = BoxSizing::BorderBox;
    placed_style
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout::flex) fn layout_flex_item_contents(
        &mut self,
        child: &StyledChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        item_height: f32,
    ) {
        let percentage_height_basis =
            flex_item_replay_percentage_height_basis(child, placed_style, item_height);
        self.with_flex_item_percentage_height_basis(percentage_height_basis, |layout| {
            layout.layout_formatting_context_item_contents(child, placed_style, stylesheets);
        });
    }
}

/// Resolve a flex container width keyword from known intrinsic contributions.
///
/// CSS Sizing defines `fit-content` as
/// `min(max-content, max(min-content, stretch-or-argument))`. Auto widths keep
/// normal block fill behavior, except float and inline-flex atom callers pass
/// `shrink_auto_width` to request CSS 2.2 shrink-to-fit sizing:
/// <https://www.w3.org/TR/css-sizing-3/#fit-content-size> and
/// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
pub(in crate::layout::flex) fn flex_container_content_width_from_intrinsic(
    style: &ComputedStyle,
    available_outer_width: f32,
    horizontal_extras: f32,
    intrinsic: FlexItemEstimate,
    shrink_auto_width: bool,
) -> f32 {
    let min_content = intrinsic.min_width.points().max(0.0);
    let max_content = intrinsic.width.points().max(min_content).max(0.0);
    let auto_width = if shrink_auto_width {
        intrinsic::IntrinsicAutoWidth::ShrinkToFit
    } else {
        intrinsic::IntrinsicAutoWidth::FillAvailable
    };
    intrinsic::content_width_from_intrinsic(
        style,
        available_outer_width,
        horizontal_extras,
        min_content,
        max_content,
        auto_width,
    )
}

/// Returns whether a block flex container's auto physical width needs intrinsic sizing.
///
/// CSS Writing Modes sizes orthogonal flow roots with the fit-content rule
/// rather than stretching the block axis to the containing block's physical
/// width. For a vertical-writing flex container in horizontal flow, that means
/// `width:auto` must shrink-wrap the flex cross size while `height` remains
/// the container's logical inline/main size:
/// <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto> and
/// <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>.
pub(in crate::layout::flex) fn orthogonal_auto_width_flex_container_needs_intrinsic(
    style: &ComputedStyle,
    containing_space: ChildAvailableSpace,
) -> bool {
    style.box_values.width.is_auto()
        && matches!(
            (containing_space.writing_mode, style.writing_mode),
            (
                WritingMode::HorizontalTb,
                WritingMode::VerticalRl | WritingMode::VerticalLr
            ) | (
                WritingMode::VerticalRl | WritingMode::VerticalLr,
                WritingMode::HorizontalTb
            )
        )
}
