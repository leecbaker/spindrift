use super::super::*;
use super::{BlockFlowChildTraversalState, BlockFlowChildrenPhaseInput};
use crate::css::Edges;
use crate::layout::block::flow::children::{
    BlockFlowMarginCollapseContext,
    shared::{BlockEndMarginTrim, preserve_adjusted_block_margins},
};
use crate::layout::builder::page_for_context;
use crate::layout::inline_layout::InlineLayoutOutcome;
use std::cmp::Reverse;
use std::ops::Deref;

/// A multicol formatting-context style after the CSS `zoom` used-value
/// boundary.
///
/// Column planning, balancing, fragmentation, and rule painting consume the
/// normalized style. The source style remains available only when a multicol
/// entry point must construct frozen descendant boxes, preventing a scaled
/// font or fixed length from becoming a cascade parent.
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
/// <https://www.w3.org/TR/css-multicol-1/#multi-column-layout>
#[derive(Debug, Clone)]
pub(in crate::layout) struct MulticolUsedStyle {
    source: ComputedStyle,
    used: crate::css::ZoomedLayoutStyle,
}

impl MulticolUsedStyle {
    fn from_source_and_normalized(
        source: ComputedStyle,
        used: crate::css::ZoomedLayoutStyle,
    ) -> Self {
        Self { source, used }
    }

    pub(in crate::layout::block) fn source(&self) -> &ComputedStyle {
        &self.source
    }

    pub(in crate::layout) fn as_computed(&self) -> &ComputedStyle {
        &self.used
    }
}

impl Deref for MulticolUsedStyle {
    type Target = ComputedStyle;

    fn deref(&self) -> &Self::Target {
        &self.used
    }
}

/// Balance probes must converge below the rasterizer's subpixel threshold.
/// A quarter-CSS-pixel interval changes glyph and one-device-pixel rule
/// antialiasing in otherwise exact percentage-sized column sets.
const MULTICOL_BALANCE_EPSILON: f32 = css::CSS_PX_TO_PT / 128.0;

#[derive(Debug, Clone, Copy)]
struct EstimatedMulticolFlowUnit {
    /// Estimated CSS block extent before multicol breakpoint arithmetic.
    block_size: LayoutLength,
    avoid_before: bool,
    avoid_after: bool,
    forced_before: bool,
    forced_after: bool,
    avoid_inside_boundary_before: bool,
}

#[derive(Debug, Clone, Copy)]
struct EstimatedMulticolFloat {
    /// Normal-flow block position at which this float is encountered.
    block_offset: LayoutLength,
    /// Mixed content, border/padding, and margin extent used for float bands.
    outer_width: LayoutLength,
    /// Mixed content, border/padding, and margin extent used for float bands.
    outer_height: LayoutLength,
}

#[derive(Debug, Clone, Copy)]
struct MulticolDescendantPercentageBasis(BlockSizePercentageBasis);

impl MulticolDescendantPercentageBasis {
    const INDEFINITE: Self = Self(PercentageBasis::Indefinite);

    fn from_points(value: Option<f32>) -> Self {
        Self(block_size_percentage_basis_from_points(
            value,
            BlockSizeBasisSource::ContainingBlock,
        ))
    }

    fn basis(self) -> BlockSizePercentageBasis {
        self.0
    }

    fn and_then<R>(self, f: impl FnOnce(f32) -> Option<R>) -> Option<R> {
        self.0.points().and_then(f)
    }

    fn map<R>(self, f: impl FnOnce(f32) -> R) -> Option<R> {
        self.0.points().map(f)
    }
}

impl Default for MulticolDescendantPercentageBasis {
    fn default() -> Self {
        Self::INDEFINITE
    }
}

/// Whether a later temporary column page needs its complete destination
/// rectangle retained during replay.
///
/// The first source page still owns normal-flow inline overflow. A later page
/// is already a committed continuation, however, so in an orthogonal writing
/// mode its cross-axis ink must not escape beyond the destination column after
/// the local-to-page translation. Horizontal multicolumn replay keeps the
/// established cross-axis overflow behavior.
/// <https://www.w3.org/TR/css-multicol-1/#overflow-inside-multicol>
/// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>
fn continuation_column_fragment_requires_full_clip(
    fragment_index: usize,
    writing_mode: WritingMode,
    direction: Direction,
) -> bool {
    fragment_index > 0 && WritingModeAxes::new(writing_mode, direction).swaps_physical_axes()
}

/// Independent block-axis constraints for one anonymous column set.
///
/// A final set can have a definite used height, an earlier balancing set can
/// merely be capped by the multicol container, and descendant percentages
/// always resolve against the original containing block rather than either
/// fragment-local limit. Keeping these quantities separate prevents the
/// planner from accidentally using a remaining fragment size as a percentage
/// basis.
/// <https://www.w3.org/TR/css-multicol-1/#filling-columns>
#[derive(Debug, Clone, Copy, Default)]
struct MulticolColumnHeightConstraints {
    used: Option<f32>,
    balance_limit: Option<f32>,
    descendant_percentage_basis: MulticolDescendantPercentageBasis,
    balance_definite_column_set: bool,
}

/// Committed principal-flow geometry exported by a multicol formatting
/// context.
///
/// Column balancing uses temporary fragmentainer pages and restores the outer
/// builder state before replaying those pages.  Atomic inline callers still
/// need the resulting used block size and final compatible baseline after
/// that restoration, so these metrics are captured at the multicol commit
/// boundary rather than inferred from the restored cursor.
/// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
/// <https://drafts.csswg.org/css-align-3/#baseline-export>
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct MulticolFlowLayoutOutcome {
    is_multicol_layout: bool,
    committed_block_extent: LayoutLength,
    final_in_flow_baseline: Option<LayoutLength>,
}

impl MulticolFlowLayoutOutcome {
    fn not_multicol() -> Self {
        Self {
            is_multicol_layout: false,
            committed_block_extent: layout_pt(0.0),
            final_in_flow_baseline: None,
        }
    }

    fn column_set(
        committed_block_extent: LayoutLength,
        final_in_flow_baseline: Option<LayoutLength>,
    ) -> Self {
        Self {
            is_multicol_layout: true,
            committed_block_extent,
            final_in_flow_baseline,
        }
    }

    fn compose_segment(
        self,
        segment_start: LayoutLength,
        segment_extent: LayoutLength,
        segment_baseline: Option<LayoutLength>,
    ) -> Self {
        debug_assert!(self.is_multicol_layout);
        let segment_end = segment_start.points() + segment_extent.points();
        Self {
            is_multicol_layout: true,
            committed_block_extent: layout_pt(
                self.committed_block_extent.points().max(segment_end),
            ),
            final_in_flow_baseline: segment_baseline
                .map(|baseline| layout_pt(segment_start.points() + baseline.points()))
                .or(self.final_in_flow_baseline),
        }
    }

    pub(in crate::layout) fn is_multicol_layout(self) -> bool {
        self.is_multicol_layout
    }

    /// The content-box block extent, measured from the flow-root block start.
    pub(in crate::layout) fn committed_block_extent(self) -> LayoutLength {
        self.committed_block_extent
    }

    /// Offset from that same content-box start to the final baseline, when a
    /// compatible in-flow line survived the committed multicol layout.
    pub(in crate::layout) fn final_in_flow_baseline(self) -> Option<LayoutLength> {
        self.final_in_flow_baseline
    }
}

struct MulticolBalanceProbeInput<'b, 'boxes> {
    element: &'b Element,
    style: &'b ComputedStyle,
    stylesheets: &'b Stylesheets<'b>,
    child_boxes: &'b [box_tree::FormattingBox<'boxes>],
    column_width: f32,
    candidate_height: f32,
    column_count: usize,
    descendant_percentage_height_basis: MulticolDescendantPercentageBasis,
    relax_widows_orphans: bool,
}

struct MulticolBalanceSearchInput<'b, 'boxes> {
    element: &'b Element,
    style: &'b ComputedStyle,
    stylesheets: &'b Stylesheets<'b>,
    child_boxes: &'b [box_tree::FormattingBox<'boxes>],
    column_width: f32,
    column_count: usize,
    descendant_percentage_height_basis: MulticolDescendantPercentageBasis,
    estimated_content_height: f32,
    estimated_balanced_height: f32,
    minimum_structural_column_height: f32,
    available_page_height: f32,
    retain_available_height_when_unfit: bool,
}

/// Source-ordered pieces of one multicol formatting context.
///
/// A lower-level `column-span: all` box is promoted out of its intervening
/// ordinary block containers. Those containers remain as fragmented wrappers
/// in the surrounding column sets, while the spanner itself becomes a sibling
/// segment whose containing block is the multicol container.
/// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
#[derive(Debug, Clone)]
enum MulticolFlowSegment<'a> {
    ColumnSet(Vec<box_tree::FormattingBox<'a>>),
    Spanner(Box<box_tree::FormattingBox<'a>>),
}

impl<'a> LayoutBuilder<'a> {
    /// Prepare a multicol root without changing the style retained for
    /// descendant cascade reconstruction.
    pub(in crate::layout) fn multicol_used_style(
        &self,
        source: &ComputedStyle,
    ) -> MulticolUsedStyle {
        let used = self.style_with_current_viewport_lengths(source);
        MulticolUsedStyle::from_source_and_normalized(source.clone(), used)
    }

    /// Estimate the auto block-size of a multicol container for an outer
    /// sizing algorithm such as Flexbox.
    ///
    /// Each spanner separates independently balanced column sets, and its own
    /// block size is then added at full multicol width. This mirrors the
    /// structure consumed by committed multicol layout without reducing a
    /// mixed block flow to whichever trailing inline line happens to exist:
    /// <https://www.w3.org/TR/css-multicol-1/#spanning-columns> and
    /// <https://www.w3.org/TR/css-multicol-1/#filling-columns>.
    pub(in crate::layout) fn estimate_multicol_auto_block_size(
        &mut self,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: &[box_tree::FormattingBox<'_>],
        available_width: f32,
    ) -> Option<f32> {
        let multicol_style = self.multicol_used_style(style);
        let style = &multicol_style;
        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            style.font_size,
        )
        .points();
        let column_count =
            used_multicol_column_count(style, available_width, gap).filter(|count| *count > 0)?;
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((available_width - total_gap) / column_count as f32).max(1.0);
        let mut total_height = 0.0f32;
        let mut segment_start = 0usize;
        for (index, child) in child_boxes.iter().enumerate() {
            let Some((element, _, child_style, children)) = child.element_parts() else {
                continue;
            };
            if child_style.column_span != css::ColumnSpan::All
                || !style_is_in_normal_flow(child_style)
                || child_style.float != Float::None
                || !child_style.display.is_block_level()
            {
                continue;
            }
            total_height += self.estimated_balanced_column_set_block_size(
                style,
                stylesheets,
                &child_boxes[segment_start..index],
                available_width,
                column_width,
                column_count,
            );
            total_height += self
                .estimate_element_height(
                    element,
                    child_style,
                    stylesheets,
                    available_width,
                    Some(children),
                )
                .unwrap_or(child_style.line_height)
                .max(0.0);
            segment_start = index + 1;
        }
        total_height += self.estimated_balanced_column_set_block_size(
            style,
            stylesheets,
            &child_boxes[segment_start..],
            available_width,
            column_width,
            column_count,
        );
        Some(total_height)
    }

    fn estimated_balanced_column_set_block_size(
        &mut self,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: &[box_tree::FormattingBox<'_>],
        available_width: f32,
        column_width: f32,
        column_count: usize,
    ) -> f32 {
        if child_boxes.is_empty() {
            return 0.0;
        }
        let flow_units = self.estimated_multicol_flow_units(
            child_boxes,
            stylesheets,
            column_width,
            MulticolDescendantPercentageBasis::INDEFINITE,
            false,
        );
        let normal_flow_extent = flow_units
            .iter()
            .map(|unit| unit.block_size.points())
            .sum::<f32>();
        let float_flow_extent =
            self.estimated_multicol_float_flow_extent(child_boxes, stylesheets, column_width);
        let flow_extent = normal_flow_extent.max(float_flow_extent);
        if flow_extent <= 0.01 {
            return 0.0;
        }
        if let css::ComputedColumnHeight::Length(height) = &style.column_height
            && let Some(column_height) =
                height.length_if_no_percent().filter(|height| *height > 0.0)
        {
            // A definite column height fixes the row grid. The auto block size
            // is the number of occupied wrapped rows, not the unfragmented
            // height of all descendants. This estimate is also consumed by an
            // ancestor multicol planner, so overestimating it would synthesize
            // overflow columns for paint that is already fragmented inside
            // the nested multicol formatting context.
            // <https://drafts.csswg.org/css-multicol-2/#column-height>
            let occupied_columns = (flow_extent / column_height).ceil().max(1.0) as usize;
            let row_count = if matches!(
                style.column_wrap,
                css::ColumnWrap::Auto | css::ColumnWrap::Wrap
            ) {
                occupied_columns.div_ceil(column_count)
            } else {
                1
            };
            let row_gap = used_multicol_column_gap(
                style.row_gap.clone(),
                PercentageBasis::definite(content_box_pt(available_width)),
                style.font_size,
            )
            .points();
            return row_count as f32 * column_height + row_count.saturating_sub(1) as f32 * row_gap;
        }
        (flow_extent / column_count as f32)
            .max(style.line_height)
            .max(self.estimated_multicol_monolithic_block_size(
                child_boxes,
                stylesheets,
                column_width,
            ))
    }

    pub(in crate::layout) fn layout_simple_block_child_columns(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        content_height: Option<f32>,
    ) -> MulticolFlowLayoutOutcome {
        let multicol_style = self.multicol_used_style(style);
        let style = &multicol_style;
        // This helper owns only a multicolumn formatting context. Ordinary
        // block containers with nested inline structure reach it while the
        // caller determines whether the child flow needs a column planner;
        // do not freeze their subtree merely to discover that no columns
        // exist.
        // <https://drafts.csswg.org/css-multicol-1/#multi-column-layout>
        if !style_establishes_multicol_formatting_context(style) {
            return MulticolFlowLayoutOutcome::not_multicol();
        }
        let built_child_boxes;
        let child_boxes = if let Some(child_boxes) = child_boxes {
            child_boxes
        } else {
            built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                element,
                stylesheets,
                multicol_style.source(),
            );
            &built_child_boxes
        };
        // Build one source-ordered principal flow regardless of whether the
        // spanner is a direct child or promoted through an eligible wrapper.
        // The former used to take a separate loop below, which meant that
        // anonymous inline runs and principal-box state (notably list markers
        // and inline-block positioning) observed a different multicol
        // lifecycle solely because of the spanner's depth.
        // <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
        if let Some(mut segments) = descendant_multicol_flow_segments(child_boxes) {
            self.distribute_descendant_spanner_wrapper_block_sizes(
                &mut segments,
                stylesheets,
                style,
            );
            return self.layout_multicol_flow_segments(
                element,
                style,
                stylesheets,
                &segments,
                content_height,
            );
        }
        self.layout_multicol_column_set(
            element,
            style,
            stylesheets,
            child_boxes,
            MulticolColumnHeightConstraints {
                used: content_height,
                balance_limit: None,
                descendant_percentage_basis: MulticolDescendantPercentageBasis::from_points(
                    content_height,
                ),
                balance_definite_column_set: true,
            },
        )
    }

    /// Lay out source-ordered principal-flow segments separated by spanners.
    ///
    /// Direct spanners and spanners below ordinary block or split-inline
    /// wrappers follow this same path. The segment tree keeps wrapper
    /// fragments and anonymous inline runs in the column flow while each
    /// spanner is laid out at the multicol container's full inline size.
    /// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
    fn layout_multicol_flow_segments(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        segments: &[MulticolFlowSegment<'_>],
        content_height: Option<f32>,
    ) -> MulticolFlowLayoutOutcome {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            style.font_size,
        )
        .points();
        if used_multicol_column_count(style, available_width, gap).unwrap_or(0) == 0 {
            return MulticolFlowLayoutOutcome::not_multicol();
        }

        let content_top = self.cursor_y;
        let mut outcome = MulticolFlowLayoutOutcome::column_set(layout_pt(0.0), None);
        let mut index = 0usize;
        while index < segments.len() {
            match &segments[index] {
                MulticolFlowSegment::ColumnSet(boxes) => {
                    if !boxes.is_empty() {
                        let has_later_spanner = segments[index + 1..]
                            .iter()
                            .any(|segment| matches!(segment, MulticolFlowSegment::Spanner(_)));
                        let mut set_style = style.clone();
                        if has_later_spanner {
                            set_style.column_fill = css::ColumnFill::Balance;
                        } else if content_height.is_none()
                            && set_style.column_fill == css::ColumnFill::Auto
                        {
                            // With no definite multicol block size, `auto`
                            // has no finite sequential fill target. The final
                            // column set is therefore intrinsically balanced;
                            // using the remaining page area would incorrectly
                            // make an auto-height multicol container page-tall.
                            // <https://www.w3.org/TR/css-multicol-1/#filling-columns>
                            set_style.column_fill = css::ColumnFill::Balance;
                        }
                        let remaining_content_height = if has_later_spanner {
                            None
                        } else {
                            content_height
                                .map(|height| (height - (content_top - self.cursor_y)).max(0.0))
                        };
                        let column_set_start = self.cursor_y;
                        let column_set_outcome = self.layout_multicol_column_set(
                            element,
                            &set_style,
                            stylesheets,
                            boxes,
                            MulticolColumnHeightConstraints {
                                used: remaining_content_height,
                                balance_limit: has_later_spanner
                                    .then_some(content_height)
                                    .flatten(),
                                descendant_percentage_basis:
                                    MulticolDescendantPercentageBasis::from_points(content_height),
                                balance_definite_column_set: true,
                            },
                        );
                        if !column_set_outcome.is_multicol_layout() {
                            return MulticolFlowLayoutOutcome::not_multicol();
                        }
                        let column_set_offset = (content_top - column_set_start).max(0.0);
                        outcome = outcome.compose_segment(
                            layout_pt(column_set_offset),
                            column_set_outcome.committed_block_extent(),
                            column_set_outcome.final_in_flow_baseline(),
                        );
                        outcome = outcome.compose_segment(
                            layout_pt(0.0),
                            layout_pt((content_top - self.cursor_y).max(0.0)),
                            None,
                        );
                    }
                    index += 1;
                }
                MulticolFlowSegment::Spanner(_) => {
                    let group_start = index;
                    while index < segments.len()
                        && matches!(segments[index], MulticolFlowSegment::Spanner(_))
                    {
                        index += 1;
                    }
                    let spanner_boxes = segments[group_start..index]
                        .iter()
                        .filter_map(|segment| match segment {
                            MulticolFlowSegment::Spanner(box_) => Some((**box_).clone()),
                            MulticolFlowSegment::ColumnSet(_) => None,
                        })
                        .collect::<Vec<_>>();
                    let spanner_boxes = multicol_spanner_boxes_with_container_inline_size(
                        &spanner_boxes,
                        style,
                        content_height
                            .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                        PhysicalContentWidth::new(content_box_pt(available_width)),
                    );
                    self.multicol_spanner_fragmentation_depth += 1;
                    self.layout_block_flow_children_phase(Box::new(BlockFlowChildrenPhaseInput {
                        fragmentainer_kind: self.active_fragmentainer_kind(),
                        element,
                        style,
                        stylesheets,
                        child_boxes: Some(&spanner_boxes),
                        can_collapse_start_margin: false,
                        can_collapse_end_margin: false,
                        applied_start_margin: layout_pt(0.0),
                        clearance_consumed_adjoining_start_margin: false,
                        starts_at_page_top: self.cursor_is_at_page_top(),
                        laid_out_column_children: false,
                        use_box_inline_items: false,
                        run_in_inline_items_laid_out: false,
                        use_ordered_mixed_flow: false,
                        has_preceding_inline_flow_content: false,
                        definite_content_height: content_height,
                        descendant_percentage_height_basis: content_height.map(|height| {
                            block_size_percentage_basis_from_points(
                                Some(height),
                                BlockSizeBasisSource::ContainingBlock,
                            )
                        }),
                    }));
                    self.multicol_spanner_fragmentation_depth -= 1;
                    self.align_wrapped_multicol_after_spanner(style, content_top, available_width);
                    let spanner_start = (content_top - self.cursor_y).max(0.0);
                    outcome = outcome.compose_segment(
                        layout_pt(0.0),
                        layout_pt(spanner_start),
                        self.last_in_flow_line_baseline_y
                            .map(|baseline| layout_pt((content_top - baseline).max(0.0))),
                    );
                }
            }
        }
        outcome
    }

    /// Advances a post-spanner column set to the next fixed multicol row.
    ///
    /// A non-`auto` `column-height` establishes one row grid for the entire
    /// multicol container. Spanners consume space within that grid; they do
    /// not re-anchor subsequent column rows at their own block-end.
    /// <https://drafts.csswg.org/css-multicol-2/#column-height>
    fn align_wrapped_multicol_after_spanner(
        &mut self,
        style: &ComputedStyle,
        content_top: f32,
        available_width: f32,
    ) {
        let css::ComputedColumnHeight::Length(ref height) = style.column_height else {
            return;
        };
        if !matches!(
            style.column_wrap,
            css::ColumnWrap::Auto | css::ColumnWrap::Wrap
        ) {
            return;
        }
        let Some(column_height) = height.length_if_no_percent().filter(|height| *height > 0.0)
        else {
            return;
        };
        let row_gap = used_multicol_column_gap(
            style.row_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            style.font_size,
        )
        .points();
        let stride = column_height + row_gap;
        if stride <= 0.01 {
            return;
        }
        let consumed = (content_top - self.cursor_y).max(0.0);
        let next_row = (consumed / stride).ceil() as usize;
        if next_row == 0 {
            return;
        }
        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            style.font_size,
        )
        .points();
        let Some(column_count) = used_multicol_column_count(style, available_width, gap) else {
            return;
        };
        let column_width = ((available_width - gap * column_count.saturating_sub(1) as f32)
            / column_count as f32)
            .max(1.0);
        let rule_paint_point = self
            .current_page
            .paint_band_insertion_point(PaintBand::InFlowBlock);
        let rule_primitives = multicol_row_gap_decoration_primitives(
            style,
            self.content_left,
            content_top,
            available_width,
            column_height,
            row_gap,
            next_row - 1,
            next_row,
            column_width,
            gap,
            column_count,
        );
        self.current_page
            .insert_primitives_at_paint_band_point(rule_paint_point, rule_primitives);
        self.cursor_y = content_top - next_row as f32 * stride;
    }

    /// Distribute a definite block size over the fragments of a wrapper split
    /// by descendant spanners.
    ///
    /// Intermediate fragments are intrinsically sized. The final fragment
    /// receives the part of the wrapper's specified block size not consumed by
    /// the earlier fragments. This is the same block-size distribution used by
    /// fragmented boxes generally; a promoted spanner does not duplicate its
    /// ancestor's specified size for every generated wrapper fragment.
    /// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>
    fn distribute_descendant_spanner_wrapper_block_sizes<'b>(
        &mut self,
        segments: &mut [MulticolFlowSegment<'b>],
        stylesheets: &Stylesheets<'_>,
        multicol_style: &ComputedStyle,
    ) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let gap = used_multicol_column_gap(
            multicol_style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            multicol_style.font_size,
        )
        .points();
        let Some(column_count) = used_multicol_column_count(multicol_style, available_width, gap)
            .filter(|count| *count > 0)
        else {
            return;
        };
        let column_width = ((available_width - gap * column_count.saturating_sub(1) as f32)
            / column_count as f32)
            .max(1.0);

        let mut wrapper_depths = HashMap::<usize, usize>::new();
        for segment in segments.iter() {
            let MulticolFlowSegment::ColumnSet(boxes) = segment else {
                continue;
            };
            collect_multicol_wrapper_depths(boxes, 0, &mut wrapper_depths);
        }
        let mut wrapper_keys = wrapper_depths.into_iter().collect::<Vec<_>>();
        wrapper_keys.sort_by_key(|(_, depth)| Reverse(*depth));

        for (key, _) in wrapper_keys {
            let fragments = segments
                .iter()
                .filter_map(|segment| match segment {
                    MulticolFlowSegment::ColumnSet(boxes) => Some(boxes.as_slice()),
                    MulticolFlowSegment::Spanner(_) => None,
                })
                .flat_map(|boxes| multicol_wrapper_fragments(boxes, key))
                .collect::<Vec<_>>();
            if fragments.len() < 2 {
                continue;
            }
            let Some((_, final_style, _)) = fragments.last().cloned() else {
                continue;
            };
            let Some(specified_block_size) = definite_logical_block_size(final_style, column_width)
            else {
                continue;
            };
            let intrinsic_sizes = fragments[..fragments.len() - 1]
                .iter()
                .map(|(element, fragment_style, children)| {
                    if children.is_empty() {
                        return 0.0;
                    }
                    let metrics = used_box_metrics(
                        fragment_style,
                        PercentageBasis::definite(layout_pt(column_width)),
                    );
                    let outer_size = self
                        .estimate_element_height(
                            element,
                            fragment_style,
                            stylesheets,
                            column_width,
                            Some(children),
                        )
                        .unwrap_or(fragment_style.line_height);
                    let block_non_content = if WritingModeAxes::new(
                        fragment_style.writing_mode,
                        fragment_style.direction,
                    )
                    .swaps_physical_axes()
                    {
                        metrics.horizontal_non_content_length().points()
                            + metrics.margin.left.points()
                            + metrics.margin.right.points()
                    } else {
                        metrics.vertical_non_content_length().points()
                            + metrics.margin.top.points()
                            + metrics.margin.bottom.points()
                    };
                    (outer_size - block_non_content).max(0.0)
                })
                .collect::<Vec<_>>();
            let mut remaining = specified_block_size;
            let mut allocations = Vec::with_capacity(fragments.len());
            for intrinsic_size in intrinsic_sizes {
                let allocation = intrinsic_size.min(remaining).max(0.0);
                allocations.push(allocation);
                remaining = (remaining - allocation).max(0.0);
            }
            allocations.push(remaining);
            set_multicol_wrapper_block_sizes(segments, key, &allocations);
        }
    }

    /// Lay out one uninterrupted column set into anonymous column
    /// fragmentainers.
    ///
    /// A `column-span:all` descendant is handled by the caller as the boundary
    /// between two such sets. This keeps column balancing and float state
    /// scoped to the anonymous column boxes required by CSS Multicol.
    /// <https://www.w3.org/TR/css-multicol-1/#column-box>
    fn layout_multicol_column_set(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: &[box_tree::FormattingBox<'_>],
        height_constraints: MulticolColumnHeightConstraints,
    ) -> MulticolFlowLayoutOutcome {
        let MulticolColumnHeightConstraints {
            used: content_height,
            balance_limit: balanced_column_height_limit,
            descendant_percentage_basis: descendant_percentage_height_basis,
            balance_definite_column_set,
        } = height_constraints;
        if is_definition_list_element(element) {
            return MulticolFlowLayoutOutcome::not_multicol();
        }
        if child_boxes.is_empty() {
            return MulticolFlowLayoutOutcome::not_multicol();
        }
        // A nested column set is laid out while its parent column remains the
        // containing block. Temporary fragmentainer page margins can expose
        // a wider physical page slice here, so use the typed logical inline
        // basis retained for descendants rather than reconstructing one from
        // those transient coordinates.
        // <https://www.w3.org/TR/css-multicol-1/#column-box>
        let containing_inline_size = self
            .multicol_column_containing_blocks
            .last()
            .copied()
            .map(|containing_block| containing_block.inline_size)
            .unwrap_or_else(|| {
                LogicalInlineContentSize::new(content_box_pt(
                    self.current_content_logical_inline_size(),
                ))
            });
        let available_width = containing_inline_size.points().max(1.0);
        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            style.font_size,
        )
        .points();
        let Some(column_count) =
            used_multicol_column_count(style, available_width, gap).filter(|count| *count > 0)
        else {
            return MulticolFlowLayoutOutcome::not_multicol();
        };
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((available_width - total_gap) / column_count as f32).max(1.0);
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let specified_column_height = match &style.column_height {
            css::ComputedColumnHeight::Auto => None,
            css::ComputedColumnHeight::Length(height) => height.length_if_no_percent(),
        };
        let wrap_column_rows = specified_column_height.is_some()
            && matches!(
                style.column_wrap,
                css::ColumnWrap::Auto | css::ColumnWrap::Wrap
            );
        let row_gap = used_multicol_column_gap(
            style.row_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            style.font_size,
        )
        .points();
        let remaining_parent_fragmentainer_height =
            (previous_cursor_y - self.page_bottom()).max(css::CSS_PX_TO_PT);
        // A nested multicol may start partway through an outer column, while
        // its continuation rows start at the top of later outer columns.
        // Balance continuation columns against the full parent
        // fragmentainer capacity; the first anonymous column receives the
        // smaller remaining capacity separately below. Using the remaining
        // capacity for every continuation makes monolithic descendants stay
        // artificially sliced into short columns after they advance.
        // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
        // <https://www.w3.org/TR/css-break-3/#unforced-breaks>
        let available_page_height =
            if self.active_fragmentainer_kind() == FragmentainerKind::Column {
                self.page_area_height()
            } else {
                remaining_parent_fragmentainer_height
            }
            .max(style.line_height);
        // A definite `column-height` remains the size of every anonymous
        // column even when the nested multicol begins near the end of an outer
        // column. Only the first anonymous column is clipped to the remaining
        // parent capacity below; continuation columns use the authored height.
        // The principal box's definite block size follows the ordinary outer
        // fragmentation constraint independently.
        // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
        let column_set_content_height = specified_column_height.or_else(|| {
            content_height.map(|height| {
                if self.active_fragmentainer_kind() == FragmentainerKind::Column {
                    height.min(remaining_parent_fragmentainer_height)
                } else {
                    height
                }
            })
        });
        let fragments_across_parent_fragmentainer = specified_column_height.is_none()
            && content_height.zip(column_set_content_height).is_some_and(
                |(principal_height, fragment_height)| principal_height > fragment_height + 0.01,
            );
        let overflow_context = self.document_canvas_overflow;
        let mut estimated_normal_flow_height = 0.0f32;
        let mut estimated_parallel_flow_height = 0.0f32;
        for child in child_boxes {
            if matches!(
                child,
                box_tree::FormattingBox::AnonymousBlock(_)
                    | box_tree::FormattingBox::InlineSplitBlockContext(_)
            ) {
                let contribution = self
                    .estimated_multicol_flow_units(
                        std::slice::from_ref(child),
                        stylesheets,
                        column_width,
                        descendant_percentage_height_basis,
                        false,
                    )
                    .iter()
                    .map(|unit| unit.block_size.points())
                    .sum::<f32>();
                estimated_normal_flow_height += contribution;
                estimated_parallel_flow_height =
                    estimated_parallel_flow_height.max(estimated_normal_flow_height);
                continue;
            }
            let Some((element, _, child_style, children)) = child.element_parts() else {
                continue;
            };
            if !style_is_in_normal_flow(child_style) || child_style.float != Float::None {
                continue;
            }
            if is_self_collapsing_block_box(element, child_style, children, overflow_context) {
                continue;
            }
            let definite_height = descendant_percentage_height_basis.and_then(|basis| {
                let metrics = used_box_metrics(
                    child_style,
                    PercentageBasis::definite(layout_pt(column_width)),
                );
                used_content_box_height_or_auto_with_basis(
                    child_style,
                    PercentageBasis::definite(content_box_pt(basis)),
                    metrics.vertical_non_content_length(),
                )
                .map(|height| {
                    constrain_content_height(
                        child_style,
                        height,
                        PercentageBasis::definite(layout_pt(column_width)),
                    )
                    .points()
                        + metrics.vertical_non_content_length().points()
                        + metrics.margin.top.points()
                        + metrics.margin.bottom.points()
                })
            });
            let own_height = definite_height
                .or_else(|| {
                    self.estimate_element_height(
                        element,
                        child_style,
                        stylesheets,
                        column_width,
                        Some(children),
                    )
                })
                .unwrap_or(0.0)
                .max(0.0);
            let descendant_flow_height = self
                .estimated_multicol_flow_units(
                    std::slice::from_ref(child),
                    stylesheets,
                    column_width,
                    descendant_percentage_height_basis,
                    false,
                )
                .iter()
                .map(|unit| unit.block_size.points())
                .sum::<f32>();
            let derives_height_from_flow = definite_height.is_none()
                && definite_logical_block_size(child_style, column_width).is_none();
            let contribution = if derives_height_from_flow {
                own_height.max(descendant_flow_height)
            } else {
                own_height
            };
            // A definite principal box contributes only its authored size to
            // subsequent normal flow, while visible descendants may continue
            // through later columns as a parallel fragmented flow. Track the
            // furthest such reach from the box's normal-flow start instead of
            // adding it to the sibling cursor.
            // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            estimated_parallel_flow_height = estimated_parallel_flow_height
                .max(estimated_normal_flow_height + descendant_flow_height.max(contribution));
            estimated_normal_flow_height += contribution;
        }
        // Floats participate in the fragmented column flow, but do not take
        // up normal-flow block space. Balance against the greater of the two
        // independent extents rather than adding them: a tall float may need
        // several columns of paint, while a same-height in-flow sibling still
        // occupies those very columns behind it.
        // <https://www.w3.org/TR/css-multicol-1/#filling-columns>
        // <https://www.w3.org/TR/css-break-3/#breaking-boxes>
        let estimated_float_flow_extent =
            self.estimated_multicol_float_flow_extent(child_boxes, stylesheets, column_width);
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        let flow_units = self.estimated_multicol_flow_units(
            child_boxes,
            stylesheets,
            column_width,
            descendant_percentage_height_basis,
            false,
        );
        let balance_parallel_flow_height = if column_count > 1 {
            estimated_parallel_flow_height
        } else {
            // With one column there is no competing column height to balance.
            // Visible descendant overflow remains attached to its definite
            // principal fragment instead of enlarging that fragment's used
            // block size.
            // <https://www.w3.org/TR/css-multicol-1/#filling-columns>
            estimated_normal_flow_height
        };
        let estimated_content_height = estimated_normal_flow_height
            .max(balance_parallel_flow_height)
            .max(estimated_float_flow_extent);
        let minimum_simple_avoid_run_height = if child_boxes.len() == 2 && flow_units.len() == 2 {
            let simple_styles = child_boxes
                .iter()
                .filter_map(|child| {
                    child
                        .element_parts()
                        .filter(|(element, _, child_style, children)| {
                            children.is_empty()
                                && style_is_in_normal_flow(child_style)
                                && child_style.float == Float::None
                                && FragmentainerKind::Column.avoids_break_inside(child_style)
                                && matches!(
                                    element_layout_kind(element, child_style),
                                    ElementLayoutKind::BlockFlow
                                )
                        })
                        .map(|(_, _, child_style, _)| child_style)
                })
                .collect::<Vec<_>>();
            if simple_styles.len() == 2
                && (simple_styles[0].break_after.avoids_column()
                    || simple_styles[1].break_before.avoids_column())
            {
                minimum_honorable_avoid_run_height(&flow_units, available_page_height)
            } else {
                0.0
            }
        } else {
            0.0
        };
        let minimum_monolithic_column_height = self
            .estimated_multicol_monolithic_block_size(child_boxes, stylesheets, column_width)
            .max(minimum_simple_avoid_run_height);
        let minimum_forced_run_height = content_height
            .is_none()
            .then(|| preferred_forced_run_height(&flow_units))
            .flatten()
            .unwrap_or(0.0);
        let intrinsically_empty_column_set = estimated_content_height <= 0.01
            && minimum_monolithic_column_height <= 0.01
            && minimum_forced_run_height <= 0.01
            && !flow_units
                .iter()
                .any(|unit| unit.forced_before || unit.forced_after);
        let minimum_column_height = if intrinsically_empty_column_set {
            css::CSS_PX_TO_PT
        } else {
            style.line_height
        };
        let balance_height_limit = column_set_content_height
            .unwrap_or(available_page_height)
            .min(balanced_column_height_limit.unwrap_or(f32::INFINITY));
        let estimated_balanced_height = (estimated_content_height / column_count as f32)
            .max(minimum_column_height)
            .max(minimum_monolithic_column_height)
            .max(minimum_forced_run_height)
            .min(balance_height_limit);
        let sequential_auto_height_limit = (content_height.is_none()
            && style.column_fill == css::ColumnFill::Auto)
            .then(|| {
                used_max_height(style, PercentageBasis::definite(layout_pt(available_width)))
                    .map(SemanticLengthExt::points)
            })
            .flatten();
        let balances_definite_column_set = balance_definite_column_set
            && content_height.is_some()
            && !WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes();
        let needs_intrinsic_balance = matches!(
            style.column_fill,
            css::ColumnFill::Balance | css::ColumnFill::BalanceAll
        ) && (content_height.is_none()
            || balances_definite_column_set);
        let balanced_untrimmed_height = if needs_intrinsic_balance {
            self.converged_multicol_balance_height(MulticolBalanceSearchInput {
                element,
                style,
                stylesheets,
                child_boxes,
                column_width,
                column_count,
                descendant_percentage_height_basis,
                estimated_content_height,
                estimated_balanced_height,
                // Floats constrain the upper balance estimate through their
                // shelf extent, but remain fragmentable and therefore do not
                // impose a monolithic lower bound. The speculative fit pass
                // resolves their actual margins and deferred column reach.
                // <https://www.w3.org/TR/css-break-3/#monolithic>
                minimum_structural_column_height: minimum_monolithic_column_height,
                available_page_height: balance_height_limit,
                retain_available_height_when_unfit: balances_definite_column_set,
            })
        } else {
            estimated_balanced_height
        };
        // With an unconstrained auto block size, `column-fill:auto` fills
        // sequentially rather than balancing. The content establishes one
        // intrinsic column height; a parent fragmentainer or explicit
        // max-height may still impose the finite continuation limit below.
        // <https://www.w3.org/TR/css-multicol-1/#filling-columns>
        let sequential_auto_content_height = estimated_content_height
            .max(minimum_column_height)
            .max(minimum_monolithic_column_height)
            .max(minimum_forced_run_height);
        // An auto-height `column-fill:auto` set grows only until it reaches
        // the active parent fragmentainer. Its next anonymous column then
        // continues in the next available fragmentainer; treating the entire
        // intrinsic content height as one column loses those continuation
        // rows when the multicol box itself is paginated.
        // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
        let sequential_auto_column_height = column_set_content_height
            .or(sequential_auto_height_limit)
            .unwrap_or_else(|| sequential_auto_content_height.min(available_page_height));
        let nominal_column_height = match style.column_fill {
            css::ColumnFill::Auto => sequential_auto_column_height,
            css::ColumnFill::Balance | css::ColumnFill::BalanceAll => {
                if content_height.is_none() || balances_definite_column_set {
                    balanced_untrimmed_height
                } else {
                    column_set_content_height.unwrap_or(balanced_untrimmed_height)
                }
            }
        };
        let planned_trim_end_child_indices = self
            .estimated_multicol_text_box_trim_end_child_indices(
                style,
                child_boxes,
                stylesheets,
                column_width,
                nominal_column_height,
                descendant_percentage_height_basis,
            );
        let every_column_end_accepts_trim = planned_trim_end_child_indices
            .as_ref()
            .is_some_and(|indices| indices.len() >= column_count);
        let per_column_trim_end =
            if text_box_line_trim.trims_block_end && every_column_end_accepts_trim {
                text_box_line_trim.block_end
            } else {
                0.0
            };
        let balanced_height = (balanced_untrimmed_height - per_column_trim_end)
            .max(minimum_column_height)
            .min(balance_height_limit);
        let column_height = match style.column_fill {
            css::ColumnFill::Auto => sequential_auto_column_height,
            css::ColumnFill::Balance | css::ColumnFill::BalanceAll => {
                if content_height.is_none() || balances_definite_column_set {
                    balanced_height
                } else {
                    column_set_content_height.unwrap_or(balanced_height)
                }
            }
        }
        // An otherwise zero-sized column box still has a one-CSS-pixel
        // fragmentainer block size so overflowing content can make progress.
        // <https://www.w3.org/TR/css-break-3/#breaking-rules>
        .max(css::CSS_PX_TO_PT);

        let first_column_height = preferred_first_multicol_break(&flow_units, column_height)
            .unwrap_or(column_height)
            .min(
                if self.active_fragmentainer_kind() == FragmentainerKind::Column {
                    remaining_parent_fragmentainer_height
                } else {
                    column_height
                },
            )
            .max(css::CSS_PX_TO_PT);
        let relax_widows_orphans = first_overflow_boundary_is_avoided(&flow_units, column_height);
        // A scroll container and an unbreakable single line box are monolithic
        // fragmentation subjects. When either is the lone column-flow box,
        // its margin box overflows the originating column instead of being
        // graphically sliced into continuation columns; a scroll container's
        // own padding-edge clip still applies to its descendants.
        // <https://www.w3.org/TR/css-break-3/#possible-breaks>
        let single_unsplittable_overflow_root = child_boxes.len() == 1
            && child_boxes[0]
                .element_parts()
                .is_some_and(|(_, _, child_style, _)| {
                    style_is_in_normal_flow(child_style)
                        && child_style.float == Float::None
                        && style_clips_overflow(child_style)
                });
        let single_unbreakable_line_root = if column_height <= css::CSS_PX_TO_PT + 0.01
            && child_boxes.len() == 1
            && let Some((child_element, _, child_style, children)) = child_boxes[0].element_parts()
            && style_is_in_normal_flow(child_style)
            && child_style.float == Float::None
        {
            self.block_has_single_unbreakable_inline_line(
                child_element,
                child_style,
                children,
                column_width,
            )
        } else {
            false
        };
        let single_unsplittable_column_subject =
            single_unsplittable_overflow_root || single_unbreakable_line_root;
        let replay_oversized_flow_slices = !single_unsplittable_column_subject
            && (estimated_float_flow_extent > column_height * 1.5
                || flow_units
                    .iter()
                    .any(|unit| unit.block_size.points() > column_height * 1.5));

        let outer_snapshot = self.snapshot();
        let deferred_positioned_children_start = self.deferred_multicol_positioned_children.len();
        self.multicol_positioned_replay_capture_depth += 1;
        let page_size = outer_snapshot.current_page_context.size;
        let continuation_context = PageContext {
            size: page_size,
            margins: PageMargins::from_points(
                page_size.height() - previous_cursor_y,
                page_size.width() - previous_left - column_width,
                previous_cursor_y - column_height,
                previous_left,
            ),
            edges: PageBoxEdges::ZERO,
            rotation: outer_snapshot.current_page_context.rotation,
        };
        let first_context = PageContext {
            margins: PageMargins::from_points(
                page_size.height() - previous_cursor_y,
                page_size.width() - previous_left - column_width,
                previous_cursor_y - first_column_height,
                previous_left,
            ),
            ..continuation_context
        };

        self.pages.clear();
        self.page_names.clear();
        self.page_blanks.clear();
        self.page_named_strings.clear();
        self.page_running_elements.clear();
        self.current_page = page_for_context(first_context);
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.current_page_selected_name = None;
        self.current_page_context = first_context;
        self.fragmentainer_override = Some(FragmentainerOverride {
            kind: FragmentainerKind::Column,
            initial_context: first_context,
            // The first column starts in the remaining block space at this
            // point in outer flow. Every later anonymous column starts at the
            // nominal column height, even when it is still in the first
            // visual column row.
            // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
            initial_fragmentainer_count: 1,
            context: continuation_context,
            relax_widows_orphans,
        });
        self.cursor_y = previous_cursor_y;
        self.content_left = previous_left;
        self.content_right = previous_left + column_width;
        self.fragment_top_offsets.clear();
        self.positioned_layers.clear();
        self.fixed_layers.clear();
        self.current_page_named_strings.clear();
        self.current_page_running_elements.clear();
        self.content_logical_inline_size_stack.push(column_width);
        self.multicol_column_containing_blocks
            .push(MulticolColumnContainingBlock {
                inline_size: LogicalInlineContentSize::new(content_box_pt(column_width)),
                content_left: previous_left,
            });
        self.definite_block_size_stack
            .push(descendant_percentage_height_basis.basis());
        self.truncate_page_start_margins = false;
        self.multicol_text_box_trim_end_child_indices = planned_trim_end_child_indices;

        // Keep the source children in one block-flow phase so class A break
        // opportunities span adjacent siblings. Rendering each child through
        // a separate entry point would lose the rollback candidate needed by
        // `break-before/after: avoid`.
        // <https://www.w3.org/TR/css-break-3/#break-between>
        self.push_float_context();
        self.layout_block_flow_children_phase(Box::new(BlockFlowChildrenPhaseInput {
            fragmentainer_kind: FragmentainerKind::Column,
            element,
            style,
            stylesheets,
            child_boxes: Some(child_boxes),
            can_collapse_start_margin: false,
            can_collapse_end_margin: false,
            applied_start_margin: layout_pt(0.0),
            clearance_consumed_adjoining_start_margin: false,
            starts_at_page_top: false,
            laid_out_column_children: false,
            use_box_inline_items: false,
            run_in_inline_items_laid_out: false,
            use_ordered_mixed_flow: false,
            has_preceding_inline_flow_content: false,
            definite_content_height: Some(column_height),
            descendant_percentage_height_basis: descendant_percentage_height_basis.map(|height| {
                block_size_percentage_basis_from_points(
                    Some(height),
                    BlockSizeBasisSource::ContainingBlock,
                )
            }),
        }));
        self.pop_float_context();
        // Out-of-flow descendants affect column balancing and the multicol
        // container's block size. Their positioned layout records the last
        // anonymous column page required even when normal flow ends earlier;
        // materialize that structural span so queued descendant fragments can
        // participate in the committed column sequence. Direct positioned
        // children remain detached in `positioned_layers` and are replayed
        // once against the principal multicol containing block below.
        // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
        self.materialize_pending_positioned_page_span();
        // A fixed promoted spanner can defer descendant overflow paint to
        // outer columns that normal flow itself never reaches. Materialize
        // those anonymous column pages before collecting the speculative
        // column fragments; the outer snapshot is restored immediately after
        // collection, so these pages remain an implementation detail.
        // <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
        while let Some(last_deferred_page) = self
            .pending_paint_fragments
            .iter()
            .map(|fragment| fragment.page_index)
            .max()
            && self.pages.len() < last_deferred_page
        {
            if !self.current_page_has_content() {
                self.mark_current_page_flow_content();
            }
            self.push_page();
        }
        self.apply_pending_fragments_for_current_page();
        // Direct positioned descendants are out of normal flow. An auto inset
        // uses its static position in the final source column, while definite
        // insets resolve against the principal multicol containing block and
        // must not inherit the final temporary column's translation.
        // Capture the distinction before restoring the outer page.
        // <https://www.w3.org/TR/css-position-3/#static-position>
        // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
        let mut source_positioned_children = {
            let sibling_tags = element_sibling_signature_list(element);
            let mut element_index = 0usize;
            let mut positioned = Vec::new();
            for child in &element.children {
                let NodeKind::Element(child_element) = &child.kind else {
                    continue;
                };
                let signature = ElementSignature::with_sibling_list(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = self.style_for_layout_element_with_parent_font_metrics(
                    child_element,
                    signature.clone(),
                    stylesheets,
                    Some(style),
                );
                if !matches!(child_style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                let static_position_depends_on_final_column =
                    (child_style.box_values.inset_left.is_auto()
                        && child_style.box_values.inset_right.is_auto())
                        || (child_style.box_values.inset_top.is_auto()
                            && child_style.box_values.inset_bottom.is_auto());
                let positioning_containing_block =
                    PositionedContainingBlockMode::for_element(element, style)
                        .zip(self.containing_blocks.last().copied());
                let source_static_rect = if static_position_depends_on_final_column {
                    PositionedChildStaticRect::new(
                        self.content_left,
                        self.content_right,
                        self.cursor_y,
                    )
                } else {
                    PositionedChildStaticRect::new(
                        previous_left,
                        previous_left + available_width,
                        previous_cursor_y,
                    )
                };
                let fragment = PositionedFragmentReplay::unfragmented(
                    source_static_rect,
                    positioning_containing_block,
                )
                .committed_to_fragmentainer(
                    if static_position_depends_on_final_column {
                        self.pages.len()
                    } else {
                        0
                    },
                    PaintTranslation::identity(),
                    None,
                );
                positioned.push((child_element.clone(), signature, child_style, fragment));
            }
            positioned
        };
        // Column pages are an implementation detail, so positioned descendants
        // must not be bound to whichever temporary page happened to be active
        // when their source was encountered. Direct out-of-flow children are
        // replayed once against the real multicol containing block below.
        self.positioned_layers.clear();
        self.fixed_layers.clear();

        let committed_counter_set = self.counter_set.clone();
        let committed_quote_depth = self.quote_depth;
        let committed_next_assignment_id = self.next_assignment_id;
        // Temporary column pages share the same page-top coordinate. Preserve
        // the actual used block size of the final temporary column even when
        // earlier columns were materialized; a paginated `column-fill:auto`
        // row may end halfway down its last column.
        // <https://www.w3.org/TR/css-multicol-1/#filling-columns>
        let actual_final_column_used_height =
            (previous_cursor_y - self.cursor_y).clamp(0.0, column_height);
        let source_used_column_height = if self.pages.is_empty() {
            actual_final_column_used_height
        } else {
            column_height
        };
        let mut column_pages = self.pages.clone();
        column_pages.push(self.current_page.clone());
        let structurally_occupied_columns = column_pages.len();
        let trailing_column_was_never_entered =
            !self.current_page_has_flow_content && self.current_page.paint_fragment().is_empty();
        let column_fragments = column_pages
            .iter()
            .map(Page::paint_fragment)
            .collect::<Vec<_>>();
        let column_set_has_paint = column_fragments.iter().any(|fragment| !fragment.is_empty());
        let estimated_occupied_columns = ((estimated_content_height - 0.01).max(0.0)
            / column_height)
            .ceil()
            .max(1.0) as usize;
        // This baseline belongs to the final committed line in the temporary
        // column sequence. `restore` below deliberately discards that
        // sequence, so retain the source-local offset now for atomic callers.
        let committed_final_in_flow_baseline = self
            .last_in_flow_line_baseline_y
            .map(|baseline| layout_pt((previous_cursor_y - baseline).max(0.0)));
        self.restore(outer_snapshot);
        self.multicol_positioned_replay_capture_depth -= 1;
        self.counter_set = committed_counter_set;
        self.quote_depth = committed_quote_depth;
        self.next_assignment_id = committed_next_assignment_id;

        let estimated_wrapped_row_count = estimated_occupied_columns.div_ceil(column_count);
        let estimated_wrapped_block_size = estimated_wrapped_row_count as f32 * column_height
            + estimated_wrapped_row_count.saturating_sub(1) as f32 * row_gap;
        let wrapped_rows_cross_parent_fragmentainer = wrap_column_rows
            && self.active_fragmentainer_kind() == FragmentainerKind::Column
            && estimated_wrapped_block_size > remaining_parent_fragmentainer_height + 0.01;
        let paginate_column_rows = fragments_across_parent_fragmentainer
            || wrapped_rows_cross_parent_fragmentainer
            || (content_height.is_none()
                && !wrap_column_rows
                && match self.active_fragmentainer_kind() {
                    // Extra columns generated by an auto-block-size nested
                    // multicol continue in the outer column fragmentainer;
                    // they are not continuous-context overflow columns.
                    // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>
                    FragmentainerKind::Column => true,
                    FragmentainerKind::Page => {
                        style.column_fill == css::ColumnFill::Auto
                            || column_height >= available_page_height - 0.01
                    }
                });
        let structural_rows_cross_parent_fragmentainer = fragments_across_parent_fragmentainer
            || (self.active_fragmentainer_kind() == FragmentainerKind::Column
                && (!wrap_column_rows || wrapped_rows_cross_parent_fragmentainer))
            || column_height >= available_page_height - 0.01;
        let mut painted_columns = if paginate_column_rows {
            0
        } else {
            structurally_occupied_columns
        };
        let mut painted_columns_in_row = if paginate_column_rows {
            0
        } else {
            structurally_occupied_columns
        };
        let mut painted_row = 0usize;
        let mut row_top = previous_cursor_y;
        let mut row_left = previous_left;
        let mut row_rule_paint_point = self
            .current_page
            .paint_band_insertion_point(PaintBand::InFlowBlock);
        let multicol_axes = FlowAxes::for_style(style);
        let multicol_block_axis = WritingModeAxes::new(style.writing_mode, style.direction)
            .physical_axis(LogicalAxis::Block);
        for (fragment_index, fragment) in column_fragments.into_iter().enumerate() {
            // Out-of-flow descendants are positioned against the multicol
            // container, not the shortened flow extent selected for an early
            // first-column break, so every replay slice uses the nominal
            // column height.
            let source_fragment_height = column_height;
            let fragment_block_extent = fragment
                .bounds()
                .map(|bounds| (previous_cursor_y - bounds.y()).max(source_fragment_height))
                .unwrap_or(source_fragment_height);
            // The numerical one-CSS-pixel progress capacity of a zero-height
            // column keeps fragmentation finite, but is not an authored
            // clipping edge. An atomic overflow subject, including a flex
            // line, therefore retains its committed overflow instead of
            // becoming a one-pixel stripe during multicol projection.
            // <https://www.w3.org/TR/css-break-3/#breaking-rules>
            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
            let zero_capacity_column_fragment_overflows = column_height <= css::CSS_PX_TO_PT + 0.01
                && fragment_block_extent > column_height + 0.01;
            // A block that is substantially taller than its temporary column
            // can remain in one paint fragment even though its background and
            // descendants must be replayed through later columns. Small glyph
            // ink overflow must not synthesize another column.
            // Once flow layout has already materialized continuation pages,
            // that structural sequence is authoritative. Do not reinterpret
            // backgrounds, decorations, or other visual overflow on those
            // fragments as more content slices and manufacture extra columns.
            // <https://www.w3.org/TR/css-break-3/#break-decoration>
            let slice_count = if replay_oversized_flow_slices
                // A zero-height column's atomic overflow is committed as one
                // source fragment per flex item. Those records may already
                // occupy multiple temporary pages, but each still needs its
                // clipped synthetic tails projected through the following
                // anonymous columns. Ordinary multi-page flow remains
                // authoritative and is never re-sliced here.
                && (structurally_occupied_columns == 1
                    || zero_capacity_column_fragment_overflows)
                && fragment_block_extent > source_fragment_height * 1.5
            {
                (fragment_block_extent / column_height).ceil().max(1.0) as usize
            } else {
                1
            };
            for slice_index in 0..slice_count {
                let slice_height = if slice_index == 0 {
                    source_fragment_height
                } else {
                    column_height
                };
                let lifted_block_offset = if slice_index == 0 {
                    0.0
                } else {
                    source_fragment_height + (slice_index - 1) as f32 * column_height
                };
                let source_slice = LogicalRect {
                    origin: LogicalPoint {
                        inline: 0.0,
                        block: lifted_block_offset,
                    },
                    size: LogicalSize {
                        inline: column_width,
                        block: slice_height,
                    },
                };
                // A column fragments overflow in its block axis. Direct
                // over-wide and orthogonal children preserve their authored
                // inline-axis overflow into the gap or adjacent column;
                // ordinary column content remains isolated to its column
                // slice. The height always remains the anonymous column's
                // fragmentation-axis extent.
                // <https://www.w3.org/TR/css-multicol-1/#overflow-inside-multicol>
                let target_fragment = fragment_index + slice_index;
                let target_row = if paginate_column_rows || wrap_column_rows {
                    target_fragment / column_count
                } else {
                    0
                };
                while painted_row < target_row {
                    let rule_primitives = if painted_columns_in_row > 0 {
                        let topology = multicol_gap_topology_for_row(MulticolGapTopologyRowInput {
                            style,
                            content_left: row_left,
                            content_top: row_top,
                            column_height,
                            inline_size: available_width,
                            column_width,
                            column_gap: gap,
                            column_count: multicol_decorated_column_count(
                                style,
                                painted_columns_in_row,
                                column_count,
                            ),
                            row: MulticolumnRowIndex::new(painted_row),
                            previous_row_gap: (painted_row > 0).then_some(row_gap),
                            following_row_gap: (wrap_column_rows && !paginate_column_rows)
                                .then_some(row_gap),
                            row_rule_count: (wrap_column_rows && !paginate_column_rows)
                                .then_some(estimated_wrapped_row_count.saturating_sub(1)),
                        });
                        gap_decoration_primitives_for_topology(style, &topology)
                    } else {
                        Vec::new()
                    };
                    if !rule_primitives.is_empty() {
                        self.current_page.insert_primitives_at_paint_band_point(
                            row_rule_paint_point,
                            rule_primitives,
                        );
                    }
                    if !wrap_column_rows || paginate_column_rows {
                        self.push_page();
                        row_rule_paint_point = self
                            .current_page
                            .paint_band_insertion_point(PaintBand::InFlowBlock);
                    }
                    painted_row += 1;
                    painted_columns_in_row = 0;
                    if wrap_column_rows && !paginate_column_rows {
                        row_top =
                            previous_cursor_y - painted_row as f32 * (column_height + row_gap);
                        row_left = previous_left;
                    } else {
                        row_top = self.page_top();
                        row_left = self.content_left;
                    }
                }
                let row_fragment = if paginate_column_rows || wrap_column_rows {
                    target_fragment % column_count
                } else {
                    target_fragment
                };
                let visual_column_offset = if style.direction == Direction::Rtl {
                    if row_fragment < column_count {
                        column_count.saturating_sub(1).saturating_sub(row_fragment) as isize
                    } else {
                        -((row_fragment - column_count + 1) as isize)
                    }
                } else {
                    row_fragment as isize
                };
                let destination_inline_extent = available_width
                    .max((row_fragment + 1) as f32 * column_width + row_fragment as f32 * gap);
                let projection = FragmentainerProjection::new(FragmentainerProjectionInput {
                    axes: multicol_axes,
                    source_origin: PageTopPoint::new(previous_left, previous_cursor_y),
                    source_extent: LogicalSize {
                        inline: column_width,
                        block: lifted_block_offset + slice_height,
                    },
                    source_slice,
                    destination_origin: PageTopPoint::new(row_left, row_top),
                    destination_extent: LogicalSize {
                        inline: destination_inline_extent,
                        block: slice_height,
                    },
                    destination_slice: LogicalRect {
                        origin: LogicalPoint {
                            inline: row_fragment as f32 * (column_width + gap),
                            block: 0.0,
                        },
                        size: source_slice.size,
                    },
                    destination_page_area: PageTopRect::new(
                        self.page_left(),
                        self.page_top(),
                        self.page_area_width(),
                        self.page_area_height(),
                    ),
                });
                // A direct positioned descendant captured on this temporary
                // page owns the same source-to-destination projection as the
                // normal-flow paint collected from it. This matters for an
                // auto inset: all temporary pages share a local X/Y origin,
                // while their committed columns do not.
                // <https://www.w3.org/TR/css-position-3/#static-position>
                // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
                if slice_index == 0 {
                    for (_, _, _, positioned_fragment) in &mut source_positioned_children {
                        if positioned_fragment.owns_source_fragmentainer(fragment_index) {
                            *positioned_fragment = positioned_fragment
                                .clone()
                                .projected_to_destination(projection.destination_translation());
                        }
                    }
                    self.project_deferred_multicol_positioned_fragments(
                        deferred_positioned_children_start,
                        fragment_index,
                        projection.destination_translation(),
                    );
                }
                self.retain_deferred_multicol_positioned_candidate(
                    deferred_positioned_children_start,
                    projection.source_clip(),
                    layout_pt(fragment_index as f32 * column_height),
                    layout_pt((fragment_index + 1) as f32 * column_height),
                    projection.destination_translation(),
                );
                let fragment = if single_unsplittable_column_subject
                    // Only the source fragment's first replay owns the
                    // atomic overflow. Later synthetic slices exist to
                    // project the remaining source ranges through the
                    // anonymous columns, and must retain their range clip.
                    || (zero_capacity_column_fragment_overflows && slice_index == 0)
                {
                    fragment.clone()
                } else {
                    fragment
                        .clone()
                        .with_primitives_clipped_to_physical_axis_range_preserving_cross_axis_overflow(
                            multicol_block_axis,
                            projection.source_clip(),
                            slice_count > 1,
                        )
                };
                // The first column's normal-flow inline overflow remains
                // visible, but a continuation captured on a later temporary
                // column page belongs to its committed column rectangle. In
                // a vertical writing mode, preserving that continuation's
                // physical cross-axis overflow would paint it beyond the
                // multicol container's inline end after destination
                // translation. Keep the entire committed source rectangle
                // for later column pages before projecting it.
                // <https://www.w3.org/TR/css-multicol-1/#overflow-inside-multicol>
                let fragment = if continuation_column_fragment_requires_full_clip(
                    fragment_index,
                    style.writing_mode,
                    style.direction,
                ) {
                    fragment.with_primitives_clipped_to_rect_preserving_structure(
                        projection.source_clip(),
                    )
                } else {
                    fragment
                };
                if fragment.is_empty() {
                    continue;
                }
                let fragment = fragment
                    .with_primitives_clipped_to_physical_axis_range_preserving_cross_axis_overflow(
                        multicol_block_axis,
                        projection.destination_page_clip_in_source_space(),
                        false,
                    )
                    // Column inline overflow remains visible across gaps and
                    // neighboring columns, but the finished anonymous column
                    // fragment is still contained by the outer page area—the
                    // initial containing block for that page fragment.
                    // <https://www.w3.org/TR/css-page-3/#page-model>
                    .with_primitives_clipped_to_rect_preserving_structure(
                        projection.destination_page_clip_in_source_space(),
                    );
                if fragment.is_empty() {
                    continue;
                }
                self.current_page
                    .append_paint_fragment_owned(fragment, projection.destination_translation());
                self.mark_current_page_flow_content();
                if visual_column_offset >= 0 {
                    painted_columns = painted_columns.max(visual_column_offset as usize + 1);
                    painted_columns_in_row =
                        painted_columns_in_row.max(visual_column_offset as usize + 1);
                }
            }
        }
        if paginate_column_rows
            && structural_rows_cross_parent_fragmentainer
            && structurally_occupied_columns > 0
            && !column_set_has_paint
        {
            // Empty or non-painting boxes still occupy anonymous columns and
            // therefore page rows. Advance the outer fragmentainer from the
            // structural column sequence even when no fragment ink reached
            // the replay loop above; following normal flow then starts after
            // the actually used part of the final row.
            // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            let final_structural_row =
                structurally_occupied_columns.saturating_sub(1) / column_count;
            while painted_row < final_structural_row {
                let structural_row_start = painted_row * column_count;
                let structural_columns_in_row = structurally_occupied_columns
                    .saturating_sub(structural_row_start)
                    .min(column_count);
                let columns_in_row = painted_columns_in_row.max(structural_columns_in_row);
                if columns_in_row > 0 {
                    let primitives = multicol_gap_decoration_primitives(
                        style,
                        row_left,
                        row_top,
                        row_top - column_height,
                        column_width,
                        gap,
                        multicol_decorated_column_count(style, columns_in_row, column_count),
                    );
                    self.current_page
                        .insert_primitives_at_paint_band_point(row_rule_paint_point, primitives);
                    // Anonymous columns are real fragmentainers even when all
                    // of their boxes are non-painting. Preserve this completed
                    // outer page instead of letting the empty-page guard reuse
                    // it for the following structural row.
                    // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                    self.mark_current_page_flow_content();
                }
                self.push_page();
                row_rule_paint_point = self
                    .current_page
                    .paint_band_insertion_point(PaintBand::InFlowBlock);
                painted_row += 1;
                painted_columns_in_row = 0;
                row_top = self.page_top();
                row_left = self.content_left;
            }
            let final_row_start = final_structural_row * column_count;
            let final_structural_columns = structurally_occupied_columns
                .saturating_sub(final_row_start)
                .min(column_count);
            painted_columns_in_row = painted_columns_in_row.max(final_structural_columns);
            painted_columns = painted_columns.max(structurally_occupied_columns);
        }
        if painted_columns > 0 {
            self.mark_current_page_flow_content();
        }
        for (child_element, signature, child_style, fragment) in source_positioned_children {
            let owning_fragment_clip =
                PageTopRect::new(previous_left, row_top, available_width, column_height)
                    .paint_clip();
            self.defer_multicol_positioned_fragment_element(
                &child_element,
                &signature,
                child_style,
                fragment.with_destination_clip(owning_fragment_clip),
            );
        }
        self.replay_deferred_multicol_positioned_children(deferred_positioned_children_start);
        self.content_left = previous_left;
        self.content_right = previous_right;
        let used_column_set_height = if content_height.is_none()
            && style.column_fill == css::ColumnFill::Auto
            && paginate_column_rows
            && !column_set_has_paint
        {
            actual_final_column_used_height
        } else if sequential_auto_height_limit.is_some() || intrinsically_empty_column_set {
            source_used_column_height
        } else if balances_definite_column_set && let Some(used_height) = column_set_content_height
        {
            used_height
        } else {
            column_height
        };
        self.cursor_y = row_top - used_column_set_height;
        let decoration_structural_columns = if needs_intrinsic_balance {
            // A balanced auto-height set generates the planned balanced
            // column sequence. A temporary continuation page created by an
            // exact block-end break is rollback bookkeeping, not an anonymous
            // column and therefore contributes no far-edge gap rule.
            estimated_occupied_columns.min(column_count)
        } else {
            structurally_occupied_columns
                .saturating_sub(usize::from(trailing_column_was_never_entered))
                .max(estimated_occupied_columns)
        }
        .max(1);
        let structural_columns_in_current_row = if wrap_column_rows || paginate_column_rows {
            decoration_structural_columns
                .saturating_sub(painted_row * column_count)
                .min(column_count)
        } else {
            decoration_structural_columns
        };
        let occupied_decorated_columns = painted_columns_in_row
            .max(structural_columns_in_current_row)
            .max(1);
        let available_decorated_columns = if wrap_column_rows || paginate_column_rows {
            column_count
        } else {
            occupied_decorated_columns.max(column_count)
        };
        let topology = multicol_gap_topology_for_row(MulticolGapTopologyRowInput {
            style,
            content_left: row_left,
            content_top: row_top,
            column_height: (row_top - self.cursor_y).max(0.0),
            inline_size: available_width,
            column_width,
            column_gap: gap,
            column_count: multicol_decorated_column_count(
                style,
                occupied_decorated_columns,
                available_decorated_columns,
            ),
            row: MulticolumnRowIndex::new(painted_row),
            previous_row_gap: (wrap_column_rows && !paginate_column_rows && painted_row > 0)
                .then_some(row_gap),
            following_row_gap: None,
            row_rule_count: (wrap_column_rows && !paginate_column_rows)
                .then_some(estimated_wrapped_row_count.saturating_sub(1)),
        });
        let rule_primitives = gap_decoration_primitives_for_topology(style, &topology);
        self.current_page
            .insert_primitives_at_paint_band_point(row_rule_paint_point, rule_primitives);
        MulticolFlowLayoutOutcome::column_set(
            // `row_top` incorporates prior wrapped rows; preserve that
            // committed advance as well as the final row's used height rather
            // than relying on a transient cursor after replay.
            layout_pt((previous_cursor_y - row_top).max(0.0) + used_column_set_height),
            committed_final_in_flow_baseline,
        )
    }

    /// Find the smallest balanced column block size that fits the source into
    /// the requested number of columns.
    ///
    /// CSS Multicol balancing is defined by the actual break opportunities,
    /// not by dividing an intrinsic-height estimate. Each probe therefore runs
    /// the real block fragmentation algorithm in an isolated snapshot. After
    /// a bounded binary search, the caller performs one committed paint pass
    /// at the converged size.
    /// <https://www.w3.org/TR/css-multicol-1/#filling-columns>
    /// <https://www.w3.org/TR/css-break-3/#breaking-rules>
    fn converged_multicol_balance_height(
        &mut self,
        input: MulticolBalanceSearchInput<'_, '_>,
    ) -> f32 {
        let balance_epsilon =
            if !WritingModeAxes::new(input.style.writing_mode, input.style.direction)
                .swaps_physical_axes()
            {
                MULTICOL_BALANCE_EPSILON
            } else {
                css::CSS_PX_TO_PT * 0.25
            };
        // A one-column set has no competing column heights to balance. Nested
        // sets reached during a speculative outer probe likewise use their
        // bounded estimate so probe cost grows with tree size rather than
        // exponentially with nesting depth.
        // <https://www.w3.org/TR/css-multicol-1/#filling-columns>
        if input.column_count <= 1 || self.multicol_balance_probe_depth > 0 {
            return input.estimated_balanced_height;
        }
        // Speculative layout cost must remain independent of authored numeric
        // magnitude. Once estimated overflow exceeds the bounded replay window
        // used by the committed column canvas, probing the same billion-pixel
        // subtree cannot select a visually distinguishable finite candidate.
        // Keep one committed pass and use the already clamped structural
        // estimate for these pathological-but-valid lengths.
        // <https://www.w3.org/TR/css-multicol-1/#filling-columns>
        if input.estimated_content_height
            > input.available_page_height * MAX_MULTICOL_BALANCE_PROBE_FRAGMENTAINERS as f32
        {
            return input
                .estimated_balanced_height
                .min(input.available_page_height)
                .max(css::CSS_PX_TO_PT);
        }
        let mut lower = input
            .minimum_structural_column_height
            .min(input.available_page_height)
            .max(css::CSS_PX_TO_PT);
        let mut upper = input
            .estimated_content_height
            .max(input.estimated_balanced_height)
            .max(input.style.line_height)
            .min(input.available_page_height)
            .max(lower);
        // When monolithic content already clamps the lower bound to the
        // available fragmentainer, the search interval is closed. Running a
        // full speculative layout cannot refine that answer and is especially
        // expensive for stress cases with billion-pixel descendants.
        if upper - lower <= balance_epsilon {
            return upper.max(css::CSS_PX_TO_PT);
        }
        if !self.multicol_balance_probe_fits(MulticolBalanceProbeInput {
            element: input.element,
            style: input.style,
            stylesheets: input.stylesheets,
            child_boxes: input.child_boxes,
            column_width: input.column_width,
            candidate_height: upper,
            column_count: input.column_count,
            descendant_percentage_height_basis: input.descendant_percentage_height_basis,
            relax_widows_orphans: false,
        }) {
            // Forced breaks or parallel/nested flows can require more
            // anonymous columns than the requested row contains regardless
            // of candidate height. In a definite column set that means there
            // is no smaller balanced height that fits this row; retain the
            // authored maximum rather than collapsing to an estimate that is
            // known not to fit.
            // <https://www.w3.org/TR/css-multicol-1/#filling-columns>
            return if input.retain_available_height_when_unfit {
                input.available_page_height
            } else {
                input.estimated_balanced_height
            };
        }

        // Eighteen probes provide better than 1/128 CSS-pixel precision for
        // normal page sizes; stop earlier once the interval is negligible.
        for _ in 0..18 {
            if upper - lower <= balance_epsilon {
                break;
            }
            let candidate = (lower + upper) * 0.5;
            if self.multicol_balance_probe_fits(MulticolBalanceProbeInput {
                element: input.element,
                style: input.style,
                stylesheets: input.stylesheets,
                child_boxes: input.child_boxes,
                column_width: input.column_width,
                candidate_height: candidate,
                column_count: input.column_count,
                descendant_percentage_height_basis: input.descendant_percentage_height_basis,
                relax_widows_orphans: false,
            }) {
                upper = candidate;
            } else {
                lower = candidate;
            }
        }
        upper
            .min(input.available_page_height)
            .max(css::CSS_PX_TO_PT)
    }

    fn multicol_balance_probe_fits(&mut self, input: MulticolBalanceProbeInput<'_, '_>) -> bool {
        let snapshot = self.snapshot();
        let page_size = snapshot.current_page_context.size;
        let previous_cursor_y = self.cursor_y;
        let previous_left = self.content_left;
        let candidate_height = input.candidate_height.max(css::CSS_PX_TO_PT);
        let context = PageContext {
            size: page_size,
            margins: PageMargins::from_points(
                page_size.height() - previous_cursor_y,
                page_size.width() - previous_left - input.column_width,
                previous_cursor_y - candidate_height,
                previous_left,
            ),
            edges: PageBoxEdges::ZERO,
            rotation: snapshot.current_page_context.rotation,
        };

        self.pages.clear();
        self.page_names.clear();
        self.page_blanks.clear();
        self.page_named_strings.clear();
        self.page_running_elements.clear();
        self.pending_paint_fragments.clear();
        self.pending_page_side_effects.clear();
        self.current_page = page_for_context(context);
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.current_page_selected_name = None;
        self.current_page_context = context;
        self.fragmentainer_override = Some(FragmentainerOverride {
            kind: FragmentainerKind::Column,
            initial_context: context,
            initial_fragmentainer_count: input.column_count,
            context,
            relax_widows_orphans: input.relax_widows_orphans,
        });
        self.cursor_y = previous_cursor_y;
        self.content_left = previous_left;
        self.content_right = previous_left + input.column_width;
        self.fragment_top_offsets.clear();
        self.positioned_layers.clear();
        self.fixed_layers.clear();
        self.current_page_named_strings.clear();
        self.current_page_running_elements.clear();
        self.content_logical_inline_size_stack
            .push(input.column_width);
        self.multicol_column_containing_blocks
            .push(MulticolColumnContainingBlock {
                inline_size: LogicalInlineContentSize::new(content_box_pt(input.column_width)),
                content_left: previous_left,
            });
        self.definite_block_size_stack
            .push(input.descendant_percentage_height_basis.basis());
        self.truncate_page_start_margins = false;
        self.multicol_text_box_trim_end_child_indices = None;

        self.push_float_context();
        self.multicol_balance_probe_depth += 1;
        self.layout_block_flow_children_phase(Box::new(BlockFlowChildrenPhaseInput {
            fragmentainer_kind: FragmentainerKind::Column,
            element: input.element,
            style: input.style,
            stylesheets: input.stylesheets,
            child_boxes: Some(input.child_boxes),
            can_collapse_start_margin: false,
            can_collapse_end_margin: false,
            applied_start_margin: layout_pt(0.0),
            clearance_consumed_adjoining_start_margin: false,
            starts_at_page_top: false,
            laid_out_column_children: false,
            use_box_inline_items: false,
            run_in_inline_items_laid_out: false,
            use_ordered_mixed_flow: false,
            has_preceding_inline_flow_content: false,
            definite_content_height: Some(candidate_height),
            descendant_percentage_height_basis: input.descendant_percentage_height_basis.map(
                |height| {
                    block_size_percentage_basis_from_points(
                        Some(height),
                        BlockSizeBasisSource::ContainingBlock,
                    )
                },
            ),
        }));
        self.multicol_balance_probe_depth -= 1;
        self.pop_float_context();
        let normal_flow_columns = self.pages.len() + usize::from(self.current_page_has_content());
        // Definite principals and positioned descendants can leave their
        // normal-flow cursor in the originating column while assigning paint
        // to later anonymous columns. Those assignments are structural input
        // to balancing even though speculative probes do not materialize the
        // pending pages themselves.
        // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
        let deferred_paint_columns = self
            .pending_paint_fragments
            .iter()
            .map(|fragment| fragment.page_index + 1)
            .chain(
                self.positioned_layers
                    .iter()
                    .map(|layer| layer.page_index + 1),
            )
            .max()
            .unwrap_or(0);
        let used_columns = normal_flow_columns.max(deferred_paint_columns);
        // A line that cannot break may paint past a tiny probe column without
        // allocating another fragmentainer. That is still an overflow, not a
        // valid balanced height: CSS Multicol balances the actual fragment
        // contents, including an unbreakable line box's block size.
        // <https://www.w3.org/TR/css-multicol-1/#column-balancing>
        let overflows_current_column = self.current_page_has_content()
            && self.cursor_y < self.page_bottom() - MULTICOL_BALANCE_EPSILON;
        let fits = used_columns <= input.column_count && !overflows_current_column;
        self.restore(snapshot);
        fits
    }

    /// Select direct children that end an anonymous column for text-box trim.
    ///
    /// Each column box is a fragmentation container, so `trim-end` applies to
    /// its last formatted line rather than only to the final line of the whole
    /// multicol element. The planner supplies these endpoints to the one
    /// committed child-flow pass.
    /// <https://www.w3.org/TR/css-inline-3/#text-box-trim>
    fn estimated_multicol_text_box_trim_end_child_indices(
        &mut self,
        style: &ComputedStyle,
        child_boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        column_width: f32,
        column_height: f32,
        descendant_percentage_height_basis: MulticolDescendantPercentageBasis,
    ) -> Option<Vec<usize>> {
        if !self
            .effective_text_box_line_trim_for_style(style)
            .trims_block_end
        {
            return None;
        }
        let mut indices = Vec::new();
        let mut offset = 0.0f32;
        let mut previous_flow_index = None;
        let mut previous_flow_accepts_trim = false;
        for (index, child) in child_boxes.iter().enumerate() {
            let Some((element, _, child_style, children)) = child.element_parts() else {
                continue;
            };
            if !style_is_in_normal_flow(child_style) || child_style.float != Float::None {
                continue;
            }
            let metrics = used_box_metrics(
                child_style,
                PercentageBasis::definite(layout_pt(column_width)),
            );
            let definite_height = descendant_percentage_height_basis.and_then(|basis| {
                used_content_box_height_or_auto_with_basis(
                    child_style,
                    PercentageBasis::definite(content_box_pt(basis)),
                    metrics.vertical_non_content_length(),
                )
                .map(|height| {
                    constrain_content_height(
                        child_style,
                        height,
                        PercentageBasis::definite(layout_pt(column_width)),
                    )
                    .points()
                        + metrics.vertical_non_content_length().points()
                        + metrics.margin.top.points()
                        + metrics.margin.bottom.points()
                })
            });
            let height = definite_height
                .or_else(|| {
                    self.estimate_element_height(
                        element,
                        child_style,
                        stylesheets,
                        column_width,
                        Some(children),
                    )
                })
                .unwrap_or(child_style.line_height)
                .max(0.0);
            if offset > 0.01
                && offset + height > column_height + 0.01
                && let Some(previous) = previous_flow_index
            {
                if previous_flow_accepts_trim {
                    indices.push(previous);
                }
                offset = 0.0;
            }
            offset += height;
            previous_flow_index = Some(index);
            previous_flow_accepts_trim =
                definition_list_item_style_allows_text_box_trim(child_style, false);
            if offset >= column_height - 0.01 {
                if previous_flow_accepts_trim {
                    indices.push(index);
                }
                offset = 0.0;
            }
        }
        if let Some(last) = previous_flow_index
            && previous_flow_accepts_trim
            && !indices.contains(&last)
        {
            indices.push(last);
        }
        (!indices.is_empty()).then_some(indices)
    }

    /// Estimate the block-axis extent contributed by floats that belong to
    /// this multicol formatting context.
    ///
    /// The value is kept separate from normal-flow height because floats are
    /// taken out of flow, yet their boxes can still fragment across anonymous
    /// columns. Descendant floats are included only through boxes that do not
    /// establish an intervening formatting context.
    /// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
    /// <https://www.w3.org/TR/css-break-3/#breaking-boxes>
    fn estimated_multicol_float_flow_extent(
        &mut self,
        boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> f32 {
        let mut floats = Vec::new();
        let mut normal_flow_offset = 0.0;
        self.collect_estimated_multicol_floats(
            boxes,
            stylesheets,
            available_width,
            &mut normal_flow_offset,
            &mut floats,
        );
        let mut total_height = 0.0f32;
        let mut band_width = 0.0f32;
        let mut band_height = 0.0f32;
        for float in floats {
            total_height = total_height.max(float.block_offset.points());
            let outer_width = float.outer_width.points().min(available_width).max(0.0);
            if band_width > 0.01 && band_width + outer_width > available_width + 0.01 {
                total_height += band_height;
                band_width = 0.0;
                band_height = 0.0;
            }
            band_width += outer_width;
            band_height = band_height.max(float.outer_height.points());
        }
        total_height + band_height
    }

    /// Collect floats that participate in one outer multicol formatting
    /// context.
    ///
    /// Consecutive left/right floats share a block-axis band while their
    /// margin boxes fit in the available inline size; only a float that cannot
    /// fit beside the current band advances to the next shelf. Descendant
    /// traversal stops at independent formatting contexts because their float
    /// state is locally scoped.
    /// <https://www.w3.org/TR/CSS22/visuren.html#float-position>
    /// <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
    fn collect_estimated_multicol_floats(
        &mut self,
        boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        normal_flow_offset: &mut f32,
        floats: &mut Vec<EstimatedMulticolFloat>,
    ) {
        for box_ in boxes {
            match box_ {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    self.collect_estimated_multicol_floats(
                        &box_.children,
                        stylesheets,
                        available_width,
                        normal_flow_offset,
                        floats,
                    );
                }
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                    self.collect_estimated_multicol_floats(
                        &box_.core.children,
                        stylesheets,
                        available_width,
                        normal_flow_offset,
                        floats,
                    );
                }
                _ => {
                    let Some((element, _, style, children)) = box_.element_parts() else {
                        continue;
                    };
                    if style.float != Float::None {
                        let metrics = used_box_metrics(
                            style,
                            PercentageBasis::definite(layout_pt(available_width)),
                        );
                        let content_width = self
                            .used_intrinsic_or_shrink_to_fit_width(
                                element,
                                style,
                                stylesheets,
                                layout_pt(available_width),
                                metrics.horizontal_non_content_length(),
                                Some(children),
                                None,
                            )
                            .points();
                        floats.push(EstimatedMulticolFloat {
                            block_offset: layout_pt(*normal_flow_offset),
                            outer_width: layout_pt(
                                content_width
                                    + metrics.horizontal_non_content_length().points()
                                    + metrics.margin.left.points()
                                    + metrics.margin.right.points(),
                            ),
                            outer_height: layout_pt(
                                self.estimate_element_height(
                                    element,
                                    style,
                                    stylesheets,
                                    available_width,
                                    Some(children),
                                )
                                .unwrap_or(style.line_height)
                                .max(0.0),
                            ),
                        });
                        continue;
                    }
                    let establishes_independent_formatting_context =
                        style.display.establishes_block_formatting_context()
                            || used_property_containment(element, style)
                                .establishes_independent_formatting_context()
                            || style_clips_overflow(style)
                            || style_establishes_multicol_formatting_context(style)
                            || block_align_content_establishes_independent_formatting_context(
                                style.align_content,
                            );
                    if !style_is_in_normal_flow(style) {
                        continue;
                    }
                    let start_offset = *normal_flow_offset;
                    let estimated_height = self
                        .estimate_element_height(
                            element,
                            style,
                            stylesheets,
                            available_width,
                            Some(children),
                        )
                        .unwrap_or(style.line_height)
                        .max(0.0);
                    if establishes_independent_formatting_context {
                        *normal_flow_offset += estimated_height;
                    } else {
                        self.collect_estimated_multicol_floats(
                            children,
                            stylesheets,
                            available_width,
                            normal_flow_offset,
                            floats,
                        );
                        *normal_flow_offset = start_offset + estimated_height;
                    }
                }
            }
        }
    }

    fn estimated_multicol_flow_units(
        &mut self,
        boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        descendant_percentage_height_basis: MulticolDescendantPercentageBasis,
        ancestor_avoid_inside: bool,
    ) -> Vec<EstimatedMulticolFlowUnit> {
        let mut units = Vec::new();
        for box_ in boxes {
            match box_ {
                box_tree::FormattingBox::AnonymousBlock(anonymous) => {
                    let mut nested = self.estimated_multicol_flow_units(
                        &anonymous.children,
                        stylesheets,
                        available_width,
                        descendant_percentage_height_basis,
                        ancestor_avoid_inside
                            || FragmentainerKind::Column.avoids_break_inside(&anonymous.style),
                    );
                    if nested.is_empty() && formatting_box_has_inline_content(&anonymous.children) {
                        nested.push(EstimatedMulticolFlowUnit {
                            block_size: layout_pt(anonymous.style.line_height),
                            avoid_before: anonymous.style.break_before.avoids_column(),
                            avoid_after: anonymous.style.break_after.avoids_column(),
                            forced_before: FragmentainerKind::Column
                                .is_forced_break(anonymous.style.break_before),
                            forced_after: FragmentainerKind::Column
                                .is_forced_break(anonymous.style.break_after),
                            avoid_inside_boundary_before: ancestor_avoid_inside,
                        });
                    }
                    units.extend(nested);
                }
                box_tree::FormattingBox::InlineSplitBlockContext(context) => {
                    units.extend(self.estimated_multicol_flow_units(
                        &context.core.children,
                        stylesheets,
                        available_width,
                        descendant_percentage_height_basis,
                        ancestor_avoid_inside,
                    ));
                }
                _ => {
                    let Some((element, _, style, children)) = box_.element_parts() else {
                        continue;
                    };
                    if !style_is_in_normal_flow(style) || style.float != Float::None {
                        continue;
                    }
                    let metrics = used_box_metrics(
                        style,
                        PercentageBasis::definite(layout_pt(available_width)),
                    );
                    let definite_height = descendant_percentage_height_basis.and_then(|basis| {
                        used_content_box_height_or_auto_with_basis(
                            style,
                            PercentageBasis::definite(content_box_pt(basis)),
                            metrics.vertical_non_content_length(),
                        )
                        .map(|height| {
                            constrain_content_height(
                                style,
                                height,
                                PercentageBasis::definite(layout_pt(available_width)),
                            )
                            .points()
                                + metrics.vertical_non_content_length().points()
                                + metrics.margin.top.points()
                                + metrics.margin.bottom.points()
                        })
                    });
                    let estimated = if is_self_collapsing_block_box(
                        element,
                        style,
                        children,
                        self.document_canvas_overflow,
                    ) {
                        0.0
                    } else {
                        definite_height
                            .or_else(|| {
                                self.estimate_element_height(
                                    element,
                                    style,
                                    stylesheets,
                                    available_width,
                                    Some(children),
                                )
                            })
                            .unwrap_or(style.line_height)
                            .max(0.0)
                    };
                    let can_expose_descendant_breaks = !children.is_empty()
                        && matches!(
                            element_layout_kind(element, style),
                            ElementLayoutKind::BlockFlow | ElementLayoutKind::InlineFlow
                        );
                    if can_expose_descendant_breaks {
                        let inside_avoid = ancestor_avoid_inside
                            || FragmentainerKind::Column.avoids_break_inside(style);
                        let mut nested = self.estimated_multicol_flow_units(
                            children,
                            stylesheets,
                            available_width,
                            descendant_percentage_height_basis,
                            inside_avoid,
                        );
                        if !nested.is_empty() {
                            nested[0].avoid_before |= style.break_before.avoids_column();
                            nested[0].forced_before |=
                                FragmentainerKind::Column.is_forced_break(style.break_before);
                            nested[0].avoid_inside_boundary_before |= ancestor_avoid_inside;
                            if let Some(last) = nested.last_mut() {
                                last.avoid_after |= style.break_after.avoids_column();
                                last.forced_after |=
                                    FragmentainerKind::Column.is_forced_break(style.break_after);
                            }
                            let nested_height = nested
                                .iter()
                                .map(|unit| unit.block_size.points())
                                .sum::<f32>();
                            if estimated > nested_height + 0.01
                                && let Some(last) = nested.last_mut()
                            {
                                last.block_size += layout_pt(estimated - nested_height);
                            }
                            units.extend(nested);
                            continue;
                        }
                    }
                    units.push(EstimatedMulticolFlowUnit {
                        block_size: layout_pt(estimated),
                        avoid_before: style.break_before.avoids_column(),
                        avoid_after: style.break_after.avoids_column(),
                        forced_before: FragmentainerKind::Column
                            .is_forced_break(style.break_before),
                        forced_after: FragmentainerKind::Column.is_forced_break(style.break_after),
                        avoid_inside_boundary_before: ancestor_avoid_inside,
                    });
                }
            }
        }
        units
    }

    /// Estimate the minimum column block-size imposed by monolithic content.
    ///
    /// Size-contained and break-avoiding boxes affect column balancing even
    /// when their internal contents expose no legal break. A negative adjoining
    /// margin before a monolithic float can place part of its margin box above
    /// the fragmentainer start, so the lower bound is measured at that
    /// collapsed sibling offset rather than from zero.
    /// <https://www.w3.org/TR/css-multicol-1/#filling-columns> and
    /// <https://www.w3.org/TR/css-contain-1/#containment-size>
    fn estimated_multicol_monolithic_block_size(
        &mut self,
        boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
    ) -> f32 {
        let mut largest = 0.0f32;
        let mut adjoining_margin = 0.0f32;
        self.accumulate_estimated_multicol_monolithic_block_size(
            boxes,
            stylesheets,
            available_width,
            &mut largest,
            &mut adjoining_margin,
        );
        largest
    }

    fn accumulate_estimated_multicol_monolithic_block_size(
        &mut self,
        boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        largest: &mut f32,
        adjoining_margin: &mut f32,
    ) {
        for box_ in boxes {
            match box_ {
                box_tree::FormattingBox::AnonymousBlock(box_) => {
                    self.accumulate_estimated_multicol_monolithic_block_size(
                        &box_.children,
                        stylesheets,
                        available_width,
                        largest,
                        adjoining_margin,
                    );
                }
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                    self.accumulate_estimated_multicol_monolithic_block_size(
                        &box_.core.children,
                        stylesheets,
                        available_width,
                        largest,
                        adjoining_margin,
                    );
                }
                _ => {
                    let Some((element, _, style, children)) = box_.element_parts() else {
                        continue;
                    };
                    // A table owns row-level fragmentation and can therefore
                    // expose compatible breaks to an orthogonal multicol
                    // parent. Other vertical flow roots remain monolithic
                    // until they provide the same fragmentainer contract.
                    // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
                    // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
                    let owns_compatible_fragmentation =
                        box_.supports_fragmentainer_fragmentation(FragmentainerKind::Column);
                    // An orthogonal flow root is atomic in the containing
                    // horizontal column's block axis unless its own nested
                    // fragmentation context supplies a compatible break. Its
                    // physical block contribution therefore bounds the outer
                    // balance search just like an explicitly monolithic box.
                    // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
                    // <https://www.w3.org/TR/css-break-3/#monolithic>
                    let own_is_monolithic = used_property_containment(element, style).size
                        || style.display.is_atomic_inline()
                        || (style.writing_mode != WritingMode::HorizontalTb
                            && !owns_compatible_fragmentation)
                        || FragmentainerKind::Column.avoids_break_inside(style);
                    let own = own_is_monolithic
                        .then(|| {
                            let estimated = self.estimate_element_height(
                                element,
                                style,
                                stylesheets,
                                available_width,
                                Some(children),
                            );
                            if style.writing_mode == WritingMode::HorizontalTb {
                                return estimated;
                            }
                            let metrics = used_box_metrics(
                                style,
                                PercentageBasis::definite(layout_pt(available_width)),
                            );
                            let text = inline_text_for_style(element, style);
                            let orthogonal_physical_height =
                                self.estimate_text_physical_height(
                                    &text,
                                    style,
                                    available_width,
                                    (metrics.padding.left + metrics.border.left).points(),
                                    (metrics.padding.right + metrics.border.right).points(),
                                ) + metrics.vertical_non_content_length().points()
                                    + metrics.margin.top.points()
                                    + metrics.margin.bottom.points();
                            Some(estimated.unwrap_or(0.0).max(orthogonal_physical_height))
                        })
                        .flatten()
                        .unwrap_or(0.0);
                    let positioned_own = if style.float != Float::None {
                        (own + adjoining_margin.min(0.0)).max(0.0)
                    } else {
                        own
                    };
                    *largest = (*largest).max(positioned_own).max(
                        self.estimated_multicol_monolithic_block_size(
                            children,
                            stylesheets,
                            available_width,
                        ),
                    );

                    if style_is_in_normal_flow(style) && style.float == Float::None {
                        *adjoining_margin = if is_self_collapsing_block_box(
                            element,
                            style,
                            children,
                            self.document_canvas_overflow,
                        ) {
                            collapse_margins(
                                collapse_margins(
                                    layout_pt(*adjoining_margin),
                                    layout_pt(style.margin.top),
                                ),
                                layout_pt(style.margin.bottom),
                            )
                            .points()
                        } else {
                            0.0
                        };
                    }
                }
            }
        }
    }

    pub(in crate::layout) fn layout_definition_list_columns(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> bool {
        let multicol_style = self.multicol_used_style(style);
        let style = &multicol_style;
        if !is_definition_list_element(element) {
            return false;
        }

        let groups = child_boxes
            .map(definition_list_column_groups_from_boxes)
            .unwrap_or_else(|| {
                definition_list_column_groups_with_font_metrics(
                    element,
                    multicol_style.source(),
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                )
            });
        if groups.is_empty() {
            return false;
        }

        let available_width = (self.content_right - self.content_left).max(1.0);
        let gap = used_multicol_column_gap(
            style.column_gap.clone(),
            PercentageBasis::definite(content_box_pt(available_width)),
            style.font_size,
        )
        .points();
        let Some(column_count) =
            used_multicol_column_count(style, available_width, gap).filter(|count| *count > 1)
        else {
            return false;
        };
        let total_gap = gap * column_count.saturating_sub(1) as f32;
        let column_width = ((available_width - total_gap) / column_count as f32).max(1.0);
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let rule_paint_point = self
            .current_page
            .paint_band_insertion_point(PaintBand::InFlowBlock);
        let mut column_cursors = vec![previous_cursor_y; column_count];
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        let text_box_trim_targets =
            definition_list_column_text_box_trim_targets(&groups, column_count, text_box_line_trim);

        for (group_index, group) in groups.iter().enumerate() {
            let column_index = group_index % column_count;
            self.content_left = previous_left + (column_width + gap) * column_index as f32;
            self.content_right = self.content_left + column_width;
            self.cursor_y = column_cursors[column_index];

            for (item_index, item) in group.iter().enumerate() {
                let child_text_box_line_trim = text_box_trim_targets.trim_for(
                    column_index,
                    group_index,
                    item_index,
                    text_box_line_trim,
                );
                self.push_ancestor_signature(item.signature.clone());
                self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                    layout.layout_element_with_child_boxes(
                        item.element,
                        &item.style,
                        stylesheets,
                        item.children,
                    );
                });
                self.ancestors.pop();
            }

            column_cursors[column_index] = self.cursor_y;
        }

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = column_cursors
            .into_iter()
            .fold(previous_cursor_y, |bottom, cursor| bottom.min(cursor));
        let rule_primitives = multicol_gap_decoration_primitives(
            style,
            previous_left,
            previous_cursor_y,
            self.cursor_y,
            column_width,
            gap,
            multicol_decorated_column_count(style, column_count, column_count),
        );
        self.current_page
            .insert_primitives_at_paint_band_point(rule_paint_point, rule_primitives);
        true
    }

    pub(in crate::layout) fn layout_ordered_mixed_flow_children(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        margin_collapse: BlockFlowMarginCollapseContext,
        traversal_state: &mut BlockFlowChildTraversalState,
    ) -> Option<BlockEndMarginCollapse> {
        let BlockFlowMarginCollapseContext {
            can_collapse_start_margin,
            can_collapse_end_margin,
            applied_start_margin,
            starts_at_page_top,
        } = margin_collapse;
        let sibling_tags = element_sibling_signature_list(element);
        let text_box_line_trim = self.effective_text_box_line_trim_for_style(style);
        let text_box_trim_targets = self.ordered_mixed_text_box_trim_targets(
            element,
            style,
            stylesheets,
            &sibling_tags,
            text_box_line_trim,
        );
        let mut element_index = 0usize;
        let mut inline_run_index = 0usize;
        // Preserve each node's source sibling index. Inline runs are laid out
        // through an isolated formatting context, but selectors such as
        // `:nth-of-type()` still resolve against the original parent.
        let mut inline_nodes = Vec::new();
        let mut previous_flow_bottom_margin = None;
        let mut seen_flow_child = false;
        let mut pending_end_margin_collapse = None;
        let mut float_run = self.float_run_state();
        let mut first_formatted_line = FirstFormattedLineState::for_style(style);
        let mut previous_child_page_end: Option<Option<String>> = None;

        for (child_node_index, child) in element.children.iter().enumerate() {
            let NodeKind::Element(child_element) = &child.kind else {
                if !traversal_state.is_exhausted() {
                    inline_nodes.push((child_node_index, child.clone()));
                }
                continue;
            };

            let child_signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let mut child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature.clone(),
                stylesheets,
                Some(style),
            );
            if traversal_state.is_exhausted()
                && (style_is_in_normal_flow(&child_style) || child_style.float != Float::None)
            {
                // Floats after the clamp boundary are part of discarded
                // source; positioned descendants remain eligible for their
                // independent containing-block layout.
                // <https://drafts.csswg.org/css-overflow-4/#continue>
                continue;
            }
            if child_style.float != Float::None {
                let has_later_inline_or_block_source = element.children[child_node_index + 1..]
                    .iter()
                    .any(|node| matches!(&node.kind, NodeKind::Text(text) if !text.trim().is_empty()))
                    || has_later_normal_block_flow_child_with_font_metrics(
                        element,
                        element_index,
                        &sibling_tags,
                        style,
                        stylesheets,
                        &self.ancestors,
                        &mut self.font_system,
                    );
                // A float is taken out of normal flow at its source position,
                // but the preceding inline run still forms around that
                // exclusion. Flushing the run first commits a full-width line
                // and wrongly pushes a following right float to the next
                // line. Place the float, then select the pending line against
                // its available band.
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                let can_share_pending_inline_line = !inline_nodes.is_empty()
                    && !inline_nodes.iter().any(|(_, node)| {
                        matches!(&node.kind, NodeKind::Element(element) if is_line_break_element(element))
                    });
                // A line-clamped run can end immediately before this float.
                // Select that terminal source range before committing the
                // float: CSS Overflow's discarded continuation owns neither
                // the float's paint nor its exclusion.  When the run does
                // not exhaust the shared budget, restore the speculative
                // layout and use the normal float-first selection below so
                // the preceding line still sees the float's exclusion.
                // <https://drafts.csswg.org/css-overflow-4/#continue>
                if can_share_pending_inline_line
                    && let Some(remaining_slots) = traversal_state.remaining_line_slots()
                {
                    let snapshot = self.snapshot();
                    let saved_inline_run_index = inline_run_index;
                    let saved_previous_child_page_end = previous_child_page_end.clone();
                    let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                        element,
                        &inline_nodes,
                        traversal_state
                            .style_with_remaining_and_continuation(
                                style,
                                BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                                    has_later_inline_or_block_source,
                                ),
                            )
                            .as_ref()
                            .unwrap_or(style),
                        stylesheets,
                        &mut inline_run_index,
                        &text_box_trim_targets,
                        text_box_line_trim,
                        first_formatted_line.applies_to_next_inline_run(),
                        &mut previous_child_page_end,
                    );
                    if inline_outcome.clamp_line_slots >= remaining_slots {
                        if inline_outcome.has_flow_effects {
                            first_formatted_line.consume_next_formatted_line();
                            self.flush_float_run(&mut float_run);
                        }
                        traversal_state.debit(inline_outcome.clamp_line_slots);
                        inline_nodes.clear();
                        seen_flow_child = true;
                        previous_flow_bottom_margin = None;
                        continue;
                    }
                    self.restore(snapshot);
                    inline_run_index = saved_inline_run_index;
                    previous_child_page_end = saved_previous_child_page_end;
                }
                if can_share_pending_inline_line
                    && self.layout_floating_child(
                        child_element,
                        child_signature.clone(),
                        &child_style,
                        None,
                        None,
                        stylesheets,
                        &mut float_run,
                    )
                {
                    let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                        element,
                        &inline_nodes,
                        traversal_state
                            .style_with_remaining_and_continuation(
                                style,
                                BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                                    has_later_inline_or_block_source,
                                ),
                            )
                            .as_ref()
                            .unwrap_or(style),
                        stylesheets,
                        &mut inline_run_index,
                        &text_box_trim_targets,
                        text_box_line_trim,
                        first_formatted_line.applies_to_next_inline_run(),
                        &mut previous_child_page_end,
                    );
                    if inline_outcome.has_flow_effects {
                        first_formatted_line.consume_next_formatted_line();
                        self.flush_float_run(&mut float_run);
                    }
                    traversal_state.debit(inline_outcome.clamp_line_slots);
                    inline_nodes.clear();
                    seen_flow_child = true;
                    previous_flow_bottom_margin = None;
                    continue;
                }
                let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                    element,
                    &inline_nodes,
                    traversal_state
                        .style_with_remaining_and_continuation(
                            style,
                            BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                                has_later_inline_or_block_source,
                            ),
                        )
                        .as_ref()
                        .unwrap_or(style),
                    stylesheets,
                    &mut inline_run_index,
                    &text_box_trim_targets,
                    text_box_line_trim,
                    first_formatted_line.applies_to_next_inline_run(),
                    &mut previous_child_page_end,
                );
                if inline_outcome.has_flow_effects {
                    first_formatted_line.consume_next_formatted_line();
                    seen_flow_child = true;
                    previous_flow_bottom_margin = None;
                    self.flush_float_run(&mut float_run);
                }
                traversal_state.debit(inline_outcome.clamp_line_slots);
                inline_nodes.clear();
                if traversal_state.is_exhausted() {
                    // The pending inline run may have spent the final slot
                    // only when it was flushed at this block boundary. A
                    // float following that source belongs to the discarded
                    // continuation and therefore has no placement.
                    // <https://drafts.csswg.org/css-overflow-4/#continue>
                    continue;
                }
                // No shareable inline line preceded this float (for example,
                // a `<br>` established a forced break), so place it only
                // after flushing that run.  Once placed it is out of normal
                // flow and must not be collected again as inline content.
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                if self.layout_floating_child(
                    child_element,
                    child_signature.clone(),
                    &child_style,
                    None,
                    None,
                    stylesheets,
                    &mut float_run,
                ) {
                    continue;
                }
            }
            // The document canvas is treated as a flow owner for its
            // block-level children, but an HTML `<br>` remains inline content
            // even directly under `body`. Keeping it in the pending inline
            // run preserves its forced-break and `clear` semantics.
            // <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
            if is_line_break_element(child_element) {
                inline_nodes.push((child_node_index, child.clone()));
                continue;
            }
            if matches!(child_style.position, Position::Absolute | Position::Fixed) {
                // Positioned descendants are out of flow, but their static
                // position is selected at this source-order boundary. Flush
                // the preceding inline run before dispatching the box rather
                // than treating the descendant as inline content (where a
                // blockified source can be dropped by the anonymous-inline
                // collector).
                // <https://www.w3.org/TR/css-position-3/#static-position>
                let has_later_inline_or_block_source = element.children[child_node_index + 1..]
                    .iter()
                    .any(|node| matches!(&node.kind, NodeKind::Text(text) if !text.trim().is_empty()))
                    || has_later_normal_block_flow_child_with_font_metrics(
                        element,
                        element_index,
                        &sibling_tags,
                        style,
                        stylesheets,
                        &self.ancestors,
                        &mut self.font_system,
                    );
                let cursor_before_inline = self.cursor_y;
                let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                    element,
                    &inline_nodes,
                    traversal_state
                        .style_with_remaining_and_continuation(
                            style,
                            BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                                has_later_inline_or_block_source,
                            ),
                        )
                        .as_ref()
                        .unwrap_or(style),
                    stylesheets,
                    &mut inline_run_index,
                    &text_box_trim_targets,
                    text_box_line_trim,
                    first_formatted_line.applies_to_next_inline_run(),
                    &mut previous_child_page_end,
                );
                if inline_outcome.has_flow_effects {
                    first_formatted_line.consume_next_formatted_line();
                    seen_flow_child = true;
                    previous_flow_bottom_margin = None;
                    self.flush_float_run(&mut float_run);
                }
                traversal_state.debit(inline_outcome.clamp_line_slots);
                inline_nodes.clear();
                // A block-level abspos at this source boundary gets the
                // static rectangle of its hypothetical in-flow block. The
                // preceding inline run has already consumed its real line
                // boxes; preserve that same measured advance as one
                // non-painting block placeholder instead of anchoring at the
                // end of the preceding line.
                // <https://www.w3.org/TR/css-position-3/#static-position>
                let block_static_y_offset = (child_style.display.is_block_level()
                    && inline_outcome.has_flow_effects)
                    .then(|| (cursor_before_inline - self.cursor_y).max(0.0));
                self.push_ancestor_signature(child_signature);
                self.with_text_box_line_trim_scope(TextBoxLineTrim::default(), |layout| {
                    let previous_static_y_offset = layout.block_static_position_y_offset;
                    layout.block_static_position_y_offset = block_static_y_offset;
                    layout.layout_element(child_element, &child_style, stylesheets);
                    layout.block_static_position_y_offset = previous_static_y_offset;
                });
                self.ancestors.pop();
                continue;
            }
            // Ordered DOM traversal may be selected for HTML table
            // structure, but the computed outer display alone decides whether
            // this child leaves the pending inline run. In particular,
            // `inline-table` is an atomic inline-level box, not block flow.
            // <https://drafts.csswg.org/css-display-3/#valdef-display-inline-table>
            let is_flow_child = is_normal_block_flow_child(child_element, &child_style);

            if !is_flow_child {
                inline_nodes.push((child_node_index, child.clone()));
                continue;
            }

            let block_end_margin_trim = BlockEndMarginTrim::for_child(
                style,
                true,
                has_later_normal_block_flow_child_with_font_metrics(
                    element,
                    element_index,
                    &sibling_tags,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                ),
            );
            block_end_margin_trim.apply_to_child(&mut child_style);

            let inherited_page_name = self.active_page_value_scope(style);
            let child_page_values = self.dom_page_boundary_values(
                child_element,
                &child_style,
                stylesheets,
                inherited_page_name.as_deref(),
            );

            let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
                element,
                &inline_nodes,
                traversal_state
                    .style_with_remaining_and_continuation(
                        style,
                        css::ClampContinuation::LaterInFlowContent,
                    )
                    .as_ref()
                    .unwrap_or(style),
                stylesheets,
                &mut inline_run_index,
                &text_box_trim_targets,
                text_box_line_trim,
                first_formatted_line.applies_to_next_inline_run(),
                &mut previous_child_page_end,
            );
            if inline_outcome.has_flow_effects {
                first_formatted_line.consume_next_formatted_line();
                seen_flow_child = true;
                previous_flow_bottom_margin = None;
                self.flush_float_run(&mut float_run);
            }
            traversal_state.debit(inline_outcome.clamp_line_slots);
            inline_nodes.clear();
            if traversal_state.is_exhausted() {
                // This child was classified before the preceding inline run
                // was laid out. Once that run exhausts the shared clamp
                // budget, the child and every descendant it would own are
                // outside the continued fragment, including any positioned
                // descendants whose containing block is wholly after the
                // clamp point.
                // <https://drafts.csswg.org/css-overflow-4/#continue>
                continue;
            }
            if previous_child_page_end
                .as_ref()
                .is_none_or(|previous| previous != &child_page_values.start)
                && (!self.current_page_has_content() || previous_child_page_end.is_some())
            {
                self.switch_page_name_at_class_a_boundary(child_page_values.start.as_deref());
            }
            let child_shares_clamp_context =
                self.child_shares_line_clamp_formatting_context(child_element, &child_style);
            if child_shares_clamp_context {
                let has_later_in_flow_child = has_later_normal_block_flow_child_with_font_metrics(
                    element,
                    element_index,
                    &sibling_tags,
                    style,
                    stylesheets,
                    &self.ancestors,
                    &mut self.font_system,
                );
                traversal_state.apply_to_with_continuation(
                    &mut child_style,
                    BlockFlowChildTraversalState::continuation_for_later_in_flow_source(
                        has_later_in_flow_child,
                    ),
                );
            }

            let collapsible_block_child = is_collapsible_block_child(child_element, &child_style);
            let mut collapses_with_parent_end = false;
            if collapsible_block_child {
                if !seen_flow_child && can_collapse_start_margin {
                    child_style.margin.top = collapsed_start_margin_delta(
                        applied_start_margin,
                        layout_pt(child_style.margin.top),
                        starts_at_page_top,
                    )
                    .points();
                } else if let Some(previous_margin) = previous_flow_bottom_margin {
                    child_style.margin.top = collapsed_margin_delta(
                        layout_pt(previous_margin),
                        layout_pt(child_style.margin.top),
                    )
                    .points();
                }

                collapses_with_parent_end = can_collapse_end_margin
                    && !has_later_normal_block_flow_child_with_font_metrics(
                        element,
                        element_index,
                        &sibling_tags,
                        style,
                        stylesheets,
                        &self.ancestors,
                        &mut self.font_system,
                    );
            }

            // The margin-collapse pass above changes used values. Block layout
            // resolves the box values again, so retain those values before
            // delegating to it.
            // <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
            preserve_adjusted_block_margins(&mut child_style);

            seen_flow_child = true;
            first_formatted_line.consume_next_formatted_line();

            self.flush_float_run(&mut float_run);
            self.push_ancestor_signature(child_signature);
            let child_uses_block_layout = matches!(
                element_layout_kind(child_element, &child_style),
                ElementLayoutKind::BlockFlow
            );
            self.last_block_layout_outcome = BlockLayoutOutcome::default();
            let child_text_box_line_trim = text_box_trim_targets.trim_for(
                OrderedMixedTextBoxTrimTarget::FlowElement(child_node_index),
                text_box_line_trim,
            );
            self.with_text_box_line_trim_scope(child_text_box_line_trim, |layout| {
                layout.layout_element(child_element, &child_style, stylesheets);
            });
            self.ancestors.pop();
            if child_uses_block_layout && child_shares_clamp_context {
                traversal_state.record_descendant_clamp_line_slots(
                    self.last_block_layout_outcome.clamp_line_slots,
                );
                traversal_state.debit(self.last_block_layout_outcome.clamp_line_slots);
            }
            let child_consumed_bottom_margin = if child_uses_block_layout {
                self.last_block_layout_outcome
                    .consumed_bottom_margin
                    .points()
            } else {
                child_style.margin.bottom
            };
            if collapses_with_parent_end {
                pending_end_margin_collapse = Some(BlockEndMarginCollapse {
                    child_consumed_margin: layout_pt(child_consumed_bottom_margin),
                    collapsed_margin: collapse_margins(
                        layout_pt(child_consumed_bottom_margin),
                        layout_pt(style.margin.bottom),
                    ),
                });
            }
            previous_flow_bottom_margin =
                collapsible_block_child.then_some(child_consumed_bottom_margin);
            previous_child_page_end = Some(child_page_values.end);
        }

        let inline_outcome = self.layout_ordered_mixed_inline_fragment_block(
            element,
            &inline_nodes,
            traversal_state
                .style_with_remaining(style)
                .as_ref()
                .unwrap_or(style),
            stylesheets,
            &mut inline_run_index,
            &text_box_trim_targets,
            text_box_line_trim,
            first_formatted_line.applies_to_next_inline_run(),
            &mut previous_child_page_end,
        );
        if inline_outcome.has_flow_effects {
            first_formatted_line.consume_next_formatted_line();
            previous_flow_bottom_margin = None;
            self.flush_float_run(&mut float_run);
        }
        traversal_state.debit(inline_outcome.clamp_line_slots);
        self.flush_float_run(&mut float_run);

        let _ = previous_flow_bottom_margin;
        pending_end_margin_collapse
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_ordered_mixed_inline_fragment_block(
        &mut self,
        parent: &Element,
        inline_nodes: &[(usize, Node)],
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        inline_run_index: &mut usize,
        text_box_trim_targets: &OrderedMixedTextBoxTrimTargets,
        text_box_line_trim: TextBoxLineTrim,
        allow_typographic_first_line: bool,
        previous_child_page_end: &mut Option<Option<String>>,
    ) -> InlineLayoutOutcome {
        // Normal white-space-only DOM runs do not create a line box. In
        // particular, an indentation text node before a block child must not
        // consume a fragmentainer row before that child is placed.
        // <https://www.w3.org/TR/css-text-3/#white-space-phase-1>
        if inline_nodes.iter().all(|(_, node)| {
            matches!(&node.kind, NodeKind::Text(text) if normalize_inline_text(text).is_empty())
        }) {
            return InlineLayoutOutcome::default();
        }
        let inline_page_value = self.active_page_value_scope(style);
        if previous_child_page_end
            .as_ref()
            .is_some_and(|previous| previous != &inline_page_value)
        {
            self.switch_page_name_at_class_a_boundary(inline_page_value.as_deref());
        }
        let is_text_box_trim_candidate = ordered_mixed_inline_nodes_accept_text_box_trim(
            &inline_nodes
                .iter()
                .map(|(_, node)| node.clone())
                .collect::<Vec<_>>(),
            style,
        );
        let run_text_box_line_trim = if is_text_box_trim_candidate {
            text_box_trim_targets.trim_for(
                OrderedMixedTextBoxTrimTarget::InlineRun(*inline_run_index),
                text_box_line_trim,
            )
        } else {
            TextBoxLineTrim::default()
        };
        let laid_out = self.with_text_box_line_trim_scope(run_text_box_line_trim, |layout| {
            layout.begin_clamp_line_slot_capture();
            layout.layout_inline_fragment_block_with_first_line_policy(
                inline_nodes,
                parent,
                style,
                stylesheets,
                allow_typographic_first_line,
            );
            layout.finish_clamp_line_slot_capture()
        });
        if is_text_box_trim_candidate {
            *inline_run_index += 1;
        }
        self.record_clamp_line_slots(laid_out);
        let outcome = InlineLayoutOutcome {
            next_line_index: laid_out,
            clamp_line_slots: laid_out,
            has_non_phantom_line: laid_out > 0,
            has_flow_effects: laid_out > 0,
        };
        if outcome.has_flow_effects {
            *previous_child_page_end = Some(inline_page_value);
        }
        outcome
    }

    fn ordered_mixed_text_box_trim_targets(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        sibling_tags: &ElementSiblingSignatureList,
        trim: TextBoxLineTrim,
    ) -> OrderedMixedTextBoxTrimTargets {
        let mut targets = OrderedMixedTextBoxTrimTargets::default();
        if trim.is_empty() {
            return targets;
        }

        let mut element_index = 0usize;
        let mut inline_run_index = 0usize;
        let mut inline_nodes = Vec::new();
        let mut candidates = Vec::new();

        for (child_node_index, child) in element.children.iter().enumerate() {
            let NodeKind::Element(child_element) = &child.kind else {
                inline_nodes.push(child.clone());
                continue;
            };

            let child_signature = ElementSignature::with_sibling_list(
                child_element.tag.clone(),
                child_element.attrs.clone(),
                element_index,
                sibling_tags.clone(),
            );
            element_index += 1;
            let child_style = self.style_for_layout_element_with_parent_font_metrics(
                child_element,
                child_signature,
                stylesheets,
                Some(style),
            );
            if child_style.float != Float::None {
                if ordered_mixed_inline_nodes_accept_text_box_trim(&inline_nodes, style) {
                    candidates.push(OrderedMixedTextBoxTrimCandidate {
                        target: OrderedMixedTextBoxTrimTarget::InlineRun(inline_run_index),
                        accepts_block_start: true,
                        accepts_block_end: true,
                    });
                    inline_run_index += 1;
                }
                inline_nodes.clear();
                continue;
            }

            let is_flow_child = is_normal_block_flow_child(child_element, &child_style);
            if !is_flow_child {
                inline_nodes.push(child.clone());
                continue;
            }

            if ordered_mixed_inline_nodes_accept_text_box_trim(&inline_nodes, style) {
                candidates.push(OrderedMixedTextBoxTrimCandidate {
                    target: OrderedMixedTextBoxTrimTarget::InlineRun(inline_run_index),
                    accepts_block_start: true,
                    accepts_block_end: true,
                });
                inline_run_index += 1;
            }
            inline_nodes.clear();

            candidates.push(OrderedMixedTextBoxTrimCandidate {
                target: OrderedMixedTextBoxTrimTarget::FlowElement(child_node_index),
                accepts_block_start: ordered_mixed_element_accepts_text_box_trim(
                    child_element,
                    &child_style,
                    true,
                ),
                accepts_block_end: ordered_mixed_element_accepts_text_box_trim(
                    child_element,
                    &child_style,
                    false,
                ),
            });
        }

        if ordered_mixed_inline_nodes_accept_text_box_trim(&inline_nodes, style) {
            candidates.push(OrderedMixedTextBoxTrimCandidate {
                target: OrderedMixedTextBoxTrimTarget::InlineRun(inline_run_index),
                accepts_block_start: true,
                accepts_block_end: true,
            });
        }

        if trim.trims_block_start {
            targets.block_start = candidates
                .first()
                .and_then(|candidate| candidate.accepts_block_start.then_some(candidate.target));
        }
        if trim.trims_block_end {
            targets.block_end = candidates
                .iter()
                .next_back()
                .and_then(|candidate| candidate.accepts_block_end.then_some(candidate.target));
        }
        targets
    }
}

/// Choose an earlier class A boundary when the latest fitting boundary is
/// avoid-constrained. The committed flow still performs the authoritative
/// layout; this only gives the first anonymous column box the block extent of
/// the best recursively discovered opportunity, allowing a break inside a
/// preceding nested block to be selected after later avoid pressure appears.
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
fn preferred_first_multicol_break(
    units: &[EstimatedMulticolFlowUnit],
    fragmentainer_block_size: f32,
) -> Option<f32> {
    if units.len() < 2 {
        return None;
    }
    let mut offsets = Vec::with_capacity(units.len() + 1);
    offsets.push(0.0);
    for unit in units {
        offsets.push(offsets.last().cloned().unwrap_or(0.0) + unit.block_size.points());
    }
    let overflow_index =
        (1..units.len()).find(|index| offsets[index + 1] > fragmentainer_block_size + 0.01)?;
    let boundary_is_avoided = |index: usize| {
        units[index - 1].avoid_after
            || units[index].avoid_before
            || units[index].avoid_inside_boundary_before
    };
    let boundary_is_forced =
        |index: usize| units[index - 1].forced_after || units[index].forced_before;
    // The committed flow layout will materialize a forced column break. Do
    // not let the speculative avoid-break height adjustment move an earlier
    // boundary ahead of it; forced breaks take precedence over avoid values.
    // <https://www.w3.org/TR/css-break-3/#forced-breaks>
    if (1..=overflow_index).any(boundary_is_forced) {
        return None;
    }
    if !boundary_is_avoided(overflow_index) {
        return None;
    }
    (1..overflow_index)
        .rev()
        .find(|index| !boundary_is_avoided(*index))
        .map(|index| offsets[index])
        .filter(|offset| *offset > 0.01)
}

/// Return the largest avoid-connected run that can fit in one fragmentainer.
///
/// The caller restricts this estimate to direct empty block siblings, where
/// each unit maps one-to-one to a class-A boundary. Nested formatting contexts
/// continue to use speculative layout because their internal opportunities
/// cannot be represented by a flat hard lower bound.
/// <https://www.w3.org/TR/css-break-3/#unforced-breaks>
/// <https://www.w3.org/TR/css-multicol-1/#filling-columns>
fn minimum_honorable_avoid_run_height(
    units: &[EstimatedMulticolFlowUnit],
    available_fragmentainer_height: f32,
) -> f32 {
    let Some(first) = units.first() else {
        return 0.0;
    };
    let mut largest = 0.0f32;
    let mut run_height = first.block_size.points();
    for index in 1..units.len() {
        let previous = units[index - 1];
        let current = units[index];
        let forced = previous.forced_after || current.forced_before;
        let avoided = previous.avoid_after || current.avoid_before;
        if forced || !avoided {
            if run_height <= available_fragmentainer_height + 0.01 {
                largest = largest.max(run_height);
            }
            run_height = current.block_size.points();
        } else {
            run_height += current.block_size.points();
        }
    }
    if run_height <= available_fragmentainer_height + 0.01 {
        largest = largest.max(run_height);
    }
    largest
}

/// Return the tallest source run delimited by forced column breaks.
///
/// An auto-sized multicol has no external block-size constraint. Forced breaks
/// nevertheless partition its source into explicit columns, so its intrinsic
/// column block size is at least the tallest such run for both sequential and
/// balanced filling. This also gives overflow columns beyond the requested
/// count a finite, content-derived height instead of borrowing the remaining
/// page height.
/// <https://www.w3.org/TR/css-multicol-1/#filling-columns>
/// <https://www.w3.org/TR/css-break-3/#forced-breaks>
fn preferred_forced_run_height(units: &[EstimatedMulticolFlowUnit]) -> Option<f32> {
    let first = units.first()?;
    let mut saw_forced_boundary = false;
    let mut tallest = 0.0f32;
    let mut run = first.block_size.points();
    for index in 1..units.len() {
        let previous = units[index - 1];
        let current = units[index];
        if previous.forced_after || current.forced_before {
            saw_forced_boundary = true;
            tallest = tallest.max(run);
            run = current.block_size.points();
        } else {
            run += current.block_size.points();
        }
    }
    tallest = tallest.max(run);
    saw_forced_boundary.then_some(tallest)
}

/// Build the source-ordered multicol flow when a descendant spanner is
/// promoted through ordinary block containers.
///
/// The walk stops at nested multicol containers, independent formatting
/// contexts, overflow roots, floats, and boxes that establish a fixed-position
/// containing block. A spanner below any such boundary belongs to a different
/// formatting context and cannot span this multicol container.
/// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
fn descendant_multicol_flow_segments<'a>(
    boxes: &[box_tree::FormattingBox<'a>],
) -> Option<Vec<MulticolFlowSegment<'a>>> {
    let (segments, found_spanner) = multicol_flow_segments_for_children(boxes);
    found_spanner.then_some(segments)
}

/// Whether this formatting-box subtree contributes a spanner to its nearest
/// multicol formatting context.
///
/// This uses the same promotion walk as committed column layout, so intrinsic
/// sizing does not accidentally treat a `column-span: all` descendant below a
/// flow root, nested multicol, float, or containment boundary as a spanner.
/// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
pub(in crate::layout) fn formatting_boxes_have_eligible_multicol_spanner(
    boxes: &[box_tree::FormattingBox<'_>],
) -> bool {
    descendant_multicol_flow_segments(boxes).is_some()
}

fn multicol_flow_segments_for_children<'a>(
    boxes: &[box_tree::FormattingBox<'a>],
) -> (Vec<MulticolFlowSegment<'a>>, bool) {
    let mut segments = vec![MulticolFlowSegment::ColumnSet(Vec::new())];
    let mut found_spanner = false;
    for box_ in boxes {
        let (parts, part_has_spanner) = multicol_flow_segments_for_box(box_);
        found_spanner |= part_has_spanner;
        for part in parts {
            append_multicol_flow_segment(&mut segments, part);
        }
    }
    (segments, found_spanner)
}

fn multicol_flow_segments_for_box<'a>(
    box_: &box_tree::FormattingBox<'a>,
) -> (Vec<MulticolFlowSegment<'a>>, bool) {
    if formatting_box_is_eligible_multicol_spanner(box_) {
        return (
            vec![MulticolFlowSegment::Spanner(Box::new(box_.clone()))],
            true,
        );
    }
    if !formatting_box_allows_descendant_spanner_promotion(box_) {
        return (
            vec![MulticolFlowSegment::ColumnSet(vec![box_.clone()])],
            false,
        );
    }

    let (child_segments, found_spanner) = multicol_flow_segments_for_children(box_.children());
    if !found_spanner {
        return (
            vec![MulticolFlowSegment::ColumnSet(vec![box_.clone()])],
            false,
        );
    }

    let column_indices = child_segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| match segment {
            MulticolFlowSegment::ColumnSet(_) => Some(index),
            _ => None,
        })
        .collect::<Vec<_>>();
    let first_column_index = column_indices.first().cloned();
    let last_column_index = column_indices.last().cloned();
    let mut segments = Vec::with_capacity(child_segments.len());
    for (index, segment) in child_segments.into_iter().enumerate() {
        match segment {
            MulticolFlowSegment::ColumnSet(children) => {
                if let Some(fragment) = clone_multicol_wrapper_with_children(
                    box_,
                    children,
                    Some(index) == first_column_index,
                    Some(index) == last_column_index,
                ) {
                    segments.push(MulticolFlowSegment::ColumnSet(vec![fragment]));
                }
            }
            MulticolFlowSegment::Spanner(spanner) => {
                segments.push(MulticolFlowSegment::Spanner(spanner));
            }
        }
    }
    (segments, true)
}

fn append_multicol_flow_segment<'a>(
    segments: &mut Vec<MulticolFlowSegment<'a>>,
    segment: MulticolFlowSegment<'a>,
) {
    match segment {
        MulticolFlowSegment::ColumnSet(mut boxes) => {
            if let Some(MulticolFlowSegment::ColumnSet(previous)) = segments.last_mut() {
                previous.append(&mut boxes);
            } else {
                segments.push(MulticolFlowSegment::ColumnSet(boxes));
            }
        }
        MulticolFlowSegment::Spanner(box_) => {
            segments.push(MulticolFlowSegment::Spanner(box_));
        }
    }
}

fn formatting_box_is_eligible_multicol_spanner(box_: &box_tree::FormattingBox<'_>) -> bool {
    box_.element_parts().is_some_and(|(_, _, style, _)| {
        style.column_span == css::ColumnSpan::All
            && style_is_in_normal_flow(style)
            && style.float == Float::None
            && style.display.is_block_level()
    })
}

fn formatting_box_allows_descendant_spanner_promotion(box_: &box_tree::FormattingBox<'_>) -> bool {
    let style = match box_ {
        box_tree::FormattingBox::Block(box_) => box_.core.style.as_ref(),
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => box_.core.style.as_ref(),
        box_tree::FormattingBox::AnonymousBlock(box_) => box_.style.as_ref(),
        _ => return false,
    };
    let establishes_fixed_containing_block = style.has_transform()
        || style.filter != css::FilterValue::None
        || style.contain.layout
        || style.contain.paint
        || style.will_change.transform
        || style.will_change.filter
        || style.will_change.contain;
    let establishes_independent_formatting_context =
        style.display.establishes_block_formatting_context()
            || style.float != Float::None
            || style_clips_overflow(style)
            || block_align_content_establishes_independent_formatting_context(style.align_content);
    let establishes_nested_multicol = matches!(style.column_count, css::ColumnCount::Count(_))
        || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
        || matches!(style.column_height, css::ComputedColumnHeight::Length(_));
    style_is_in_normal_flow(style)
        && !establishes_fixed_containing_block
        && !establishes_independent_formatting_context
        && !establishes_nested_multicol
}

fn clone_multicol_wrapper_with_children<'a>(
    box_: &box_tree::FormattingBox<'a>,
    children: Vec<box_tree::FormattingBox<'a>>,
    owns_block_start: bool,
    owns_block_end: bool,
) -> Option<box_tree::FormattingBox<'a>> {
    if children.is_empty() && empty_multicol_wrapper_fragment_is_zero_sized(box_) {
        return None;
    }
    match box_ {
        box_tree::FormattingBox::Block(box_) => {
            let mut fragment = box_.clone();
            fragment.core.children = children;
            fragment.core.style = multicol_wrapper_fragment_style(
                &fragment.core.style,
                owns_block_start,
                owns_block_end,
            );
            Some(box_tree::FormattingBox::Block(fragment))
        }
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
            let mut fragment = box_.clone();
            fragment.core.children = children;
            fragment.core.style = multicol_wrapper_fragment_style(
                &fragment.core.style,
                owns_block_start,
                owns_block_end,
            );
            Some(box_tree::FormattingBox::InlineSplitBlockContext(fragment))
        }
        box_tree::FormattingBox::AnonymousBlock(box_) => {
            let mut fragment = box_.clone();
            fragment.children = children;
            fragment.style =
                multicol_wrapper_fragment_style(&fragment.style, owns_block_start, owns_block_end);
            Some(box_tree::FormattingBox::AnonymousBlock(fragment))
        }
        _ => None,
    }
}

/// Whether an empty generated wrapper fragment has neither size nor margins.
///
/// Promoting a descendant spanner may leave an empty wrapper on either side of
/// it. CSS block margin collapsing makes a plain empty block self-collapsing;
/// retaining that wrapper as a nominal balanced column would incorrectly add
/// one line-height of space before the spanner. Margin-bearing or decorated
/// wrappers are retained because their fragment edges still participate in
/// layout and painting.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
fn empty_multicol_wrapper_fragment_is_zero_sized(box_: &box_tree::FormattingBox<'_>) -> bool {
    let box_tree::FormattingBox::Block(box_) = box_ else {
        return false;
    };
    let style = box_.core.style.as_ref();
    style.margin == css::Edges::ZERO
        && is_self_collapsing_block_box(
            box_.core.element,
            style,
            &[],
            DocumentCanvasResolution::default(),
        )
}

/// Create the computed style used by one generated wrapper fragment.
///
/// `box-decoration-break:slice` owns the block-start decoration only on the
/// first fragment and the block-end decoration only on the last. A definite
/// block size is likewise retained only on the last fragment, where the
/// distribution pass replaces it with the remaining size.
/// <https://www.w3.org/TR/css-break-3/#break-decoration>
fn multicol_wrapper_fragment_style(
    style: &std::rc::Rc<ComputedStyle>,
    owns_block_start: bool,
    owns_block_end: bool,
) -> std::rc::Rc<ComputedStyle> {
    let mut fragment_style = style.as_ref().clone();
    if !owns_block_end {
        set_style_auto_logical_block_size(&mut fragment_style);
    }
    if fragment_style.box_decoration_break == css::BoxDecorationBreak::Slice {
        suppress_multicol_wrapper_fragment_block_edges(
            &mut fragment_style,
            owns_block_start,
            owns_block_end,
        );
    }
    std::rc::Rc::new(fragment_style)
}

fn set_style_auto_logical_block_size(style: &mut ComputedStyle) {
    if WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes() {
        set_style_auto_width(style);
    } else {
        set_style_auto_height(style);
    }
}

fn set_style_used_logical_block_size(style: &mut ComputedStyle, size: f32) {
    if WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes() {
        set_style_used_width(style, size);
    } else {
        set_style_used_height(style, size);
    }
}

/// Give promoted spanners the multicol container's definite logical inline size.
///
/// A spanning box establishes a containing block whose inline size is the
/// multicol content box's inline size. In a vertical writing mode that inline
/// axis is physical height, so the ordinary horizontal block-width algorithm
/// cannot supply the spanner's auto physical height. Resolve that one used
/// dimension on the temporary formatting-box style before laying the spanner
/// out as a full-width sibling of the adjacent column sets.
/// <https://www.w3.org/TR/css-multicol-1/#spanning-columns>
/// <https://www.w3.org/TR/css-writing-modes-3/#logical-to-physical>
fn multicol_spanner_boxes_with_container_inline_size<'a>(
    boxes: &[box_tree::FormattingBox<'a>],
    multicol_style: &ComputedStyle,
    container_physical_height: Option<PhysicalContentHeight>,
    container_physical_width: PhysicalContentWidth,
) -> Vec<box_tree::FormattingBox<'a>> {
    if !WritingModeAxes::new(multicol_style.writing_mode, multicol_style.direction)
        .swaps_physical_axes()
    {
        return boxes.to_vec();
    }
    let Some(container_inline_size) = container_physical_height else {
        return boxes.to_vec();
    };

    boxes
        .iter()
        .cloned()
        .map(|mut box_| {
            let Some(style_slot) = box_.element_core_mut().map(|core| &mut core.style) else {
                return box_;
            };
            if !matches!(
                *style_slot.box_values.height,
                css::ComputedLengthPercentageOrAuto::Auto
                    | css::ComputedLengthPercentageOrAuto::Stretch
            ) {
                return box_;
            }
            let mut used_style = style_slot.as_ref().clone();
            let metrics = used_box_metrics(
                &used_style,
                PercentageBasis::definite(crate::units::IntoLayoutLength::into_layout_length(
                    container_physical_width.content_box_length(),
                )),
            );
            let non_content = metrics.vertical_non_content_length().points()
                + metrics.margin.top.points()
                + metrics.margin.bottom.points();
            set_style_used_height(
                &mut used_style,
                (container_inline_size.points() - non_content).max(0.0),
            );
            *style_slot = std::rc::Rc::new(used_style);
            box_
        })
        .collect()
}

fn definite_logical_block_size(style: &ComputedStyle, available_width: f32) -> Option<f32> {
    let metrics = used_box_metrics(style, PercentageBasis::definite(layout_pt(available_width)));
    let (value, non_content) =
        if WritingModeAxes::new(style.writing_mode, style.direction).swaps_physical_axes() {
            (
                style.box_values.width.clone(),
                metrics.horizontal_non_content_length().points(),
            )
        } else {
            (
                style.box_values.height.value().clone(),
                metrics.vertical_non_content_length().points(),
            )
        };
    match value {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(value)
            if value.is_definitely_absolute() =>
        {
            let specified = value.length_max_zero().points();
            Some(match style.box_sizing {
                css::BoxSizing::ContentBox => specified,
                css::BoxSizing::BorderBox => (specified - non_content).max(0.0),
            })
        }
        _ => None,
    }
}

fn suppress_multicol_wrapper_fragment_block_edges(
    style: &mut ComputedStyle,
    owns_block_start: bool,
    owns_block_end: bool,
) {
    if !owns_block_start {
        suppress_multicol_wrapper_fragment_physical_edge(
            style,
            block_start_side(style.writing_mode),
        );
    }
    if !owns_block_end {
        suppress_multicol_wrapper_fragment_physical_edge(style, block_end_side(style.writing_mode));
    }
}

fn suppress_multicol_wrapper_fragment_physical_edge(style: &mut ComputedStyle, side: PhysicalSide) {
    let zero_margin = css::ComputedLengthPercentageOrAuto::ZERO;
    let zero_edge = css::ComputedLengthPercentage::ZERO;
    match side {
        PhysicalSide::Top => {
            style.box_values.margin.top = zero_margin;
            style.box_values.padding.top = zero_edge.clone();
            style.margin.top = 0.0;
            style.padding.top = 0.0;
            style.border_width_values.top = zero_edge;
            style.border_widths.top = 0.0;
            style.border_styles.top = css::BorderStyle::None;
        }
        PhysicalSide::Right => {
            style.box_values.margin.right = zero_margin;
            style.box_values.padding.right = zero_edge.clone();
            style.margin.right = 0.0;
            style.padding.right = 0.0;
            style.border_width_values.right = zero_edge;
            style.border_widths.right = 0.0;
            style.border_styles.right = css::BorderStyle::None;
        }
        PhysicalSide::Bottom => {
            style.box_values.margin.bottom = zero_margin;
            style.box_values.padding.bottom = zero_edge.clone();
            style.margin.bottom = 0.0;
            style.padding.bottom = 0.0;
            style.border_width_values.bottom = zero_edge;
            style.border_widths.bottom = 0.0;
            style.border_styles.bottom = css::BorderStyle::None;
        }
        PhysicalSide::Left => {
            style.box_values.margin.left = zero_margin;
            style.box_values.padding.left = zero_edge.clone();
            style.margin.left = 0.0;
            style.padding.left = 0.0;
            style.border_width_values.left = zero_edge;
            style.border_widths.left = 0.0;
            style.border_styles.left = css::BorderStyle::None;
        }
    }
}

fn multicol_wrapper_key(box_: &box_tree::FormattingBox<'_>) -> Option<usize> {
    let signature = match box_ {
        box_tree::FormattingBox::Block(box_) => &box_.core.signature,
        box_tree::FormattingBox::InlineSplitBlockContext(box_) => &box_.core.signature,
        _ => return None,
    };
    Some(std::rc::Rc::as_ptr(&signature.opaque_id) as usize)
}

fn collect_multicol_wrapper_depths(
    boxes: &[box_tree::FormattingBox<'_>],
    depth: usize,
    depths: &mut HashMap<usize, usize>,
) {
    for box_ in boxes {
        if let Some(key) = multicol_wrapper_key(box_) {
            depths
                .entry(key)
                .and_modify(|known| *known = (*known).max(depth))
                .or_insert(depth);
        }
        collect_multicol_wrapper_depths(box_.children(), depth + 1, depths);
    }
}

type MulticolWrapperFragmentRef<'a, 'b> = (
    &'a Element,
    &'b ComputedStyle,
    &'b [box_tree::FormattingBox<'a>],
);

fn multicol_wrapper_fragments<'a, 'b>(
    boxes: &'b [box_tree::FormattingBox<'a>],
    key: usize,
) -> Vec<MulticolWrapperFragmentRef<'a, 'b>> {
    let mut fragments = Vec::new();
    for box_ in boxes {
        match box_ {
            box_tree::FormattingBox::Block(box_)
                if multicol_wrapper_key(&box_tree::FormattingBox::Block(box_.clone()))
                    == Some(key) =>
            {
                fragments.push((
                    box_.core.element,
                    box_.core.style.as_ref(),
                    box_.core.children.as_slice(),
                ));
            }
            box_tree::FormattingBox::InlineSplitBlockContext(box_)
                if multicol_wrapper_key(&box_tree::FormattingBox::InlineSplitBlockContext(
                    box_.clone(),
                )) == Some(key) =>
            {
                fragments.push((
                    box_.core.element,
                    box_.core.style.as_ref(),
                    box_.core.children.as_slice(),
                ));
            }
            _ => {}
        }
        fragments.extend(multicol_wrapper_fragments(box_.children(), key));
    }
    fragments
}

fn set_multicol_wrapper_block_sizes(
    segments: &mut [MulticolFlowSegment<'_>],
    key: usize,
    sizes: &[f32],
) {
    let mut occurrence = 0usize;
    for segment in segments.iter_mut() {
        let MulticolFlowSegment::ColumnSet(boxes) = segment else {
            continue;
        };
        set_multicol_wrapper_block_sizes_in_boxes(boxes, key, sizes, &mut occurrence);
    }
}

fn set_multicol_wrapper_block_sizes_in_boxes(
    boxes: &mut [box_tree::FormattingBox<'_>],
    key: usize,
    sizes: &[f32],
    occurrence: &mut usize,
) {
    for box_ in boxes.iter_mut() {
        let is_target = multicol_wrapper_key(box_) == Some(key);
        if is_target && let Some(size) = sizes.get(*occurrence).cloned() {
            let style = match box_ {
                box_tree::FormattingBox::Block(box_) => Some(&mut box_.core.style),
                box_tree::FormattingBox::InlineSplitBlockContext(box_) => {
                    Some(&mut box_.core.style)
                }
                _ => None,
            };
            if let Some(style) = style {
                let mut used_style = style.as_ref().clone();
                set_style_used_logical_block_size(&mut used_style, size);
                *style = std::rc::Rc::new(used_style);
            }
            *occurrence += 1;
        }
        set_multicol_wrapper_block_sizes_in_boxes(box_.children_mut(), key, sizes, occurrence);
    }
}

fn first_overflow_boundary_is_avoided(
    units: &[EstimatedMulticolFlowUnit],
    fragmentainer_block_size: f32,
) -> bool {
    let total = units
        .iter()
        .map(|unit| unit.block_size.points())
        .sum::<f32>();
    if total > fragmentainer_block_size + 0.01
        && units
            .iter()
            .any(|unit| unit.avoid_before || unit.avoid_after)
    {
        return true;
    }
    let mut offset = 0.0;
    for (index, unit) in units.iter().enumerate() {
        if index > 0 && offset + unit.block_size.points() > fragmentainer_block_size + 0.01 {
            if units[index - 1].forced_after || unit.forced_before {
                return false;
            }
            return units[index - 1].avoid_after
                || unit.avoid_before
                || unit.avoid_inside_boundary_before;
        }
        offset += unit.block_size.points();
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderedMixedTextBoxTrimTarget {
    InlineRun(usize),
    FlowElement(usize),
}

#[derive(Debug, Clone, Copy)]
struct OrderedMixedTextBoxTrimCandidate {
    target: OrderedMixedTextBoxTrimTarget,
    accepts_block_start: bool,
    accepts_block_end: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct OrderedMixedTextBoxTrimTargets {
    block_start: Option<OrderedMixedTextBoxTrimTarget>,
    block_end: Option<OrderedMixedTextBoxTrimTarget>,
}

impl OrderedMixedTextBoxTrimTargets {
    fn trim_for(
        self,
        target: OrderedMixedTextBoxTrimTarget,
        source: TextBoxLineTrim,
    ) -> TextBoxLineTrim {
        let trims_block_start = self.block_start == Some(target);
        let trims_block_end = self.block_end == Some(target);
        TextBoxLineTrim {
            trims_block_start,
            trims_block_end,
            block_start: if trims_block_start {
                source.block_start
            } else {
                0.0
            },
            block_end: if trims_block_end {
                source.block_end
            } else {
                0.0
            },
        }
    }
}

fn ordered_mixed_inline_nodes_accept_text_box_trim(
    nodes: &[Node],
    containing_style: &ComputedStyle,
) -> bool {
    if nodes.is_empty() {
        return false;
    }
    !inline_text_for_style(
        &Element {
            id: crate::dom::ElementId::next(),
            tag: "span".to_string(),
            namespace_url: String::new(),
            document_syntax: dom::DocumentSyntax::Html,
            attrs: HashMap::new(),
            namespace_attrs: Vec::new(),
            children: nodes.to_vec(),
            is_target: false,
            object_rendering: dom::ObjectRendering::Fallback,
        },
        containing_style,
    )
    .is_empty()
}

fn ordered_mixed_element_accepts_text_box_trim(
    element: &Element,
    style: &ComputedStyle,
    block_start: bool,
) -> bool {
    matches!(
        element_layout_kind(element, style),
        ElementLayoutKind::BlockFlow
    ) && definition_list_item_style_allows_text_box_trim(style, block_start)
}

#[derive(Debug, Clone)]
struct DefinitionListColumnTextBoxTrimTargets {
    block_start: Vec<Option<(usize, usize)>>,
    block_end: Vec<Option<(usize, usize)>>,
    block_start_blocked: Vec<bool>,
    block_end_blocked: Vec<bool>,
}

impl DefinitionListColumnTextBoxTrimTargets {
    fn empty(column_count: usize) -> Self {
        Self {
            block_start: vec![None; column_count],
            block_end: vec![None; column_count],
            block_start_blocked: vec![false; column_count],
            block_end_blocked: vec![false; column_count],
        }
    }

    fn trim_for(
        &self,
        column_index: usize,
        group_index: usize,
        item_index: usize,
        source: TextBoxLineTrim,
    ) -> TextBoxLineTrim {
        let trims_block_start = self.block_start.get(column_index).cloned().flatten()
            == Some((group_index, item_index));
        let trims_block_end =
            self.block_end.get(column_index).cloned().flatten() == Some((group_index, item_index));
        TextBoxLineTrim {
            trims_block_start,
            trims_block_end,
            block_start: if trims_block_start {
                source.block_start
            } else {
                0.0
            },
            block_end: if trims_block_end {
                source.block_end
            } else {
                0.0
            },
        }
    }
}

fn definition_list_column_text_box_trim_targets(
    groups: &[Vec<DefinitionListColumnItem<'_>>],
    column_count: usize,
    trim: TextBoxLineTrim,
) -> DefinitionListColumnTextBoxTrimTargets {
    let mut targets = DefinitionListColumnTextBoxTrimTargets::empty(column_count);
    if trim.is_empty() {
        return targets;
    }

    if trim.trims_block_start {
        for (group_index, group) in groups.iter().enumerate() {
            let column_index = group_index % column_count;
            if targets.block_start[column_index].is_some()
                || targets.block_start_blocked[column_index]
            {
                continue;
            }
            match definition_list_group_edge_text_box_trim_target(group, true, false) {
                Some((item_index, true)) => {
                    targets.block_start[column_index] = Some((group_index, item_index));
                }
                Some((_, false)) => targets.block_start_blocked[column_index] = true,
                None => continue,
            }
        }
    }

    if trim.trims_block_end {
        for (group_index, group) in groups.iter().enumerate().rev() {
            let column_index = group_index % column_count;
            if targets.block_end[column_index].is_some() || targets.block_end_blocked[column_index]
            {
                continue;
            }
            match definition_list_group_edge_text_box_trim_target(group, false, true) {
                Some((item_index, true)) => {
                    targets.block_end[column_index] = Some((group_index, item_index));
                }
                Some((_, false)) => targets.block_end_blocked[column_index] = true,
                None => continue,
            }
        }
    }

    targets
}

fn definition_list_group_edge_text_box_trim_target(
    group: &[DefinitionListColumnItem<'_>],
    block_start: bool,
    find_last: bool,
) -> Option<(usize, bool)> {
    if find_last {
        group.iter().enumerate().next_back().map(|(index, item)| {
            (
                index,
                definition_list_item_accepts_text_box_trim(item, block_start),
            )
        })
    } else {
        group.iter().enumerate().next().map(|(index, item)| {
            (
                index,
                definition_list_item_accepts_text_box_trim(item, block_start),
            )
        })
    }
}

fn definition_list_item_accepts_text_box_trim(
    item: &DefinitionListColumnItem<'_>,
    block_start: bool,
) -> bool {
    matches!(
        element_layout_kind(item.element, &item.style),
        ElementLayoutKind::BlockFlow
    ) && definition_list_item_style_allows_text_box_trim(&item.style, block_start)
}

fn definition_list_item_style_allows_text_box_trim(
    style: &ComputedStyle,
    block_start: bool,
) -> bool {
    let side = if block_start {
        block_start_side(style.writing_mode)
    } else {
        block_end_side(style.writing_mode)
    };
    definition_list_item_physical_edge_value(style.padding, side) <= 0.0
        && definition_list_item_physical_edge_value(used_border_widths(style), side) <= 0.0
}

fn definition_list_item_physical_edge_value(edges: Edges, side: PhysicalSide) -> f32 {
    match side {
        PhysicalSide::Top => edges.top,
        PhysicalSide::Right => edges.right,
        PhysicalSide::Bottom => edges.bottom,
        PhysicalSide::Left => edges.left,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_later_orthogonal_column_pages_receive_a_full_destination_clip() {
        assert!(!continuation_column_fragment_requires_full_clip(
            0,
            WritingMode::VerticalLr,
            Direction::Ltr,
        ));
        assert!(!continuation_column_fragment_requires_full_clip(
            1,
            WritingMode::HorizontalTb,
            Direction::Ltr,
        ));
        assert!(continuation_column_fragment_requires_full_clip(
            1,
            WritingMode::VerticalLr,
            Direction::Ltr,
        ));
        assert!(continuation_column_fragment_requires_full_clip(
            1,
            WritingMode::VerticalRl,
            Direction::Rtl,
        ));
    }

    #[test]
    fn multicol_flow_outcome_composes_column_sets_around_a_spanner() {
        let before = MulticolFlowLayoutOutcome::column_set(layout_pt(24.0), Some(layout_pt(18.0)));
        let after = MulticolFlowLayoutOutcome::column_set(layout_pt(30.0), Some(layout_pt(22.0)));
        let outcome = MulticolFlowLayoutOutcome::column_set(layout_pt(0.0), None)
            .compose_segment(
                layout_pt(0.0),
                before.committed_block_extent(),
                before.final_in_flow_baseline(),
            )
            .compose_segment(layout_pt(24.0), layout_pt(40.0), None)
            .compose_segment(
                layout_pt(64.0),
                after.committed_block_extent(),
                after.final_in_flow_baseline(),
            );

        assert_eq!(outcome.committed_block_extent(), layout_pt(94.0));
        assert_eq!(outcome.final_in_flow_baseline(), Some(layout_pt(86.0)));
    }

    #[test]
    fn spanner_only_multicol_flow_exports_no_baseline() {
        let outcome = MulticolFlowLayoutOutcome::column_set(layout_pt(0.0), None).compose_segment(
            layout_pt(0.0),
            layout_pt(40.0),
            None,
        );

        assert!(outcome.is_multicol_layout());
        assert_eq!(outcome.committed_block_extent(), layout_pt(40.0));
        assert_eq!(outcome.final_in_flow_baseline(), None);
    }
}
