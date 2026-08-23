use std::ops::{Deref, DerefMut};

use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct GridItemEstimate {
    /// Logical inline preferred size, projected to a physical Taffy axis only
    /// at the Grid adapter boundary.
    pub(super) metrics: IntrinsicItemMetrics,
    pub(super) swaps_physical_axes: bool,
    /// Replaced items retain their physical automatic used size separately
    /// from their Grid intrinsic contribution. A `minmax(auto, 0)` track can
    /// suppress the latter without changing the former.
    pub(super) replaced_used_size: Option<ReplacedGridItemUsedSize>,
}

/// What a caller needs from a grid item's intrinsic measurement.
///
/// A subgrid contributes no independent intrinsic size in an inherited axis:
/// its descendants are instead projected into the parent track-sizing pass.
/// Keep that fact at the measurement boundary so intrinsic-only callers do
/// not recursively measure a value that they must subsequently discard.
/// Callers which consume exported baselines retain the complete probe because
/// a grid container baseline can depend on the laid-out descendant baseline.
/// <https://drafts.csswg.org/css-grid-2/#subgrid-item-contribution> and
/// <https://drafts.csswg.org/css-grid-2/#grid-baselines>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GridItemMeasurementRequest {
    contributes_columns: bool,
    contributes_rows: bool,
    export_baselines: bool,
}

impl GridItemMeasurementRequest {
    /// Request the complete estimate used by Grid layout and baseline export.
    fn complete() -> Self {
        Self {
            contributes_columns: true,
            contributes_rows: true,
            export_baselines: true,
        }
    }

    /// Request only the values that can participate in parent track sizing.
    fn parent_track_sizing(style: &ComputedStyle) -> Self {
        Self {
            contributes_columns: !matches!(
                style.grid_template_columns,
                css::GridTrackList::Subgrid { .. }
            ),
            contributes_rows: !matches!(
                style.grid_template_rows,
                css::GridTrackList::Subgrid { .. }
            ),
            export_baselines: false,
        }
    }

    fn has_inherited_axis(self) -> bool {
        !self.contributes_columns || !self.contributes_rows
    }

    fn uses_zeroed_subgrid_contributions(self) -> bool {
        self.has_inherited_axis() && !self.export_baselines
    }

    fn apply_to_estimate(self, estimate: &mut GridItemEstimate) {
        if !self.contributes_columns {
            estimate.metrics.width = content_box_pt(0.0);
            estimate.metrics.min_width = content_box_pt(0.0);
            estimate.metrics.content_width = content_box_pt(0.0);
        }
        if !self.contributes_rows {
            estimate.metrics.height = content_box_pt(0.0);
            estimate.metrics.min_height = content_box_pt(0.0);
            estimate.metrics.content_height = content_box_pt(0.0);
        }
    }
}

/// Physical content-box geometry retained for Grid's final replaced-item
/// sizing phase.
#[derive(Debug, Clone, Copy)]
pub(super) struct ReplacedGridItemUsedSize {
    pub(super) width: PhysicalContentWidth,
    pub(super) height: PhysicalContentHeight,
}

/// Intrinsic grid-container geometry needed when Grid itself participates as
/// a flex item. The values are content-box sizes, with percentage resolution
/// represented separately from the numeric constraints supplied by Flexbox.
///
/// CSS Grid contributes its track-sized min/max content sizes to a parent
/// formatting context; it must not be approximated as an inline text line:
/// <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes>.
pub(in crate::layout) struct GridContainerFlexItemEstimate {
    pub(in crate::layout) min_width: ContentBoxLength,
    pub(in crate::layout) max_width: ContentBoxLength,
    pub(in crate::layout) content_width: ContentBoxLength,
    pub(in crate::layout) intrinsic_height: ContentBoxLength,
    pub(in crate::layout) definite_content_height: Option<ContentBoxLength>,
    pub(in crate::layout) first_baseline: Option<f32>,
    pub(in crate::layout) last_baseline: Option<f32>,
}

impl GridItemEstimate {
    pub(super) fn fixed(width: f32, height: f32) -> Self {
        Self {
            metrics: IntrinsicItemMetrics::fixed(width, height),
            swaps_physical_axes: false,
            replaced_used_size: None,
        }
    }

    /// Convert logical Grid contribution measurements to Taffy's physical x/y
    /// coordinate order. CSS properties remain physical; this conversion is
    /// only for automatic intrinsic measurements.
    /// <https://www.w3.org/TR/css-writing-modes-4/#abstract-box>.
    pub(super) fn physical_measurements(self) -> Self {
        if !self.swaps_physical_axes {
            return self;
        }
        Self {
            metrics: self.metrics.swapped_axes(),
            swaps_physical_axes: false,
            replaced_used_size: self.replaced_used_size,
        }
    }
}

impl Deref for GridItemEstimate {
    type Target = IntrinsicItemMetrics;

    fn deref(&self) -> &Self::Target {
        &self.metrics
    }
}

impl DerefMut for GridItemEstimate {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.metrics
    }
}

impl<'a> LayoutBuilder<'a> {
    /// Estimate a grid container while it participates as a flex item.
    ///
    /// The Flexbox algorithm needs Grid's actual track sizing contributions,
    /// including the zero block-size of an empty grid item. This is distinct
    /// from normal-flow block estimation, whose inline-line fallback is not a
    /// grid intrinsic contribution:
    /// <https://drafts.csswg.org/css-flexbox/#algo-main-item> and
    /// <https://www.w3.org/TR/css-grid-1/#algo-overview>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn estimate_grid_container_for_flex_item(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_width: f32,
        width_basis: GridPercentageBasis,
        height_basis: GridPercentageBasis,
        vertical_non_content: f32,
    ) -> Option<GridContainerFlexItemEstimate> {
        let used_style = self.grid_used_style(style);
        let style: &ComputedStyle = used_style.used_style();
        let built_child_boxes;
        let child_boxes = if let Some(child_boxes) = child_boxes {
            child_boxes
        } else {
            built_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            &built_child_boxes
        };
        let (min_width, max_width) = self.estimate_grid_intrinsic_widths(
            element,
            style,
            stylesheets,
            available_width,
            Some(child_boxes),
        );
        let content_width =
            used_length_percentage_or_auto_with_basis(style.box_values.width.clone(), width_basis)
                .map(|width| width.points())
                .unwrap_or(max_width)
                .max(0.0);
        let definite_content_height = used_content_box_height_or_auto_with_basis(
            style,
            height_basis,
            non_content_pt(vertical_non_content),
        )
        .map(SemanticLengthExt::points);
        let (children, _) = grid_child_lists_from_boxes(child_boxes);
        let children = self.prepare_grid_children(children);
        let layout = self.compute_grid_layout(
            style,
            &children,
            stylesheets,
            PhysicalContentWidth::new(content_box_pt(content_width)),
            definite_content_height
                .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
            GridLayoutPurpose::IntrinsicProbe,
        )?;
        Some(GridContainerFlexItemEstimate {
            min_width: content_box_pt(min_width.max(0.0)),
            max_width: content_box_pt(max_width.max(min_width).max(0.0)),
            content_width: content_box_pt(content_width),
            intrinsic_height: content_box_pt(layout.height.points().max(0.0)),
            definite_content_height: definite_content_height.map(content_box_pt),
            first_baseline: layout.first_baseline,
            last_baseline: layout.last_baseline,
        })
    }

    /// Return a size-contained grid's intrinsic inline sizes.
    ///
    /// Size containment removes grid items from the principal box's intrinsic
    /// sizing input, but an empty grid still has its explicit tracks and gaps.
    /// Keeping that distinction here avoids treating `contain: size` as an
    /// unconditional zero-size shortcut.
    /// <https://www.w3.org/TR/css-contain-1/#containment-size>
    /// <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes>
    pub(in crate::layout) fn size_contained_grid_intrinsic_widths(
        &self,
        style: &ComputedStyle,
    ) -> (f32, f32) {
        let mut explicit_row_count = intrinsic_explicit_grid_track_count(&style.grid_template_rows)
            .unwrap_or(1)
            .max(grid_template_area_row_count(&style.grid_template_areas));
        let row_line_names =
            intrinsic_grid_line_names(&style.grid_template_rows, &style.grid_template_areas)
                .unwrap_or_else(|| vec![Vec::new(); explicit_row_count + 1]);
        explicit_row_count = explicit_row_count.max(row_line_names.len().saturating_sub(1).max(1));
        let (min_width, max_width) = grid_track_list_intrinsic_widths(GridTrackIntrinsicInputs {
            tracks: &style.grid_template_columns,
            auto_tracks: &style.grid_auto_columns,
            areas: &style.grid_template_areas,
            auto_flow: style.grid_auto_flow,
            explicit_row_count,
            row_line_names: &row_line_names,
            children: &[],
            estimates: &[],
            gap: style.column_gap.clone(),
        });
        (min_width.max(0.0), max_width.max(min_width).max(0.0))
    }

    /// Estimate a grid container's min-content and max-content inline widths.
    ///
    /// CSS Grid defines container intrinsic sizes from track sizing with grid
    /// item intrinsic contributions. This is a first Quire-native entrypoint
    /// for parent sizing and shrink-to-fit paths; it handles fixed and basic
    /// intrinsic explicit column tracks while keeping more complex spanning and
    /// flexible-track cases documented as remaining divergences:
    /// <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(in crate::layout) fn estimate_grid_intrinsic_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> (f32, f32) {
        self.estimate_grid_intrinsic_axis_sizes(
            element,
            style,
            stylesheets,
            available_width,
            child_boxes,
            GridIntrinsicAxis::Inline,
        )
    }

    /// Estimate a grid container's intrinsic logical block sizes.
    ///
    /// Grid's rows are its logical block-axis tracks.  A vertical grid uses
    /// those tracks for its physical width, so this probe transposes the
    /// grid-only track and placement representation before reusing the
    /// column-oriented intrinsic track algorithm.  Item measurements remain
    /// in their original writing mode; only the contribution axis changes.
    /// <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>.
    pub(in crate::layout) fn estimate_grid_intrinsic_block_sizes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> (f32, f32) {
        self.estimate_grid_intrinsic_axis_sizes(
            element,
            style,
            stylesheets,
            available_width,
            child_boxes,
            GridIntrinsicAxis::Block,
        )
    }

    fn estimate_grid_intrinsic_axis_sizes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        axis: GridIntrinsicAxis,
    ) -> (f32, f32) {
        let used_style = self.grid_used_style(style);
        let style: &ComputedStyle = used_style.used_style();
        let scrollbar_reservation = ScrollbarGutterReservation::static_pdf_overlay();
        let built_child_boxes;
        let child_boxes = if let Some(child_boxes) = child_boxes {
            child_boxes
        } else {
            built_child_boxes =
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style);
            &built_child_boxes
        };
        let (children, _) = grid_child_lists_from_boxes(child_boxes);
        let children = self.prepare_grid_children(children);
        let estimates = children
            .iter()
            .map(|child| {
                self.estimate_grid_item_size_for_parent_track_sizing(
                    child,
                    stylesheets,
                    available_width,
                    GridPercentageBasis::indefinite(),
                    GridPercentageBasis::indefinite(),
                )
            })
            .collect::<Vec<_>>();
        let transposed_style;
        let transposed_children;
        let transposed_estimates;
        let (style, children, estimates) = match axis {
            GridIntrinsicAxis::Inline => (style, children, estimates),
            GridIntrinsicAxis::Block => {
                transposed_style = transposed_intrinsic_grid_style(style);
                transposed_children = transposed_intrinsic_grid_children(&children);
                transposed_estimates = estimates
                    .into_iter()
                    .zip(children.iter())
                    .map(|(estimate, child)| {
                        estimate.logical_block_contribution(
                            grid_item_logical_block_outer_non_content(&child.style),
                        )
                    })
                    .collect::<Vec<_>>();
                (&transposed_style, transposed_children, transposed_estimates)
            }
        };
        let mut explicit_row_count = intrinsic_explicit_grid_track_count(&style.grid_template_rows)
            .unwrap_or(1)
            .max(grid_template_area_row_count(&style.grid_template_areas));
        let row_line_names =
            intrinsic_grid_line_names(&style.grid_template_rows, &style.grid_template_areas)
                .unwrap_or_else(|| vec![Vec::new(); explicit_row_count + 1]);
        explicit_row_count = explicit_row_count.max(row_line_names.len().saturating_sub(1).max(1));
        let (min_width, max_width) = grid_track_list_intrinsic_widths(GridTrackIntrinsicInputs {
            tracks: &style.grid_template_columns,
            auto_tracks: &style.grid_auto_columns,
            areas: &style.grid_template_areas,
            auto_flow: style.grid_auto_flow,
            explicit_row_count,
            row_line_names: &row_line_names,
            children: &children,
            estimates: &estimates,
            gap: style.column_gap.clone(),
        });
        let scrollbar_extent = intrinsic_grid_scrollbar_axis_extent(
            used_style.used_style(),
            axis,
            scrollbar_reservation,
        )
        .points();
        let min_width = min_width + scrollbar_extent;
        let max_width = max_width + scrollbar_extent;
        (min_width.max(0.0), max_width.max(min_width).max(0.0))
    }
}

/// Which logical Grid track axis supplies an intrinsic contribution.
///
/// The intrinsic-track helper uses a column-oriented placement model; block
/// sizing presents a transposed Grid view at that narrow boundary instead of
/// leaking physical-axis swaps into track sizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridIntrinsicAxis {
    Inline,
    Block,
}

/// The reserved scrollbar space contributes to the Grid container's
/// intrinsic size in the corresponding logical axis.  The reservation itself
/// remains physical because CSS overflow longhands are physical.
/// <https://drafts.csswg.org/css-overflow-3/#scrollbars-layout>
fn intrinsic_grid_scrollbar_axis_extent(
    style: &ComputedStyle,
    axis: GridIntrinsicAxis,
    reservation: ScrollbarGutterReservation,
) -> LayoutLength {
    let inline_is_physical_horizontal =
        !WritingModeAxes::new(style.writing_mode, style.used_direction()).swaps_physical_axes();
    match (axis, inline_is_physical_horizontal) {
        (GridIntrinsicAxis::Inline, true) | (GridIntrinsicAxis::Block, false) => {
            reservation.horizontal_extent()
        }
        (GridIntrinsicAxis::Inline, false) | (GridIntrinsicAxis::Block, true) => {
            reservation.vertical_extent()
        }
    }
}

impl GridItemEstimate {
    /// Present this item's logical block measurements as the intrinsic
    /// track algorithm's logical inline measurements, including the item's
    /// logical block-axis outer contribution required by Grid track sizing.
    fn logical_block_contribution(self, outer_non_content: f32) -> Self {
        let mut metrics = self.metrics.swapped_axes();
        let outer_non_content = outer_non_content.max(0.0);
        metrics.width = content_box_pt(metrics.width.points() + outer_non_content);
        metrics.min_width = content_box_pt(metrics.min_width.points() + outer_non_content);
        metrics.content_width = content_box_pt(metrics.content_width.points() + outer_non_content);
        Self {
            metrics,
            swaps_physical_axes: false,
            replaced_used_size: self.replaced_used_size,
        }
    }
}

/// Return a Grid item's logical block-axis border, padding, and margins.
///
/// The scalar intrinsic track estimator receives its contribution after the
/// item's content probe, so it must add the outer size that Taffy normally
/// applies at its physical Grid boundary.
/// <https://www.w3.org/TR/css-grid-1/#algo-content> and
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>.
fn grid_item_logical_block_outer_non_content(style: &ComputedStyle) -> f32 {
    let metrics = intrinsic_box_metrics(style);
    if style.writing_mode.has_vertical_lines() {
        metrics.horizontal_non_content_length().points()
            + metrics.margin.left.points()
            + metrics.margin.right.points()
    } else {
        metrics.vertical_non_content_length().points()
            + metrics.margin.top.points()
            + metrics.margin.bottom.points()
    }
}

/// Transpose the Grid-only state required by the intrinsic column-track
/// algorithm.  CSS Grid rows and columns exchange their placement, track,
/// gap, auto-flow, and template-area dimensions as one operation.
/// <https://www.w3.org/TR/css-grid-1/#grid-placement-property> and
/// <https://www.w3.org/TR/css-grid-1/#track-sizing>.
fn transposed_intrinsic_grid_style(style: &ComputedStyle) -> ComputedStyle {
    let mut transposed = style.clone();
    std::mem::swap(
        &mut transposed.grid_template_columns,
        &mut transposed.grid_template_rows,
    );
    std::mem::swap(
        &mut transposed.grid_auto_columns,
        &mut transposed.grid_auto_rows,
    );
    std::mem::swap(&mut transposed.column_gap, &mut transposed.row_gap);
    transposed.grid_auto_flow = match transposed.grid_auto_flow {
        css::GridAutoFlow::Row => css::GridAutoFlow::Column,
        css::GridAutoFlow::Column => css::GridAutoFlow::Row,
        css::GridAutoFlow::RowDense => css::GridAutoFlow::ColumnDense,
        css::GridAutoFlow::ColumnDense => css::GridAutoFlow::RowDense,
    };
    transposed.grid_template_areas =
        transposed_grid_template_areas(&transposed.grid_template_areas);
    transposed
}

fn transposed_intrinsic_grid_children<'a>(children: &[GridChild<'a>]) -> Vec<GridChild<'a>> {
    children
        .iter()
        .cloned()
        .map(|mut child| {
            let style = &mut *child.style;
            std::mem::swap(&mut style.grid_column_start, &mut style.grid_row_start);
            std::mem::swap(&mut style.grid_column_end, &mut style.grid_row_end);
            child
        })
        .collect()
}

fn transposed_grid_template_areas(areas: &css::GridTemplateAreas) -> css::GridTemplateAreas {
    let css::GridTemplateAreas::Areas(rows) = areas else {
        return css::GridTemplateAreas::None;
    };
    let column_count = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
    css::GridTemplateAreas::Areas(
        (0..column_count)
            .map(|column| css::GridTemplateAreaRow {
                cells: rows
                    .iter()
                    .map(|row| row.cells.get(column).cloned().unwrap_or(None))
                    .collect(),
            })
            .collect(),
    )
}

impl<'a> LayoutBuilder<'a> {
    /// Estimate one grid item's content-box intrinsic contribution.
    ///
    /// CSS Grid track sizing depends on grid item min-content and max-content
    /// contributions. This helper deliberately reuses Quire's existing inline,
    /// block, flex, table, and replaced-element estimators so grid content
    /// sizing stays aligned with other formatting contexts:
    /// <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>.
    pub(super) fn estimate_grid_item_size(
        &mut self,
        child: &GridChild<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        available_width_basis: GridPercentageBasis,
        available_height_basis: GridPercentageBasis,
    ) -> GridItemEstimate {
        let mut estimate = self.estimate_grid_item_size_with_request(
            child,
            stylesheets,
            available_width,
            available_width_basis,
            available_height_basis,
            GridItemMeasurementRequest::complete(),
        );
        if child
            .element_parts()
            .is_some_and(|(element, _, _)| used_property_containment(element, &child.style).layout)
        {
            // Layout containment suppresses baseline export; grid alignment
            // synthesizes the fallback from the item's border box.
            // <https://www.w3.org/TR/css-contain-1/#containment-layout>
            estimate.metrics.clear_block_baselines();
        }
        estimate
    }

    pub(super) fn estimate_grid_item_size_for_parent_track_sizing(
        &mut self,
        child: &GridChild<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        available_width_basis: GridPercentageBasis,
        available_height_basis: GridPercentageBasis,
    ) -> GridItemEstimate {
        self.estimate_grid_item_size_with_request(
            child,
            stylesheets,
            available_width,
            available_width_basis,
            available_height_basis,
            GridItemMeasurementRequest::parent_track_sizing(&child.style),
        )
    }

    fn estimate_grid_item_size_with_request(
        &mut self,
        child: &GridChild<'_>,
        stylesheets: &Stylesheets<'_>,
        available_width: f32,
        available_width_basis: GridPercentageBasis,
        available_height_basis: GridPercentageBasis,
        request: GridItemMeasurementRequest,
    ) -> GridItemEstimate {
        let layout_style = grid_item_layout_style(&child.style);
        let style = &layout_style;
        let available_height = available_height_basis.points();
        let axes = WritingModeAxes::new(style.writing_mode, style.used_direction());
        let inline_available = if axes.swaps_physical_axes() {
            available_height.unwrap_or(available_width)
        } else {
            available_width
        }
        .max(1.0);
        let inline_basis = if axes.swaps_physical_axes() {
            available_height_basis
        } else {
            available_width_basis
        };
        let block_basis =
            available_height_basis.map_value(|height| content_box_pt(height.points().max(1.0)));

        if let Some(children) = child.anonymous_content() {
            let measurement = self.intrinsic_inline_measurement_for_boxes(
                children,
                style,
                stylesheets,
                inline_available,
            );
            let mut estimate = grid_item_estimate_from_intrinsic(
                style,
                available_width,
                inline_basis,
                block_basis,
                measurement.contribution.min_content.points(),
                measurement.contribution.max_content.points(),
                measurement.height().max(style.line_height),
            );
            set_grid_item_text_baselines(
                &mut estimate,
                style,
                measurement.sequence.first_line_baseline_offset(
                    self.inline_box_text_line_layout_baseline_offset(style),
                ),
                measurement.sequence.last_line_baseline_offset(
                    self.inline_box_text_line_layout_baseline_offset(style),
                ),
                measurement.line_count(),
            );
            return estimate;
        }

        let Some((element, signature, child_boxes)) = child.element_parts() else {
            return GridItemEstimate::fixed(0.0, 0.0);
        };

        self.with_ancestor_signature(signature.clone(), |layout| {
            if replaced_element_kind(element) == Some(ReplacedElementKind::Image)
                && let Some(image) = used_image(
                    element,
                    style,
                    available_width,
                    block_size_percentage_basis_from_points(
                        block_basis.points(),
                        BlockSizeBasisSource::GridItem,
                    ),
                    layout.base_url,
                    layout.root_url,
                    layout.resource_cache,
                )
            {
                let content_size = image.content_size;
                let (inline_size, block_size) = grid_item_logical_sizes_from_physical(
                    style,
                    content_size.width.max(1.0),
                    content_size.height.max(1.0),
                );
                let mut estimate = grid_item_estimate_from_intrinsic(
                    style,
                    available_width,
                    inline_basis,
                    block_basis,
                    inline_size,
                    inline_size,
                    block_size,
                );
                estimate.replaced_used_size = Some(ReplacedGridItemUsedSize {
                    width: PhysicalContentWidth::new(content_box_pt(content_size.width)),
                    height: PhysicalContentHeight::new(content_box_pt(content_size.height)),
                });
                return estimate;
            }

            if replaced_element_kind(element) == Some(ReplacedElementKind::Canvas) {
                let canvas = used_canvas(
                    element,
                    style,
                    available_width,
                    block_size_percentage_basis_from_points(
                        block_basis.points(),
                        BlockSizeBasisSource::GridItem,
                    ),
                );
                let content_size = canvas.content_size;
                let (inline_size, block_size) = grid_item_logical_sizes_from_physical(
                    style,
                    content_size.width.max(1.0),
                    content_size.height.max(1.0),
                );
                let mut estimate = grid_item_estimate_from_intrinsic(
                    style,
                    available_width,
                    inline_basis,
                    block_basis,
                    inline_size,
                    inline_size,
                    block_size,
                );
                estimate.replaced_used_size = Some(ReplacedGridItemUsedSize {
                    width: PhysicalContentWidth::new(content_box_pt(content_size.width)),
                    height: PhysicalContentHeight::new(content_box_pt(content_size.height)),
                });
                return estimate;
            }

            if replaced_element_kind(element) == Some(ReplacedElementKind::Svg)
                && let Some(svg) = used_svg(
                    element,
                    style,
                    available_width,
                    block_size_percentage_basis_from_points(
                        block_basis.points(),
                        BlockSizeBasisSource::GridItem,
                    ),
                )
            {
                let width = svg.content_size.width;
                let height = svg.content_size.height;
                let (inline_size, block_size) =
                    grid_item_logical_sizes_from_physical(style, width.max(1.0), height.max(1.0));
                let mut estimate = grid_item_estimate_from_intrinsic(
                    style,
                    available_width,
                    inline_basis,
                    block_basis,
                    inline_size,
                    inline_size,
                    block_size,
                );
                estimate.replaced_used_size = Some(ReplacedGridItemUsedSize {
                    width: PhysicalContentWidth::new(content_box_pt(width)),
                    height: PhysicalContentHeight::new(content_box_pt(height)),
                });
                return estimate;
            }

            if style.display.inner == DisplayInner::Grid
                && let Some(child_boxes) = child_boxes
            {
                // A size-contained grid is measured as an empty grid by its
                // parent.  Its real items are still formatted during final
                // layout, after the principal box has received that used
                // size.  In particular, implicit tracks must not acquire a
                // parent-facing contribution from those real items.
                //
                // <https://www.w3.org/TR/css-contain-1/#containment-size>
                let inherited_columns = matches!(
                    style.grid_template_columns,
                    css::GridTrackList::Subgrid { .. }
                );
                let inherited_rows =
                    matches!(style.grid_template_rows, css::GridTrackList::Subgrid { .. });
                let fast_subgrid_measurement = request.uses_zeroed_subgrid_contributions();
                let (grid_min, grid_max) = if fast_subgrid_measurement && inherited_columns {
                    (0.0, 0.0)
                } else if intrinsic_physical_width_is_contained(style) {
                    layout.size_contained_grid_intrinsic_widths(style)
                } else {
                    layout.estimate_grid_intrinsic_widths(
                        element,
                        style,
                        stylesheets,
                        available_width,
                        Some(child_boxes),
                    )
                };
                let intrinsic_metrics = intrinsic_box_metrics(style);
                let horizontal_margin =
                    intrinsic_metrics.margin.left + intrinsic_metrics.margin.right;
                let horizontal_non_content = intrinsic_metrics.horizontal_non_content_length();
                let requested_content_width =
                    crate::layout::intrinsic::content_box_width_from_intrinsic_in_margin_box(
                        style,
                        layout_pt(available_width),
                        horizontal_margin,
                        horizontal_non_content,
                        content_box_pt(grid_min),
                        content_box_pt(grid_max),
                        crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                    );
                let content_width = constrain_content_width(
                    style,
                    requested_content_width,
                    PercentageBasis::definite(layout_pt(available_width.max(0.0))),
                )
                .points();
                let vertical_extras = intrinsic_metrics.margin.top.points()
                    + intrinsic_metrics.margin.bottom.points()
                    + intrinsic_metrics.vertical_non_content_length().points();
                let definite_content_height = used_content_box_height_or_auto(
                    style,
                    layout_pt(style.line_height.max(1.0)),
                    non_content_pt(vertical_extras),
                )
                .map(SemanticLengthExt::points)
                .map(|height| {
                    constrain_content_height(
                        style,
                        content_box_pt(height),
                        PercentageBasis::definite(layout_pt(available_width)),
                    )
                    .points()
                });
                let (grid_children, _) = grid_child_lists_from_boxes(child_boxes);
                let grid_children = layout.prepare_grid_children(grid_children);
                // This is the parent-facing intrinsic sizing pass, so size
                // containment must remove the same items from both grid axes.
                // The later final-layout pass still receives `grid_children`.
                let intrinsic_grid_children: &[GridChild<'_>] =
                    if used_property_containment(element, style).size {
                        &[] as &[GridChild<'_>]
                    } else {
                        grid_children.as_slice()
                    };
                // A subgrid has no independent contribution in its inherited
                // axis, but its standalone axis is still an ordinary grid
                // axis.  In particular, it cannot borrow an unresolved
                // `subgrid` layout pass to discover that standalone axis:
                // the parent-track context exists only after placement.
                // Measure that axis directly from the grid's own tracks.
                // <https://drafts.csswg.org/css-grid-2/#subgrids>
                let standalone_block_size = (inherited_columns && !inherited_rows).then(|| {
                    layout
                        .estimate_grid_intrinsic_block_sizes(
                            element,
                            style,
                            stylesheets,
                            available_width,
                            Some(child_boxes),
                        )
                        .1
                });
                let grid_layout = (!fast_subgrid_measurement)
                    .then(|| {
                        layout.compute_grid_layout(
                            style,
                            intrinsic_grid_children,
                            stylesheets,
                            PhysicalContentWidth::new(content_box_pt(content_width)),
                            definite_content_height
                                .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                            GridLayoutPurpose::IntrinsicProbe,
                        )
                    })
                    .flatten();
                let content_height = if fast_subgrid_measurement && inherited_rows {
                    0.0
                } else {
                    standalone_block_size.unwrap_or_else(|| {
                        grid_layout
                            .as_ref()
                            .map_or(0.0, |layout| layout.height.points())
                    })
                };
                let content_height =
                    constrain_grid_intrinsic_height(style, content_height, block_basis);
                let mut estimate = grid_item_estimate_from_intrinsic(
                    style,
                    available_width,
                    inline_basis,
                    block_basis,
                    grid_min,
                    grid_max,
                    content_height,
                );
                if let Some(grid_layout) = grid_layout {
                    let baseline_offset =
                        (intrinsic_metrics.border.top + intrinsic_metrics.padding.top).points();
                    estimate.first_baseline = grid_layout
                        .first_baseline
                        .map(|baseline| baseline_offset + baseline);
                    estimate.last_baseline = grid_layout
                        .last_baseline
                        .map(|baseline| baseline_offset + baseline);
                    // An unresolved inherited row axis has no independent
                    // zero-distance first baseline. Its final shared row
                    // supplies the stretch, while the empty standalone
                    // formatting context still synthesizes its first inline
                    // baseline from the used line height.
                    // <https://drafts.csswg.org/css-grid-2/#subgrids>
                    // <https://drafts.csswg.org/css-align-3/#synthesize-baseline>
                    if matches!(style.grid_template_rows, css::GridTrackList::Subgrid { .. })
                        && estimate
                            .first_baseline
                            .is_some_and(|baseline| (baseline - baseline_offset).abs() < 0.01)
                    {
                        estimate.first_baseline = Some(baseline_offset + style.line_height);
                    }
                }
                if fast_subgrid_measurement {
                    request.apply_to_estimate(&mut estimate);
                }
                return estimate;
            }

            // During Grid's row-to-column feedback pass the item's grid area
            // is a definite containing block.  Make that basis visible to
            // nested atomic/replaced descendants while measuring the item's
            // inline contribution: a descendant `height: 100%` can transfer
            // through its aspect ratio into that contribution.
            // <https://drafts.csswg.org/css-grid-2/#algo-track-sizing>
            layout
                .definite_block_size_stack
                .push(block_size_percentage_basis_from_points(
                    block_basis.points(),
                    BlockSizeBasisSource::GridItem,
                ));
            let inline_measurement = layout.intrinsic_inline_measurement_for_element(
                element,
                style,
                stylesheets,
                child_boxes,
                inline_available,
            );
            let mut min_content = inline_measurement.contribution.min_content.points();
            let mut max_content = inline_measurement.contribution.max_content.points();
            if min_content == 0.0 && max_content == 0.0 {
                let (block_min, block_max) = layout.block_intrinsic_content_widths(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    available_width,
                );
                min_content = block_min;
                max_content = block_max;
            }
            let content_height = layout
                .estimate_element_height(element, style, stylesheets, available_width, child_boxes)
                .map(|height| {
                    let intrinsic_metrics = intrinsic_box_metrics(style);
                    let vertical_non_content = intrinsic_metrics.margin.top.points()
                        + intrinsic_metrics.margin.bottom.points()
                        + intrinsic_metrics.vertical_non_content_length().points();
                    (height - vertical_non_content).max(0.0)
                })
                .unwrap_or_else(|| inline_measurement.height().max(style.line_height));
            let content_height = if axes.swaps_physical_axes() {
                // `GridItemEstimate` carries logical inline/block metrics.
                // The ordinary height probe is physical, so it is an inline
                // extent in vertical writing modes and cannot size a row.
                // Ask the block intrinsic model for the physical-width
                // projection, which is precisely this item's logical block
                // contribution.
                // <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes> and
                // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
                layout
                    .block_intrinsic_content_sizes(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        available_width,
                    )
                    .physical_width_min_max(FlowAxes::for_style(style))
                    .1
                    .points()
            } else {
                content_height
            };
            // A tree-abiding generated pseudo can have an empty formatting
            // child list even though its `content` produces an inline line.
            // Its element-height probe then reports the empty principal box;
            // retain the generated inline line for this grid's *internal*
            // track sizing. Parent-facing size containment still uses the
            // separate empty-grid pass.
            // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
            // <https://www.w3.org/TR/css-grid-1/#track-sizing>
            let content_height = if style.content.is_generated() {
                content_height.max(inline_measurement.height().max(style.line_height))
            } else {
                content_height
            };

            layout.definite_block_size_stack.pop();

            let mut estimate = grid_item_estimate_from_intrinsic(
                style,
                available_width,
                inline_basis,
                block_basis,
                min_content,
                max_content,
                content_height,
            );
            set_grid_item_text_baselines(
                &mut estimate,
                style,
                inline_measurement.sequence.first_line_baseline_offset(
                    layout.inline_box_text_line_layout_baseline_offset(style),
                ),
                inline_measurement.sequence.last_line_baseline_offset(
                    layout.inline_box_text_line_layout_baseline_offset(style),
                ),
                inline_measurement.line_count(),
            );
            estimate
        })
    }
}

/// Project an intrinsic replaced object's physical size into a Grid item's
/// logical inline/block measurement pair.
/// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>.
fn grid_item_logical_sizes_from_physical(
    style: &ComputedStyle,
    physical_width: f32,
    physical_height: f32,
) -> (f32, f32) {
    if WritingModeAxes::new(style.writing_mode, style.used_direction()).swaps_physical_axes() {
        (physical_height, physical_width)
    } else {
        (physical_width, physical_height)
    }
}

struct GridTrackIntrinsicInputs<'a, 'grid> {
    tracks: &'a css::GridTrackList,
    auto_tracks: &'a css::GridAutoTrackList,
    areas: &'a css::GridTemplateAreas,
    auto_flow: css::GridAutoFlow,
    explicit_row_count: usize,
    row_line_names: &'a [Vec<String>],
    children: &'a [GridChild<'grid>],
    estimates: &'a [GridItemEstimate],
    gap: css::ComputedGap,
}

fn grid_track_list_intrinsic_widths(inputs: GridTrackIntrinsicInputs<'_, '_>) -> (f32, f32) {
    let fallback = grid_item_intrinsic_contribution(inputs.estimates.iter().cloned());
    let Some(expanded) = intrinsic_grid_columns(&inputs) else {
        return fallback;
    };
    let track_sizes = &expanded.sizes;
    if track_sizes.is_empty() {
        return fallback;
    }
    // Track contribution distribution is scalar coordinate arithmetic; keep
    // the CSS gap typed until it enters that algorithm.
    let gap_width = intrinsic_grid_gap_size(inputs.gap).points();
    let contributions = grid_track_intrinsic_contributions(GridTrackContributionInputs {
        track_count: track_sizes.len(),
        explicit_column_count: expanded.explicit_track_count,
        first_line_index: expanded.first_line_index,
        line_names: &expanded.line_names,
        auto_flow: inputs.auto_flow,
        explicit_row_count: inputs.explicit_row_count,
        row_line_names: inputs.row_line_names,
        children: inputs.children,
        estimates: inputs.estimates,
        gap_width,
    });
    let track_widths = track_sizes
        .iter()
        .zip(contributions)
        .map(|(size, (item_min, item_max))| {
            grid_track_intrinsic_width(size.clone(), item_min, item_max)
        })
        .collect::<Vec<_>>();
    let min_width = track_widths.iter().map(|(min, _)| min).sum::<f32>();
    let max_width = track_widths.iter().map(|(_, max)| max).sum::<f32>();
    let gap_count = track_widths.len().saturating_sub(1);
    let total_gap_width = gap_width * gap_count as f32;
    (min_width + total_gap_width, max_width + total_gap_width)
}

fn intrinsic_grid_columns(inputs: &GridTrackIntrinsicInputs<'_, '_>) -> Option<ExpandedGridTracks> {
    match inputs.tracks {
        css::GridTrackList::Tracks {
            components,
            trailing_names,
        } if !components.is_empty() => {
            explicit_grid_columns_for_intrinsic_width(components, trailing_names, inputs)
        }
        css::GridTrackList::None => implicit_grid_columns_for_intrinsic_width(inputs),
        _ => None,
    }
}

/// Expand explicit column tracks for grid container intrinsic sizing.
///
/// CSS Grid's explicit grid is the larger of the authored track list and
/// `grid-template-areas`. Area-created columns not sized by
/// `grid-template-columns` take their sizes from `grid-auto-columns`:
/// <https://www.w3.org/TR/css-grid-1/#explicit-grids>.
fn explicit_grid_columns_for_intrinsic_width(
    components: &[css::GridTrackListComponent],
    trailing_names: &[String],
    inputs: &GridTrackIntrinsicInputs<'_, '_>,
) -> Option<ExpandedGridTracks> {
    let mut expanded =
        expanded_grid_tracks_for_axis(components, trailing_names, inputs.areas, GridAxis::Column)?;
    append_area_created_grid_columns(&mut expanded, inputs.areas, inputs.auto_tracks);
    expanded.explicit_track_count = expanded.sizes.len();
    expand_simple_implicit_grid_columns(&mut expanded, inputs);
    Some(expanded)
}

fn append_area_created_grid_columns(
    expanded: &mut ExpandedGridTracks,
    areas: &css::GridTemplateAreas,
    auto_tracks: &css::GridAutoTrackList,
) {
    let area_column_count = grid_template_area_column_count(areas);
    let authored_column_count = expanded.sizes.len();
    if area_column_count <= authored_column_count {
        return;
    }
    expanded
        .sizes
        .extend((0..area_column_count - authored_column_count).map(|index| {
            auto_tracks
                .get(index % auto_tracks.len())
                .expect("grid auto-track list is non-empty")
                .clone()
        }));
    expanded
        .line_names
        .resize_with(expanded.sizes.len() + 1, Vec::new);
}

/// Add simple implicit columns required before or past the explicit grid.
///
/// CSS Grid creates implicit tracks when placement puts items outside the
/// explicit grid, and those tracks are sized by `grid-auto-columns`:
/// <https://www.w3.org/TR/css-grid-1/#implicit-grids>.
fn expand_simple_implicit_grid_columns(
    expanded: &mut ExpandedGridTracks,
    inputs: &GridTrackIntrinsicInputs<'_, '_>,
) {
    let explicit_column_count = expanded.sizes.len();
    let Some(extent) = simple_implicit_column_extent(
        inputs.auto_flow,
        inputs.explicit_row_count,
        explicit_column_count,
        &expanded.line_names,
        inputs.row_line_names,
        inputs.children,
    ) else {
        return;
    };
    prepend_implicit_grid_columns(expanded, extent.first_line, inputs.auto_tracks);
    append_implicit_grid_columns(expanded, extent.end_line, inputs.auto_tracks);
    expanded
        .line_names
        .resize_with(expanded.sizes.len() + 1, Vec::new);
}

fn prepend_implicit_grid_columns(
    expanded: &mut ExpandedGridTracks,
    first_line: i32,
    auto_tracks: &css::GridAutoTrackList,
) {
    if first_line >= expanded.first_line_index {
        return;
    }
    let before_count = usize::try_from(expanded.first_line_index - first_line).unwrap_or(0);
    let mut sizes = (0..before_count)
        .map(|index| {
            let distance_from_explicit = before_count - index;
            cycled_auto_track_size_before(auto_tracks, distance_from_explicit)
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    sizes.extend(expanded.sizes.iter().cloned());
    expanded.sizes = sizes;
    let mut line_names = vec![Vec::new(); before_count];
    line_names.extend(expanded.line_names.iter().cloned());
    expanded.line_names = line_names;
    expanded.first_line_index = first_line;
}

fn append_implicit_grid_columns(
    expanded: &mut ExpandedGridTracks,
    end_line: i32,
    auto_tracks: &css::GridAutoTrackList,
) {
    let Some(current_end_line) = expanded
        .first_line_index
        .checked_add(i32::try_from(expanded.sizes.len()).ok().unwrap_or(0))
    else {
        return;
    };
    if end_line <= current_end_line {
        return;
    }
    let Some(explicit_end_line) = i32::try_from(expanded.explicit_track_count)
        .ok()
        .and_then(|count| count.checked_add(1))
    else {
        return;
    };
    let after_count = usize::try_from(end_line - current_end_line).unwrap_or(0);
    expanded.sizes.extend((0..after_count).filter_map(|index| {
        let track_line = current_end_line.checked_add(i32::try_from(index).ok()?)?;
        let auto_index = usize::try_from(track_line.checked_sub(explicit_end_line)?).ok()?;
        auto_tracks.get(auto_index % auto_tracks.len()).cloned()
    }));
}

/// Expand simple implicit column tracks for grid container intrinsic sizing.
///
/// CSS Grid's explicit grid is sized by the larger of the authored track list
/// and `grid-template-areas`; area-created columns without explicit
/// `grid-template-columns` sizes take their sizes from `grid-auto-columns`.
/// CSS Grid creates implicit tracks when auto-placement places items outside
/// the explicit grid, and sizes those tracks from `grid-auto-columns`:
/// <https://www.w3.org/TR/css-grid-1/#explicit-grids> and
/// <https://www.w3.org/TR/css-grid-1/#implicit-grids>.
fn implicit_grid_columns_for_intrinsic_width(
    inputs: &GridTrackIntrinsicInputs<'_, '_>,
) -> Option<ExpandedGridTracks> {
    let area_column_count = grid_template_area_column_count(inputs.areas);
    let mut line_names = vec![Vec::new(); area_column_count + 1];
    add_generated_area_line_names(&mut line_names, inputs.areas, GridAxis::Column);
    let extent = simple_implicit_column_extent(
        inputs.auto_flow,
        inputs.explicit_row_count,
        area_column_count,
        &line_names,
        inputs.row_line_names,
        inputs.children,
    )?;
    if extent.track_count() == 0 {
        return None;
    }
    let sizes = (0..area_column_count)
        .map(|index| {
            inputs
                .auto_tracks
                .get(index % inputs.auto_tracks.len())
                .expect("grid auto-track list is non-empty")
                .clone()
        })
        .collect::<Vec<_>>();
    let mut expanded = ExpandedGridTracks {
        explicit_track_count: area_column_count,
        sizes,
        line_names,
        first_line_index: 1,
    };
    prepend_implicit_grid_columns(&mut expanded, extent.first_line, inputs.auto_tracks);
    append_implicit_grid_columns(&mut expanded, extent.end_line, inputs.auto_tracks);
    expanded
        .line_names
        .resize_with(expanded.sizes.len() + 1, Vec::new);
    Some(expanded)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntrinsicColumnExtent {
    first_line: i32,
    end_line: i32,
}

impl IntrinsicColumnExtent {
    fn track_count(self) -> usize {
        usize::try_from(self.end_line.saturating_sub(self.first_line)).unwrap_or(0)
    }
}

/// Count implicit columns for simple grid item placement.
///
/// This intentionally handles template-area explicit columns, all-auto
/// placement, positive numeric column lines, positive named column lines, and
/// named spans that search past the end of the explicit grid. Negative and
/// more complex implicit-line cases still fall back to the conservative aggregate
/// intrinsic contribution until Quire has a fuller implicit-grid placement
/// model:
/// <https://www.w3.org/TR/css-grid-1/#explicit-grids> and
/// <https://www.w3.org/TR/css-grid-1/#auto-placement-algo>.
#[cfg(test)]
fn simple_implicit_column_count(
    auto_flow: css::GridAutoFlow,
    explicit_row_count: usize,
    explicit_column_count: usize,
    column_line_names: &[Vec<String>],
    row_line_names: &[Vec<String>],
    children: &[GridChild<'_>],
) -> Option<usize> {
    simple_implicit_column_extent(
        auto_flow,
        explicit_row_count,
        explicit_column_count,
        column_line_names,
        row_line_names,
        children,
    )
    .map(IntrinsicColumnExtent::track_count)
}

fn simple_implicit_column_extent(
    auto_flow: css::GridAutoFlow,
    explicit_row_count: usize,
    explicit_column_count: usize,
    column_line_names: &[Vec<String>],
    row_line_names: &[Vec<String>],
    children: &[GridChild<'_>],
) -> Option<IntrinsicColumnExtent> {
    let mut span_sum = 0_usize;
    let mut max_auto_span = 0_usize;
    let mut constrained_row_spans = vec![0_usize; explicit_row_count.max(1)];
    let mut definite_track_count = 0_usize;
    let mut first_line = 1_i32;
    let mut end_line = i32::try_from(explicit_column_count).ok()?.checked_add(1)?;
    for child in children {
        if let Some(span) = simple_grid_child_auto_column_span(&child.style) {
            let row_constraint =
                simple_grid_child_row_constraint(&child.style, explicit_row_count, row_line_names)?;
            if let Some(rows) = row_constraint {
                for row in rows {
                    constrained_row_spans[row] = constrained_row_spans[row].checked_add(span)?;
                }
            }
            span_sum = span_sum.checked_add(span)?;
            max_auto_span = max_auto_span.max(span);
            continue;
        }
        if let Some(count) = simple_positive_numeric_implicit_column_count(&child.style) {
            definite_track_count = definite_track_count.max(count);
        } else if let Some(count) = simple_positive_named_implicit_column_count(
            &child.style,
            explicit_column_count,
            column_line_names,
        ) {
            definite_track_count = definite_track_count.max(count);
        } else if let Some(count) = simple_forward_named_span_implicit_column_count(
            &child.style,
            explicit_column_count,
            column_line_names,
        ) {
            definite_track_count = definite_track_count.max(count);
        } else if let Some(range) = simple_grid_child_column_line_range(
            &child.style,
            explicit_column_count,
            column_line_names,
        ) {
            first_line = first_line.min(range.start);
            end_line = end_line.max(range.end);
        } else if explicit_column_count == 0 {
            return None;
        }
    }
    let auto_track_count = match auto_flow {
        css::GridAutoFlow::Row | css::GridAutoFlow::RowDense => {
            let constrained_count = constrained_row_spans.iter().cloned().max().unwrap_or(0);
            // CSS Grid creates the implicit columns before it auto-places
            // items with automatic positions in both axes. Those items can
            // create rows, but only their largest column span can enlarge the
            // implicit column extent. Items pre-placed into a definite row
            // have already made their column demand definite.
            // <https://www.w3.org/TR/css-grid-1/#auto-placement-algo>
            Some(
                explicit_column_count
                    .max(max_auto_span)
                    .max(constrained_count),
            )
        }
        css::GridAutoFlow::Column | css::GridAutoFlow::ColumnDense => {
            if span_sum == 0 {
                let auto_end_line = i32::try_from(definite_track_count.max(explicit_column_count))
                    .ok()?
                    .checked_add(1)?;
                end_line = end_line.max(auto_end_line);
                return (first_line <= end_line).then_some(IntrinsicColumnExtent {
                    first_line,
                    end_line,
                });
            }
            let row_count = explicit_row_count.max(1);
            let unconstrained_count = span_sum.div_ceil(row_count).max(1);
            let constrained_count = constrained_row_spans.into_iter().max().unwrap_or(0);
            Some(
                explicit_column_count
                    .max(unconstrained_count)
                    .max(constrained_count),
            )
        }
    }?;
    let auto_end_line = i32::try_from(definite_track_count.max(auto_track_count))
        .ok()?
        .checked_add(1)?;
    end_line = end_line.max(auto_end_line);
    (first_line <= end_line).then_some(IntrinsicColumnExtent {
        first_line,
        end_line,
    })
}

/// Return the `grid-auto-columns` size for a startward implicit track.
///
/// CSS Grid applies the auto-track list backward before the explicit grid, so
/// the first implicit track before the explicit grid receives the last
/// `grid-auto-columns` size:
/// <https://www.w3.org/TR/css-grid-1/#auto-tracks>.
fn cycled_auto_track_size_before(
    auto_tracks: &css::GridAutoTrackList,
    distance_from_explicit: usize,
) -> Option<css::GridTrackSize> {
    let len = auto_tracks.len();
    let offset = distance_from_explicit % len;
    let index = (len - offset) % len;
    auto_tracks.get(index).cloned()
}

fn simple_positive_numeric_implicit_column_count(style: &ComputedStyle) -> Option<usize> {
    if let Some(count) = simple_positive_numeric_forward_column_count(style) {
        return Some(count);
    }
    simple_positive_numeric_backward_column_count(style)
}

fn simple_positive_named_implicit_column_count(
    style: &ComputedStyle,
    explicit_column_count: usize,
    line_names: &[Vec<String>],
) -> Option<usize> {
    if let Some(count) =
        simple_positive_named_forward_column_count(style, explicit_column_count, line_names)
    {
        return Some(count);
    }
    simple_positive_named_backward_column_count(style, explicit_column_count, line_names)
}

fn simple_positive_named_forward_column_count(
    style: &ComputedStyle,
    explicit_column_count: usize,
    line_names: &[Vec<String>],
) -> Option<usize> {
    let explicit_line_count = explicit_column_count.checked_add(1)?;
    let start =
        positive_named_grid_line_index(&style.grid_column_start, line_names, explicit_line_count)?;
    let span = match &style.grid_column_end {
        css::GridPlacement::Auto => 1,
        css::GridPlacement::Span(span) if span.name().is_none() => {
            span.count().map(usize::from).filter(|span| *span > 0)?
        }
        css::GridPlacement::Line(_) => {
            let end = positive_named_grid_line_index(
                &style.grid_column_end,
                line_names,
                explicit_line_count,
            )?;
            return (end > start).then_some(end - 1);
        }
        css::GridPlacement::Span(_) => return None,
    };
    start.checked_add(span)?.checked_sub(1)
}

fn simple_positive_named_backward_column_count(
    style: &ComputedStyle,
    explicit_column_count: usize,
    line_names: &[Vec<String>],
) -> Option<usize> {
    let explicit_line_count = explicit_column_count.checked_add(1)?;
    let end =
        positive_named_grid_line_index(&style.grid_column_end, line_names, explicit_line_count)?;
    match &style.grid_column_start {
        css::GridPlacement::Auto => Some(end.checked_sub(1)?),
        css::GridPlacement::Span(span) if span.name().is_none() => {
            let span = span.count().map(usize::from).filter(|span| *span > 0)?;
            (end > span).then_some(end - 1)
        }
        css::GridPlacement::Span(_) | css::GridPlacement::Line(_) => None,
    }
}

fn simple_forward_named_span_implicit_column_count(
    style: &ComputedStyle,
    explicit_column_count: usize,
    line_names: &[Vec<String>],
) -> Option<usize> {
    let start = intrinsic_column_line_index(
        &style.grid_column_start,
        line_names,
        explicit_column_count,
        1,
    )?;
    let css::GridPlacement::Span(span) = &style.grid_column_end else {
        return None;
    };
    span.name()?;
    let span =
        simple_grid_named_column_span_after(span, start, line_names, explicit_column_count, 1)?;
    usize::try_from(start)
        .ok()?
        .checked_add(span)?
        .checked_sub(1)
}

/// Resolve a positive named line for intrinsic implicit-column sizing.
///
/// CSS Grid treats implicit lines after the explicit grid as having the
/// requested name when there are not enough matching explicit lines:
/// <https://www.w3.org/TR/css-grid-1/#grid-placement-errors>.
fn positive_named_grid_line_index(
    placement: &css::GridPlacement,
    line_names: &[Vec<String>],
    explicit_line_count: usize,
) -> Option<usize> {
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    let name = line.name()?;
    let occurrence = line.index().unwrap_or(1);
    if occurrence <= 0 {
        return None;
    }
    let target = u32::try_from(occurrence).ok()?;
    let mut matches_seen = 0_u32;
    for (index, names) in line_names.iter().take(explicit_line_count).enumerate() {
        if names.iter().any(|line_name| line_name == name) {
            matches_seen += 1;
            if matches_seen == target {
                return Some(index + 1);
            }
        }
    }
    let remaining = usize::try_from(target - matches_seen).ok()?;
    explicit_line_count.checked_add(remaining)
}

fn simple_positive_numeric_forward_column_count(style: &ComputedStyle) -> Option<usize> {
    let start = positive_numeric_grid_line(&style.grid_column_start)?;
    let span = match &style.grid_column_end {
        css::GridPlacement::Auto => 1,
        css::GridPlacement::Span(span) if span.name().is_none() => {
            span.count().map(usize::from).filter(|span| *span > 0)?
        }
        css::GridPlacement::Line(_) => {
            let end = positive_numeric_grid_line(&style.grid_column_end)?;
            return (end > start).then_some(end - 1);
        }
        css::GridPlacement::Span(_) => return None,
    };
    start.checked_add(span)?.checked_sub(1)
}

fn simple_positive_numeric_backward_column_count(style: &ComputedStyle) -> Option<usize> {
    let end = positive_numeric_grid_line(&style.grid_column_end)?;
    match &style.grid_column_start {
        css::GridPlacement::Auto => Some(end.checked_sub(1)?),
        css::GridPlacement::Span(span) if span.name().is_none() => {
            let span = span.count().map(usize::from).filter(|span| *span > 0)?;
            (end > span).then_some(end - 1)
        }
        css::GridPlacement::Span(_) | css::GridPlacement::Line(_) => None,
    }
}

fn positive_numeric_grid_line(placement: &css::GridPlacement) -> Option<usize> {
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    if line.name().is_some() {
        return None;
    }
    line.index()
        .filter(|index| *index > 0)
        .and_then(|index| usize::try_from(index).ok())
}

#[derive(Debug, Clone, PartialEq)]
struct ExpandedGridTracks {
    sizes: Vec<css::GridTrackSize>,
    line_names: Vec<Vec<String>>,
    explicit_track_count: usize,
    first_line_index: i32,
}

fn expanded_grid_tracks(
    components: &[css::GridTrackListComponent],
    trailing_names: &[String],
    areas: &css::GridTemplateAreas,
) -> Option<ExpandedGridTracks> {
    expanded_grid_tracks_for_axis(components, trailing_names, areas, GridAxis::Column)
}

fn expanded_grid_tracks_for_axis(
    components: &[css::GridTrackListComponent],
    trailing_names: &[String],
    areas: &css::GridTemplateAreas,
    axis: GridAxis,
) -> Option<ExpandedGridTracks> {
    let mut sizes = Vec::new();
    let mut line_names = Vec::new();
    let mut current_line_names = Vec::new();
    collect_expanded_grid_tracks(
        components,
        &mut current_line_names,
        &mut sizes,
        &mut line_names,
    )?;
    current_line_names.extend(trailing_names.iter().cloned());
    line_names.push(current_line_names);
    add_generated_area_line_names(&mut line_names, areas, axis);
    Some(ExpandedGridTracks {
        explicit_track_count: sizes.len(),
        sizes,
        line_names,
        first_line_index: 1,
    })
}

fn intrinsic_grid_line_names(
    tracks: &css::GridTrackList,
    areas: &css::GridTemplateAreas,
) -> Option<Vec<Vec<String>>> {
    match tracks {
        css::GridTrackList::Tracks {
            components,
            trailing_names,
        } if !components.is_empty() => {
            expanded_grid_tracks_for_axis(components, trailing_names, areas, GridAxis::Row)
                .map(|expanded| expanded.line_names)
        }
        css::GridTrackList::None => {
            let row_count = grid_template_area_row_count(areas).max(1);
            let mut line_names = vec![Vec::new(); row_count + 1];
            add_generated_area_line_names(&mut line_names, areas, GridAxis::Row);
            Some(line_names)
        }
        _ => None,
    }
}

fn collect_expanded_grid_tracks(
    components: &[css::GridTrackListComponent],
    current_line_names: &mut Vec<String>,
    sizes: &mut Vec<css::GridTrackSize>,
    line_names: &mut Vec<Vec<String>>,
) -> Option<()> {
    for component in components {
        match component {
            css::GridTrackListComponent::Track(names, size) => {
                current_line_names.extend(names.iter().cloned());
                line_names.push(std::mem::take(current_line_names));
                sizes.push(size.clone());
            }
            css::GridTrackListComponent::Repeat(names, repeat) => {
                let count = intrinsic_grid_repeat_count(repeat.count);
                current_line_names.extend(names.iter().cloned());
                for _ in 0..count {
                    collect_expanded_grid_tracks(
                        &repeat.tracks,
                        current_line_names,
                        sizes,
                        line_names,
                    )?;
                    current_line_names.extend(repeat.trailing_names.iter().cloned());
                }
            }
        }
    }
    Some(())
}

fn intrinsic_explicit_grid_track_count(tracks: &css::GridTrackList) -> Option<usize> {
    let css::GridTrackList::Tracks {
        components,
        trailing_names,
    } = tracks
    else {
        return Some(1);
    };
    if components.is_empty() {
        return Some(1);
    }
    expanded_grid_tracks(components, trailing_names, &css::GridTemplateAreas::None)
        .map(|expanded| expanded.sizes.len().max(1))
}

fn grid_template_area_column_count(areas: &css::GridTemplateAreas) -> usize {
    let css::GridTemplateAreas::Areas(rows) = areas else {
        return 0;
    };
    rows.iter().map(|row| row.cells.len()).max().unwrap_or(0)
}

fn grid_template_area_row_count(areas: &css::GridTemplateAreas) -> usize {
    let css::GridTemplateAreas::Areas(rows) = areas else {
        return 0;
    };
    rows.len()
}

struct GridTrackContributionInputs<'a, 'grid> {
    track_count: usize,
    explicit_column_count: usize,
    first_line_index: i32,
    line_names: &'a [Vec<String>],
    auto_flow: css::GridAutoFlow,
    explicit_row_count: usize,
    row_line_names: &'a [Vec<String>],
    children: &'a [GridChild<'grid>],
    estimates: &'a [GridItemEstimate],
    gap_width: f32,
}

fn grid_track_intrinsic_contributions(
    inputs: GridTrackContributionInputs<'_, '_>,
) -> Vec<(f32, f32)> {
    let mut contributions = vec![(0.0_f32, 0.0_f32); inputs.track_count];
    let mut complex = (0.0_f32, 0.0_f32);
    let mut placer = SimpleGridColumnPlacer::new(
        inputs.track_count,
        inputs.explicit_row_count,
        inputs.auto_flow,
    );

    for (child, estimate) in inputs.children.iter().zip(inputs.estimates) {
        let contribution = (estimate.min_width.points(), estimate.content_width.points());
        let range = match simple_grid_child_column_placement(
            &child.style,
            inputs.track_count,
            inputs.explicit_column_count,
            inputs.first_line_index,
            inputs.line_names,
            inputs.explicit_row_count,
            inputs.row_line_names,
        ) {
            Some(SimpleGridColumnPlacement::Definite { columns, rows }) => {
                placer.place_definite(columns, rows)
            }
            Some(SimpleGridColumnPlacement::Auto { span, rows }) => placer.place_auto(span, rows),
            None => None,
        };
        if let Some(range) = range {
            distribute_grid_item_contribution(
                &mut contributions,
                range,
                contribution,
                inputs.gap_width,
            );
        } else {
            complex.0 = complex.0.max(contribution.0);
            complex.1 = complex.1.max(contribution.1);
        }
    }

    if complex.0 > 0.0 || complex.1 > 0.0 {
        for contribution in &mut contributions {
            contribution.0 = contribution.0.max(complex.0);
            contribution.1 = contribution.1.max(complex.1);
        }
    }

    contributions
}

fn grid_item_intrinsic_contribution(
    estimates: impl Iterator<Item = GridItemEstimate>,
) -> (f32, f32) {
    estimates.fold((0.0_f32, 0.0_f32), |(min, max), estimate| {
        (
            min.max(estimate.min_width.points()),
            max.max(estimate.content_width.points()),
        )
    })
}

fn grid_child_column_is_auto(style: &ComputedStyle) -> bool {
    matches!(style.grid_column_start, css::GridPlacement::Auto)
        && matches!(style.grid_column_end, css::GridPlacement::Auto)
}

enum SimpleGridColumnPlacement {
    Definite {
        columns: std::ops::Range<usize>,
        rows: Option<std::ops::Range<usize>>,
    },
    Auto {
        span: usize,
        rows: Option<std::ops::Range<usize>>,
    },
}

fn simple_grid_child_column_placement(
    style: &ComputedStyle,
    track_count: usize,
    explicit_column_count: usize,
    first_line_index: i32,
    line_names: &[Vec<String>],
    explicit_row_count: usize,
    row_line_names: &[Vec<String>],
) -> Option<SimpleGridColumnPlacement> {
    if track_count == 0 {
        return None;
    }
    let rows = simple_grid_child_row_constraint(style, explicit_row_count, row_line_names)?;
    if let Some(span) = simple_grid_child_auto_column_span(style) {
        return Some(SimpleGridColumnPlacement::Auto { span, rows });
    }
    if let Some(range) = simple_grid_child_forward_column_range(
        style,
        track_count,
        explicit_column_count,
        first_line_index,
        line_names,
    ) {
        return Some(SimpleGridColumnPlacement::Definite {
            columns: range,
            rows,
        });
    }
    simple_grid_child_backward_column_range(
        style,
        track_count,
        explicit_column_count,
        first_line_index,
        line_names,
    )
    .map(|range| SimpleGridColumnPlacement::Definite {
        columns: range,
        rows,
    })
}

fn simple_grid_child_auto_column_span(style: &ComputedStyle) -> Option<usize> {
    if grid_child_column_is_auto(style) {
        return Some(1);
    }
    if matches!(style.grid_column_end, css::GridPlacement::Auto)
        && let css::GridPlacement::Span(span) = &style.grid_column_start
        && span.name().is_none()
    {
        return span.count().map(usize::from).filter(|span| *span > 0);
    }
    if matches!(style.grid_column_start, css::GridPlacement::Auto)
        && let css::GridPlacement::Span(span) = &style.grid_column_end
        && span.name().is_none()
    {
        return span.count().map(usize::from).filter(|span| *span > 0);
    }
    None
}

/// Resolve simple definite row placement constraints for intrinsic placement.
///
/// CSS Grid auto-placement honors definite row positions before assigning an
/// auto column. This estimator supports positive numeric row lines within the
/// explicit row grid so column intrinsic sizing can count same-row column-flow
/// items correctly:
/// <https://www.w3.org/TR/css-grid-1/#auto-placement-algo>.
fn simple_grid_child_row_constraint(
    style: &ComputedStyle,
    explicit_row_count: usize,
    row_line_names: &[Vec<String>],
) -> Option<Option<std::ops::Range<usize>>> {
    if matches!(style.grid_row_start, css::GridPlacement::Auto)
        && matches!(style.grid_row_end, css::GridPlacement::Auto)
    {
        return Some(None);
    }
    if let Some(range) =
        simple_grid_child_forward_row_range(style, explicit_row_count, row_line_names)
    {
        return Some(Some(range));
    }
    simple_grid_child_backward_row_range(style, explicit_row_count, row_line_names).map(Some)
}

fn simple_grid_child_forward_row_range(
    style: &ComputedStyle,
    explicit_row_count: usize,
    row_line_names: &[Vec<String>],
) -> Option<std::ops::Range<usize>> {
    let start = grid_line_index(&style.grid_row_start, row_line_names)?;
    let row_index = usize::try_from(start - 1).ok()?;
    if row_index >= explicit_row_count {
        return None;
    }
    let span = simple_grid_child_column_span_after(
        &style.grid_row_end,
        start,
        row_line_names,
        explicit_row_count,
        1,
    )?;
    let end = row_index.checked_add(span)?;
    (end <= explicit_row_count).then_some(row_index..end)
}

fn simple_grid_child_backward_row_range(
    style: &ComputedStyle,
    explicit_row_count: usize,
    row_line_names: &[Vec<String>],
) -> Option<std::ops::Range<usize>> {
    let end = grid_line_index(&style.grid_row_end, row_line_names)?;
    let end_row_index = usize::try_from(end - 1).ok()?;
    if end_row_index > explicit_row_count {
        return None;
    }
    let span = simple_grid_child_column_span_before(
        &style.grid_row_start,
        end,
        row_line_names,
        explicit_row_count,
        1,
    )?;
    let start = end_row_index.checked_sub(span)?;
    (start < end_row_index).then_some(start..end_row_index)
}

struct SimpleGridColumnPlacer {
    track_count: usize,
    explicit_row_count: usize,
    auto_flow: css::GridAutoFlow,
    rows: Vec<Vec<bool>>,
    cursor_row: usize,
    cursor_column: usize,
}

impl SimpleGridColumnPlacer {
    fn new(track_count: usize, explicit_row_count: usize, auto_flow: css::GridAutoFlow) -> Self {
        Self {
            track_count,
            explicit_row_count: explicit_row_count.max(1),
            auto_flow,
            rows: vec![vec![false; track_count]],
            cursor_row: 0,
            cursor_column: 0,
        }
    }

    fn place_definite(
        &mut self,
        columns: std::ops::Range<usize>,
        rows: Option<std::ops::Range<usize>>,
    ) -> Option<std::ops::Range<usize>> {
        if columns.is_empty() || columns.end > self.track_count {
            return None;
        }
        if let Some(rows) = rows {
            self.mark_rows(rows, columns.clone());
            return Some(columns);
        }
        let mut row = 0;
        loop {
            if self.can_place(row, columns.clone()) {
                self.mark(row, columns.clone());
                return Some(columns);
            }
            row += 1;
        }
    }

    /// Place a simple auto-positioned grid item in sparse or dense order.
    ///
    /// CSS Grid's auto-placement cursor advances through columns or rows
    /// depending on `grid-auto-flow`; this intrinsic estimator only tracks the
    /// explicit-grid occupancy needed to assign column track contributions:
    /// <https://www.w3.org/TR/css-grid-1/#auto-placement-algo>.
    fn place_auto(
        &mut self,
        span: usize,
        rows: Option<std::ops::Range<usize>>,
    ) -> Option<std::ops::Range<usize>> {
        if span == 0 || span > self.track_count {
            return None;
        }
        match self.auto_flow {
            css::GridAutoFlow::Row => self.place_auto_row_sparse(span, rows),
            css::GridAutoFlow::RowDense => self.place_auto_row_dense(span, rows),
            css::GridAutoFlow::Column => self.place_auto_column_sparse(span, rows),
            css::GridAutoFlow::ColumnDense => self.place_auto_column_dense(span, rows),
        }
    }

    fn place_auto_row_sparse(
        &mut self,
        span: usize,
        rows: Option<std::ops::Range<usize>>,
    ) -> Option<std::ops::Range<usize>> {
        if let Some(rows) = rows {
            while self.cursor_column + span <= self.track_count {
                let range = self.cursor_column..self.cursor_column + span;
                if self.can_place_rows(rows.clone(), range.clone()) {
                    self.mark_rows(rows, range.clone());
                    self.cursor_column = range.end;
                    return Some(range);
                }
                self.cursor_column += 1;
            }
            return None;
        }
        loop {
            if self.cursor_column + span > self.track_count {
                self.cursor_row += 1;
                self.cursor_column = 0;
                continue;
            }
            let range = self.cursor_column..self.cursor_column + span;
            if self.can_place(self.cursor_row, range.clone()) {
                self.mark(self.cursor_row, range.clone());
                self.cursor_column = range.end;
                return Some(range);
            }
            self.cursor_column += 1;
        }
    }

    fn place_auto_row_dense(
        &mut self,
        span: usize,
        rows: Option<std::ops::Range<usize>>,
    ) -> Option<std::ops::Range<usize>> {
        if let Some(rows) = rows {
            for column in 0..=self.track_count - span {
                let range = column..column + span;
                if self.can_place_rows(rows.clone(), range.clone()) {
                    self.mark_rows(rows, range.clone());
                    return Some(range);
                }
            }
            return None;
        }
        for row in 0.. {
            for column in 0..=self.track_count - span {
                let range = column..column + span;
                if self.can_place(row, range.clone()) {
                    self.mark(row, range.clone());
                    return Some(range);
                }
            }
        }
        None
    }

    fn place_auto_column_sparse(
        &mut self,
        span: usize,
        rows: Option<std::ops::Range<usize>>,
    ) -> Option<std::ops::Range<usize>> {
        if let Some(rows) = rows {
            loop {
                if self.cursor_column + span > self.track_count {
                    return None;
                }
                let range = self.cursor_column..self.cursor_column + span;
                if self.can_place_rows(rows.clone(), range.clone()) {
                    self.mark_rows(rows, range.clone());
                    self.cursor_column = range.end;
                    return Some(range);
                }
                self.cursor_column += 1;
            }
        }
        loop {
            if self.cursor_column + span > self.track_count {
                return None;
            }
            if self.cursor_row >= self.explicit_row_count {
                self.cursor_column += 1;
                self.cursor_row = 0;
                continue;
            }
            let range = self.cursor_column..self.cursor_column + span;
            if self.can_place(self.cursor_row, range.clone()) {
                self.mark(self.cursor_row, range.clone());
                self.cursor_row += 1;
                return Some(range);
            }
            self.cursor_row += 1;
        }
    }

    fn place_auto_column_dense(
        &mut self,
        span: usize,
        rows: Option<std::ops::Range<usize>>,
    ) -> Option<std::ops::Range<usize>> {
        if let Some(rows) = rows {
            for column in 0..=self.track_count - span {
                let range = column..column + span;
                if self.can_place_rows(rows.clone(), range.clone()) {
                    self.mark_rows(rows, range.clone());
                    return Some(range);
                }
            }
            return None;
        }
        for column in 0..=self.track_count - span {
            for row in 0..self.explicit_row_count {
                let range = column..column + span;
                if self.can_place(row, range.clone()) {
                    self.mark(row, range.clone());
                    return Some(range);
                }
            }
        }
        None
    }

    fn can_place(&mut self, row: usize, range: std::ops::Range<usize>) -> bool {
        self.ensure_row(row);
        range.end <= self.track_count && self.rows[row][range].iter().all(|occupied| !occupied)
    }

    fn can_place_rows(
        &mut self,
        rows: std::ops::Range<usize>,
        columns: std::ops::Range<usize>,
    ) -> bool {
        rows.into_iter()
            .all(|row| self.can_place(row, columns.clone()))
    }

    fn mark(&mut self, row: usize, range: std::ops::Range<usize>) {
        self.ensure_row(row);
        for occupied in &mut self.rows[row][range] {
            *occupied = true;
        }
    }

    fn mark_rows(&mut self, rows: std::ops::Range<usize>, columns: std::ops::Range<usize>) {
        for row in rows {
            self.mark(row, columns.clone());
        }
    }

    fn ensure_row(&mut self, row: usize) {
        while row >= self.rows.len() {
            self.rows.push(vec![false; self.track_count]);
        }
    }
}

fn simple_grid_child_forward_column_range(
    style: &ComputedStyle,
    track_count: usize,
    explicit_column_count: usize,
    first_line_index: i32,
    line_names: &[Vec<String>],
) -> Option<std::ops::Range<usize>> {
    let range = simple_grid_child_forward_column_line_range(
        style,
        explicit_column_count,
        first_line_index,
        line_names,
    )?;
    line_range_to_track_range(range, first_line_index, track_count)
}

fn simple_grid_child_forward_column_line_range(
    style: &ComputedStyle,
    explicit_column_count: usize,
    first_line_index: i32,
    line_names: &[Vec<String>],
) -> Option<std::ops::Range<i32>> {
    let start = intrinsic_column_line_index(
        &style.grid_column_start,
        line_names,
        explicit_column_count,
        first_line_index,
    )?;
    let span = simple_grid_child_column_span_after(
        &style.grid_column_end,
        start,
        line_names,
        explicit_column_count,
        first_line_index,
    )?;
    let end = start.checked_add(i32::try_from(span).ok()?)?;
    (start < end).then_some(start..end)
}

fn simple_grid_child_backward_column_range(
    style: &ComputedStyle,
    track_count: usize,
    explicit_column_count: usize,
    first_line_index: i32,
    line_names: &[Vec<String>],
) -> Option<std::ops::Range<usize>> {
    let range = simple_grid_child_backward_column_line_range(
        style,
        explicit_column_count,
        first_line_index,
        line_names,
    )?;
    line_range_to_track_range(range, first_line_index, track_count)
}

fn simple_grid_child_column_line_range(
    style: &ComputedStyle,
    explicit_column_count: usize,
    line_names: &[Vec<String>],
) -> Option<std::ops::Range<i32>> {
    simple_grid_child_forward_column_line_range(style, explicit_column_count, 1, line_names)
        .or_else(|| {
            simple_grid_child_backward_column_line_range(
                style,
                explicit_column_count,
                1,
                line_names,
            )
        })
}

fn simple_grid_child_backward_column_line_range(
    style: &ComputedStyle,
    explicit_column_count: usize,
    first_line_index: i32,
    line_names: &[Vec<String>],
) -> Option<std::ops::Range<i32>> {
    let end = intrinsic_column_line_index(
        &style.grid_column_end,
        line_names,
        explicit_column_count,
        first_line_index,
    )?;
    let span = simple_grid_child_column_span_before(
        &style.grid_column_start,
        end,
        line_names,
        explicit_column_count,
        first_line_index,
    )?;
    let start = end.checked_sub(i32::try_from(span).ok()?)?;
    (start < end).then_some(start..end)
}

fn line_range_to_track_range(
    range: std::ops::Range<i32>,
    first_line_index: i32,
    track_count: usize,
) -> Option<std::ops::Range<usize>> {
    let start = usize::try_from(range.start.checked_sub(first_line_index)?).ok()?;
    let end = usize::try_from(range.end.checked_sub(first_line_index)?).ok()?;
    (start < end && end <= track_count).then_some(start..end)
}

fn simple_grid_child_column_span_after(
    end: &css::GridPlacement,
    start: i32,
    line_names: &[Vec<String>],
    explicit_column_count: usize,
    first_line_index: i32,
) -> Option<usize> {
    match end {
        css::GridPlacement::Auto => Some(1),
        css::GridPlacement::Span(span) if span.name().is_none() => {
            span.count().map(usize::from).filter(|span| *span > 0)
        }
        css::GridPlacement::Span(span) => simple_grid_named_column_span_after(
            span,
            start,
            line_names,
            explicit_column_count,
            first_line_index,
        ),
        css::GridPlacement::Line(_) => {
            let end = intrinsic_column_line_index(
                end,
                line_names,
                explicit_column_count,
                first_line_index,
            )?;
            (end > start)
                .then_some(end - start)
                .and_then(|span| usize::try_from(span).ok())
        }
    }
}

fn simple_grid_child_column_span_before(
    start: &css::GridPlacement,
    end: i32,
    line_names: &[Vec<String>],
    explicit_column_count: usize,
    first_line_index: i32,
) -> Option<usize> {
    match start {
        css::GridPlacement::Auto => Some(1),
        css::GridPlacement::Span(span) if span.name().is_none() => {
            span.count().map(usize::from).filter(|span| *span > 0)
        }
        css::GridPlacement::Span(span) => simple_grid_named_column_span_before(
            span,
            end,
            line_names,
            explicit_column_count,
            first_line_index,
        ),
        css::GridPlacement::Line(_) => None,
    }
}

/// Resolve a simple forward named column span from a definite start line.
///
/// CSS Grid resolves a named span from the opposite definite edge by counting
/// matching named lines in the span direction:
/// <https://www.w3.org/TR/css-grid-1/#grid-placement-span-int>.
fn simple_grid_named_column_span_after(
    span: &css::GridSpanPlacement,
    start: i32,
    line_names: &[Vec<String>],
    explicit_track_count: usize,
    first_line_index: i32,
) -> Option<usize> {
    let name = span.name()?;
    let target = span.count().unwrap_or(1);
    if target == 0 {
        return None;
    }
    let mut matches_seen = 0_u16;
    let explicit_line_count = explicit_track_count.checked_add(1)?;
    for (line_index, names) in line_names
        .iter()
        .enumerate()
        .skip(explicit_line_vector_start(first_line_index)?)
        .take(explicit_line_count)
    {
        let css_line = first_line_index.checked_add(i32::try_from(line_index).ok()?)?;
        if css_line <= start {
            continue;
        }
        if names.iter().any(|line_name| line_name == name) {
            matches_seen += 1;
            if matches_seen == target {
                return usize::try_from(css_line.checked_sub(start)?).ok();
            }
        }
    }
    let explicit_line_count = i32::try_from(explicit_line_count).ok()?;
    if start < explicit_line_count {
        let missing = usize::from(target - matches_seen);
        i32::try_from(missing)
            .ok()
            .and_then(|missing| explicit_line_count.checked_add(missing))
            .and_then(|line| line.checked_sub(start))
            .and_then(|span| usize::try_from(span).ok())
    } else {
        Some(usize::from(target - matches_seen))
    }
}

/// Resolve a simple backward named column span from a definite end line.
///
/// CSS Grid resolves a named span from the opposite definite edge by counting
/// matching named lines in the span direction:
/// <https://www.w3.org/TR/css-grid-1/#grid-placement-span-int>.
fn simple_grid_named_column_span_before(
    span: &css::GridSpanPlacement,
    end: i32,
    line_names: &[Vec<String>],
    explicit_track_count: usize,
    first_line_index: i32,
) -> Option<usize> {
    let name = span.name()?;
    let target = span.count().unwrap_or(1);
    if target == 0 {
        return None;
    }
    let mut matches_seen = 0_u16;
    let explicit_line_count = explicit_track_count.checked_add(1)?;
    for (line_index, names) in line_names
        .iter()
        .enumerate()
        .skip(explicit_line_vector_start(first_line_index)?)
        .take(explicit_line_count)
        .rev()
    {
        let css_line = first_line_index.checked_add(i32::try_from(line_index).ok()?)?;
        if css_line >= end {
            continue;
        }
        if names.iter().any(|line_name| line_name == name) {
            matches_seen += 1;
            if matches_seen == target {
                return usize::try_from(end.checked_sub(css_line)?).ok();
            }
        }
    }
    let missing = usize::from(target - matches_seen);
    let start_line = 1_i32.checked_sub(i32::try_from(missing).ok()?)?;
    end.checked_sub(start_line)
        .and_then(|span| usize::try_from(span).ok())
}

fn intrinsic_column_line_index(
    placement: &css::GridPlacement,
    line_names: &[Vec<String>],
    explicit_track_count: usize,
    first_line_index: i32,
) -> Option<i32> {
    let css::GridPlacement::Line(line) = placement else {
        return None;
    };
    let explicit_line_count = i32::try_from(explicit_track_count).ok()?.checked_add(1)?;
    if line.name().is_none() {
        let index = line.index()?;
        if index > 0 {
            return Some(index);
        }
        return (index < 0).then(|| explicit_line_count + index + 1);
    }
    let name = line.name()?;
    let occurrence = line.index().unwrap_or(1);
    named_intrinsic_column_line_index(
        line_names,
        name,
        occurrence,
        explicit_track_count,
        first_line_index,
    )
}

fn named_intrinsic_column_line_index(
    line_names: &[Vec<String>],
    name: &str,
    occurrence: i32,
    explicit_track_count: usize,
    first_line_index: i32,
) -> Option<i32> {
    if occurrence == 0 {
        return None;
    }
    let target = occurrence.unsigned_abs();
    let explicit_line_count = explicit_track_count.checked_add(1)?;
    let explicit_start = explicit_line_vector_start(first_line_index)?;
    let mut matches_seen = 0_u32;
    let explicit_lines = line_names
        .iter()
        .enumerate()
        .skip(explicit_start)
        .take(explicit_line_count);
    if occurrence > 0 {
        for (line_index, names) in explicit_lines {
            if names.iter().any(|line_name| line_name == name) {
                matches_seen += 1;
                if matches_seen == target {
                    return first_line_index.checked_add(i32::try_from(line_index).ok()?);
                }
            }
        }
        let missing = i32::try_from(target.checked_sub(matches_seen)?).ok()?;
        return i32::try_from(explicit_line_count)
            .ok()?
            .checked_add(missing);
    }
    for (line_index, names) in explicit_lines.rev() {
        if names.iter().any(|line_name| line_name == name) {
            matches_seen += 1;
            if matches_seen == target {
                return first_line_index.checked_add(i32::try_from(line_index).ok()?);
            }
        }
    }
    let missing = i32::try_from(target.checked_sub(matches_seen)?).ok()?;
    1_i32.checked_sub(missing)
}

fn explicit_line_vector_start(first_line_index: i32) -> Option<usize> {
    usize::try_from(1_i32.checked_sub(first_line_index)?).ok()
}

fn distribute_grid_item_contribution(
    contributions: &mut [(f32, f32)],
    range: std::ops::Range<usize>,
    contribution: (f32, f32),
    gap_width: f32,
) {
    let span = range.len();
    if span == 0 {
        return;
    }
    let crossed_gaps = span.saturating_sub(1) as f32;
    let min = (contribution.0 - gap_width * crossed_gaps).max(0.0) / span as f32;
    let max = (contribution.1 - gap_width * crossed_gaps).max(0.0) / span as f32;
    for track in &mut contributions[range] {
        track.0 = track.0.max(min);
        track.1 = track.1.max(max);
    }
}

/// Return the grid gap contribution for intrinsic container width estimates.
///
/// CSS Box Alignment resolves cyclic percentage gaps against zero for
/// intrinsic size contributions, while preserving non-percentage length
/// components:
/// <https://www.w3.org/TR/css-align-3/#gaps>.
fn intrinsic_grid_gap_size(gap: css::ComputedGap) -> LayoutLength {
    match gap {
        css::ComputedGap::Normal => layout_pt(0.0),
        css::ComputedGap::LengthPercentage(value) => value.length_max_zero(),
    }
}

/// Resolve repeat count for grid container intrinsic width estimates.
///
/// CSS Grid's auto-repeat expansion uses the available definite container size
/// when there is one; otherwise `auto-fill`/`auto-fit` repeat once. Container
/// intrinsic sizing is an indefinite inline-size query in this estimator, so
/// auto-repeat contributes one copy of its fixed-size repeated track list:
/// <https://www.w3.org/TR/css-grid-1/#auto-repeat>.
fn intrinsic_grid_repeat_count(count: css::GridRepeatCount) -> u16 {
    match count {
        css::GridRepeatCount::Number(count) => count,
        css::GridRepeatCount::AutoFill | css::GridRepeatCount::AutoFit => 1,
    }
}

fn grid_track_intrinsic_width(
    size: css::GridTrackSize,
    item_min: f32,
    item_max: f32,
) -> (f32, f32) {
    let min = grid_min_track_intrinsic_width(size.min, item_min, item_max);
    let max = grid_max_track_intrinsic_width(size.max, item_min, item_max);
    (min.min(max).max(0.0), max.max(min).max(0.0))
}

fn grid_min_track_intrinsic_width(
    breadth: css::GridMinTrackBreadth,
    item_min: f32,
    item_max: f32,
) -> f32 {
    match breadth {
        css::GridMinTrackBreadth::Auto | css::GridMinTrackBreadth::MinContent => item_min,
        css::GridMinTrackBreadth::MaxContent => item_max,
        css::GridMinTrackBreadth::LengthPercentage(value) => {
            intrinsic_grid_min_track_breadth_length(value, layout_pt(item_min)).points()
        }
    }
}

fn grid_max_track_intrinsic_width(
    breadth: css::GridMaxTrackBreadth,
    item_min: f32,
    item_max: f32,
) -> f32 {
    match breadth {
        css::GridMaxTrackBreadth::Auto
        | css::GridMaxTrackBreadth::MaxContent
        | css::GridMaxTrackBreadth::Flex(_) => item_max,
        css::GridMaxTrackBreadth::MinContent => item_min,
        css::GridMaxTrackBreadth::LengthPercentage(value) => {
            intrinsic_grid_max_track_breadth_length(value, layout_pt(item_max)).points()
        }
        css::GridMaxTrackBreadth::FitContent(value) => {
            let limit = intrinsic_grid_fit_content_limit(value, layout_pt(item_max)).points();
            item_max.min(item_min.max(limit)).max(0.0)
        }
    }
}

/// Return a min track breadth for intrinsic grid container sizing.
///
/// CSS Grid treats percentage track sizes as `auto` for intrinsic size
/// calculations when the grid container's size depends on its tracks:
/// <https://www.w3.org/TR/css-grid-1/#valdef-grid-template-columns-percentage>.
fn intrinsic_grid_min_track_breadth_length(
    value: css::ComputedLengthPercentage,
    item_min: LayoutLength,
) -> LayoutLength {
    if value.contains_percentage() {
        item_min
    } else {
        value.length_max_zero()
    }
}

/// Return a max track breadth for intrinsic grid container sizing.
///
/// CSS Grid treats percentage track sizes as `auto` for intrinsic size
/// calculations when the grid container's size depends on its tracks:
/// <https://www.w3.org/TR/css-grid-1/#valdef-grid-template-columns-percentage>.
fn intrinsic_grid_max_track_breadth_length(
    value: css::ComputedLengthPercentage,
    item_max: LayoutLength,
) -> LayoutLength {
    if value.contains_percentage() {
        item_max
    } else {
        value.length_max_zero()
    }
}

/// Return the `fit-content()` limit for intrinsic grid container sizing.
///
/// CSS Grid defines `fit-content()` as `minmax(auto, max-content)` capped by
/// the argument, and percentage track sizes behave as `auto` during intrinsic
/// container sizing:
/// <https://www.w3.org/TR/css-grid-1/#valdef-grid-template-columns-fit-content>.
fn intrinsic_grid_fit_content_limit(
    value: css::ComputedLengthPercentage,
    item_max: LayoutLength,
) -> LayoutLength {
    if value.contains_percentage() {
        item_max
    } else {
        value.length_max_zero()
    }
}

fn grid_item_estimate_from_intrinsic(
    style: &ComputedStyle,
    available_width: f32,
    inline_basis: GridPercentageBasis,
    block_basis: GridPercentageBasis,
    min_content: f32,
    max_content: f32,
    content_height: f32,
) -> GridItemEstimate {
    let max_content = max_content.max(min_content).max(0.0);
    let min_content = min_content.max(0.0);
    let content_height = content_height.max(0.0);
    let intrinsic_metrics = intrinsic_box_metrics(style);
    let horizontal_non_content = intrinsic_metrics.horizontal_non_content_length();
    let specified_width = crate::layout::intrinsic::intrinsic_content_box_width_keyword(
        style.box_values.width.clone(),
        content_box_pt(min_content),
        content_box_pt(max_content),
        layout_pt(available_width),
        horizontal_non_content,
    )
    .map(SemanticLengthExt::points)
    .or_else(|| {
        used_length_percentage_or_auto_with_basis(style.box_values.width.clone(), inline_basis)
            .map(SemanticLengthExt::points)
    });
    let content_width = specified_width.unwrap_or(max_content);
    // A definite or intrinsic preferred inline size constrains the item's
    // min-content contribution. For example, `width: max-content` makes the
    // item's min-content contribution its max-content size, rather than the
    // width of its narrowest word. An unresolved percentage remains automatic
    // for this intrinsic probe.
    // <https://www.w3.org/TR/css-grid-1/#min-size-auto>
    // <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes>
    let min_width_contribution = match style.box_values.width.clone() {
        css::ComputedLengthPercentageOrAuto::Auto
        | css::ComputedLengthPercentageOrAuto::Stretch => min_content,
        css::ComputedLengthPercentageOrAuto::LengthPercentage(_) => {
            specified_width.map_or(min_content, |_| content_width)
        }
        css::ComputedLengthPercentageOrAuto::MinContent
        | css::ComputedLengthPercentageOrAuto::MaxContent
        | css::ComputedLengthPercentageOrAuto::FitContent(_)
        | css::ComputedLengthPercentageOrAuto::CalcSize(_) => content_width,
    };
    let content_height = used_length_percentage_or_auto_with_basis(
        style.box_values.height.value().clone(),
        block_basis,
    )
    .map(|height| height.points())
    .unwrap_or(content_height);
    GridItemEstimate {
        metrics: IntrinsicItemMetrics {
            width: constrain_content_width(
                style,
                content_box_pt(content_width),
                PercentageBasis::definite(layout_pt(available_width)),
            ),
            height: content_box_pt(constrain_grid_intrinsic_height(
                style,
                content_height,
                block_basis,
            )),
            min_width: constrain_content_width(
                style,
                content_box_pt(min_width_contribution),
                PercentageBasis::definite(layout_pt(available_width)),
            ),
            min_height: content_box_pt(constrain_grid_intrinsic_height(
                style,
                content_height,
                block_basis,
            )),
            // A preferred size constrains both intrinsic contributions.  The
            // raw max-content width remains relevant only while `width` is
            // automatic (or percentage-cyclic) in this intrinsic pass.
            // <https://drafts.csswg.org/css-sizing-3/#intrinsic-contribution>
            content_width: content_box_pt(content_width),
            content_height: content_box_pt(content_height),
            preferred_aspect_ratio: style.aspect_ratio.preferred_ratio_for_non_replaced(false),
            first_baseline: None,
            last_baseline: None,
        },
        swaps_physical_axes: WritingModeAxes::new(style.writing_mode, style.direction)
            .swaps_physical_axes(),
        replaced_used_size: None,
    }
}

/// Store horizontal text baselines for grid item baseline self-alignment.
///
/// CSS Grid participates in CSS Box Alignment baseline sharing. For the
/// same-page path covered here, inline text baselines are measured from the
/// grid item's border-box block-start edge, matching the baseline coordinate
/// used when the item is replayed:
/// <https://www.w3.org/TR/css-grid-1/#grid-baselines> and
/// <https://www.w3.org/TR/css-align-3/#baseline-align-self>.
fn set_grid_item_text_baselines(
    estimate: &mut GridItemEstimate,
    style: &ComputedStyle,
    first_line_baseline: f32,
    last_line_baseline: f32,
    line_count: usize,
) {
    if line_count == 0 || style.writing_mode != WritingMode::HorizontalTb {
        return;
    }
    let borders = used_border_widths(style);
    let baseline_edge = borders.top + style.padding.top;
    estimate.first_baseline = Some(baseline_edge + first_line_baseline);
    estimate.last_baseline = Some(baseline_edge + last_line_baseline);
}

/// Apply grid item intrinsic min/max height constraints with an optional basis.
///
/// CSS Sizing resolves percentages only against definite containing-block
/// sizes. Grid row intrinsic sizing can query an item while the grid
/// container's block size is indefinite, so percentage heights and percentage
/// min/max-height constraints must behave as unresolved rather than using the
/// grid inline size as a fallback:
/// <https://www.w3.org/TR/css-sizing-3/#percentage-sizing> and
/// <https://www.w3.org/TR/css-grid-1/#algo-overview>.
fn constrain_grid_intrinsic_height(
    style: &ComputedStyle,
    mut value: f32,
    percentage_basis: GridPercentageBasis,
) -> f32 {
    if let Some(min) = used_length_percentage_or_auto_with_basis(
        style.box_values.min_height.clone(),
        percentage_basis,
    )
    .map(|height| height.points().max(0.0))
    {
        value = value.max(min);
    }
    if let Some(max) = used_length_percentage_or_auto_with_basis(
        style.box_values.max_height.clone(),
        percentage_basis,
    )
    .map(|height| height.points().max(0.0))
    {
        value = value.min(max);
    }
    value
}

/// Measures a leaf grid item for Taffy's Grid track-sizing algorithm.
///
/// CSS Grid track sizing asks grid items for intrinsic size contributions in
/// the inline and block axes. The measurement result is a content-box size;
/// Taffy applies padding and borders around it:
/// <https://www.w3.org/TR/css-grid-1/#algo-overview>.
pub(super) fn measure_grid_item(
    known_dimensions: taffy_layout::Size<Option<f32>>,
    available_space: taffy_layout::Size<taffy_layout::AvailableSpace>,
    estimate: Option<&mut GridItemEstimate>,
) -> taffy_layout::Size<f32> {
    let estimate = estimate.cloned().unwrap_or(GridItemEstimate {
        metrics: IntrinsicItemMetrics::zero(),
        swaps_physical_axes: false,
        replaced_used_size: None,
    });
    let estimate = estimate.physical_measurements();
    measure_intrinsic_item_leaf(
        known_dimensions,
        estimate.preferred_aspect_ratio,
        taffy_layout::Size {
            width: grid_item_measured_size(
                available_space.width,
                estimate.width,
                estimate.min_width,
                estimate.content_width,
            ),
            height: grid_item_measured_size(
                available_space.height,
                estimate.height,
                estimate.min_height,
                estimate.content_height,
            ),
        },
    )
}

fn grid_item_measured_size(
    available_space: taffy_layout::AvailableSpace,
    preferred: ContentBoxLength,
    min_content: ContentBoxLength,
    max_content: ContentBoxLength,
) -> f32 {
    match available_space {
        taffy_layout::AvailableSpace::MinContent => min_content.points(),
        taffy_layout::AvailableSpace::MaxContent => max_content.points(),
        taffy_layout::AvailableSpace::Definite(_) => preferred.points(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intrinsic_gap_preserves_a_typed_fixed_component() {
        let value = css::ComputedLengthPercentage::from_affine(layout_pt(12.0), 0.5, true);

        let gap: LayoutLength = intrinsic_grid_gap_size(css::ComputedGap::LengthPercentage(value));

        // Intrinsic sizing resolves the cyclic percentage component to zero.
        assert_eq!(gap, layout_pt(12.0));
    }

    fn fixed_track(size: f32) -> css::GridTrackSize {
        css::GridTrackSize {
            min: css::GridMinTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(size),
            ),
            max: css::GridMaxTrackBreadth::LengthPercentage(
                css::ComputedLengthPercentage::from_points(size),
            ),
        }
    }

    fn anonymous_grid_child_with_style(style: ComputedStyle) -> GridChild<'static> {
        let source = FormattingContextChild {
            kind: FormattingContextChildKind::AnonymousContent {
                children: Vec::new(),
            },
            style: style.clone(),
        };
        let used_style = css::LayoutStyle::from_computed(&style).into_zoomed();
        GridUsedItem::from_source(source, used_style)
    }

    fn grid_line(index: i32) -> css::GridPlacement {
        css::GridPlacement::Line(css::GridLinePlacement::Number(
            std::num::NonZeroI32::new(index).unwrap(),
        ))
    }

    fn named_grid_line(name: &str) -> css::GridPlacement {
        css::GridPlacement::Line(css::GridLinePlacement::Named {
            name: name.to_string(),
            occurrence: None,
        })
    }

    fn row_lines(row_count: usize) -> Vec<Vec<String>> {
        vec![Vec::new(); row_count + 1]
    }

    #[test]
    fn grid_item_fixed_estimate_preserves_content_box_lengths() {
        let estimate = GridItemEstimate::fixed(24.0, 36.0);

        assert_eq!(estimate.width.points(), 24.0);
        assert_eq!(estimate.height.points(), 36.0);
        assert_eq!(estimate.min_width.points(), 24.0);
        assert_eq!(estimate.min_height.points(), 36.0);
        assert_eq!(estimate.content_width.points(), 24.0);
        assert_eq!(estimate.content_height.points(), 36.0);
    }

    #[test]
    fn inherited_subgrid_axes_are_removed_from_parent_sizing_requests() {
        let mut column_subgrid = ComputedStyle::initial();
        column_subgrid.grid_template_columns = css::GridTrackList::Subgrid {
            line_names: css::SubgridLineNameList::default(),
        };
        let column_request = GridItemMeasurementRequest::parent_track_sizing(&column_subgrid);
        assert!(!column_request.contributes_columns);
        assert!(column_request.contributes_rows);
        assert!(!column_request.export_baselines);
        let mut column_estimate = GridItemEstimate::fixed(24.0, 36.0);
        column_request.apply_to_estimate(&mut column_estimate);
        assert_eq!(column_estimate.width.points(), 0.0);
        assert_eq!(column_estimate.min_width.points(), 0.0);
        assert_eq!(column_estimate.content_width.points(), 0.0);
        assert_eq!(column_estimate.height.points(), 36.0);

        let mut row_subgrid = ComputedStyle::initial();
        row_subgrid.grid_template_rows = css::GridTrackList::Subgrid {
            line_names: css::SubgridLineNameList::default(),
        };
        let row_request = GridItemMeasurementRequest::parent_track_sizing(&row_subgrid);
        assert!(row_request.contributes_columns);
        assert!(!row_request.contributes_rows);
        let mut row_estimate = GridItemEstimate::fixed(24.0, 36.0);
        row_request.apply_to_estimate(&mut row_estimate);
        assert_eq!(row_estimate.width.points(), 24.0);
        assert_eq!(row_estimate.height.points(), 0.0);
        assert_eq!(row_estimate.min_height.points(), 0.0);
        assert_eq!(row_estimate.content_height.points(), 0.0);

        let mut two_axis_subgrid = column_subgrid;
        two_axis_subgrid.grid_template_rows = css::GridTrackList::Subgrid {
            line_names: css::SubgridLineNameList::default(),
        };
        let two_axis_request = GridItemMeasurementRequest::parent_track_sizing(&two_axis_subgrid);
        assert!(!two_axis_request.contributes_columns);
        assert!(!two_axis_request.contributes_rows);
        let mut two_axis_estimate = GridItemEstimate::fixed(24.0, 36.0);
        two_axis_request.apply_to_estimate(&mut two_axis_estimate);
        assert_eq!(two_axis_estimate.width.points(), 0.0);
        assert_eq!(two_axis_estimate.height.points(), 0.0);
    }

    #[test]
    fn complete_grid_measurement_retains_baseline_probe() {
        let request = GridItemMeasurementRequest::complete();
        assert!(request.export_baselines);
        assert!(!request.uses_zeroed_subgrid_contributions());
    }

    #[test]
    fn replaced_grid_item_sizes_are_projected_into_logical_axes() {
        let horizontal = ComputedStyle::initial();
        assert_eq!(
            grid_item_logical_sizes_from_physical(&horizontal, 200.0, 100.0),
            (200.0, 100.0)
        );

        let mut vertical = ComputedStyle::initial();
        vertical.writing_mode = WritingMode::VerticalLr;
        assert_eq!(
            grid_item_logical_sizes_from_physical(&vertical, 200.0, 100.0),
            (100.0, 200.0)
        );
    }

    #[test]
    fn replaced_grid_estimate_retains_intrinsic_used_size_across_axis_projection() {
        let mut estimate = GridItemEstimate::fixed(200.0, 100.0);
        estimate.replaced_used_size = Some(ReplacedGridItemUsedSize {
            width: PhysicalContentWidth::new(content_box_pt(200.0)),
            height: PhysicalContentHeight::new(content_box_pt(100.0)),
        });

        let projected = estimate.logical_block_contribution(0.0);
        assert_eq!(projected.replaced_used_size.unwrap().width.points(), 200.0);
        assert_eq!(projected.replaced_used_size.unwrap().height.points(), 100.0);
    }

    #[test]
    fn block_track_contribution_swaps_grid_item_intrinsic_axes() {
        let estimate = GridItemEstimate {
            metrics: IntrinsicItemMetrics::fixed(40.0, 60.0),
            swaps_physical_axes: true,
            replaced_used_size: None,
        };

        let contribution = estimate.logical_block_contribution(3.0);
        assert_eq!(contribution.min_width.points(), 63.0);
        assert_eq!(contribution.content_width.points(), 63.0);
        assert!(!contribution.swaps_physical_axes);
    }

    #[test]
    fn max_content_item_uses_its_max_content_width_for_min_content_measurement() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::MaxContent;

        let estimate = grid_item_estimate_from_intrinsic(
            &style,
            300.0,
            GridPercentageBasis::indefinite(),
            GridPercentageBasis::indefinite(),
            20.0,
            100.0,
            20.0,
        );

        assert_eq!(estimate.min_width.points(), 100.0);
        assert_eq!(estimate.content_width.points(), 100.0);
    }

    #[test]
    fn definite_preferred_width_caps_grid_item_max_content_measurement() {
        let mut style = ComputedStyle::initial();
        style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(100.0),
        );

        let estimate = grid_item_estimate_from_intrinsic(
            &style,
            300.0,
            GridPercentageBasis::indefinite(),
            GridPercentageBasis::indefinite(),
            20.0,
            200.0,
            20.0,
        );

        assert_eq!(estimate.min_width.points(), 100.0);
        assert_eq!(estimate.content_width.points(), 100.0);
    }

    #[test]
    fn implicit_column_count_does_not_synthesize_empty_column_flow_track() {
        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Column,
                2,
                0,
                &row_lines(0),
                &row_lines(2),
                &[]
            ),
            Some(0)
        );
    }

    #[test]
    fn row_auto_flow_reuses_one_implicit_column_across_explicit_rows() {
        let children = [
            anonymous_grid_child_with_style(ComputedStyle::initial()),
            anonymous_grid_child_with_style(ComputedStyle::initial()),
            anonymous_grid_child_with_style(ComputedStyle::initial()),
        ];

        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Row,
                3,
                0,
                &row_lines(0),
                &row_lines(3),
                &children
            ),
            Some(1)
        );
    }

    #[test]
    fn implicit_column_count_includes_positive_numeric_lines() {
        let mut second_column = ComputedStyle::initial();
        second_column.grid_column_start = grid_line(2);
        let mut third_column = ComputedStyle::initial();
        third_column.grid_column_start = grid_line(3);
        let children = [
            anonymous_grid_child_with_style(second_column),
            anonymous_grid_child_with_style(third_column),
        ];

        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Row,
                1,
                0,
                &row_lines(0),
                &row_lines(1),
                &children
            ),
            Some(3)
        );
    }

    #[test]
    fn implicit_column_count_includes_positive_named_implicit_lines() {
        let mut named_start = ComputedStyle::initial();
        named_start.grid_column_start = named_grid_line("slot");
        let mut named_end = ComputedStyle::initial();
        named_end.grid_column_start = css::GridPlacement::Auto;
        named_end.grid_column_end = named_grid_line("slot");
        let children = [
            anonymous_grid_child_with_style(named_start),
            anonymous_grid_child_with_style(named_end),
        ];

        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Row,
                1,
                1,
                &row_lines(1),
                &row_lines(1),
                &children
            ),
            Some(3)
        );
    }

    #[test]
    fn implicit_column_count_includes_forward_named_implicit_spans() {
        let mut named_span = ComputedStyle::initial();
        named_span.grid_column_start = grid_line(1);
        named_span.grid_column_end = css::GridPlacement::Span(css::GridSpanPlacement::Named {
            name: "slot".to_string(),
            count: None,
        });
        let children = [anonymous_grid_child_with_style(named_span)];

        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Row,
                1,
                1,
                &row_lines(1),
                &row_lines(1),
                &children
            ),
            Some(2)
        );
    }

    #[test]
    fn backward_named_implicit_span_resolves_startward() {
        let mut named_span = ComputedStyle::initial();
        named_span.grid_column_start = css::GridPlacement::Span(css::GridSpanPlacement::Named {
            name: "slot".to_string(),
            count: None,
        });
        named_span.grid_column_end = grid_line(3);

        let placement = simple_grid_child_column_placement(
            &named_span,
            3,
            1,
            0,
            &row_lines(3),
            1,
            &row_lines(1),
        );
        match placement {
            Some(SimpleGridColumnPlacement::Definite { columns, rows }) => {
                assert_eq!(columns, 0..3);
                assert!(rows.is_none());
            }
            _ => panic!("backward named implicit span should resolve to startward columns"),
        }
    }

    #[test]
    fn implicit_column_count_uses_template_area_explicit_columns() {
        let children = [
            anonymous_grid_child_with_style(ComputedStyle::initial()),
            anonymous_grid_child_with_style(ComputedStyle::initial()),
            anonymous_grid_child_with_style(ComputedStyle::initial()),
        ];

        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Row,
                1,
                2,
                &row_lines(2),
                &row_lines(1),
                &children
            ),
            Some(2)
        );
        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Column,
                2,
                2,
                &row_lines(2),
                &row_lines(2),
                &children
            ),
            Some(2)
        );
    }

    #[test]
    fn implicit_column_count_honors_definite_rows_in_column_auto_flow() {
        let mut first = ComputedStyle::initial();
        first.grid_row_start = grid_line(2);
        let mut second = ComputedStyle::initial();
        second.grid_row_start = grid_line(2);
        let mut third = ComputedStyle::initial();
        third.grid_row_start = grid_line(2);
        let children = [
            anonymous_grid_child_with_style(first),
            anonymous_grid_child_with_style(second),
            anonymous_grid_child_with_style(third),
        ];

        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Column,
                2,
                0,
                &row_lines(0),
                &row_lines(2),
                &children
            ),
            Some(3)
        );
        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Column,
                2,
                1,
                &row_lines(1),
                &row_lines(2),
                &children
            ),
            Some(3)
        );
    }

    #[test]
    fn implicit_column_count_honors_definite_rows_in_row_auto_flow() {
        let mut first = ComputedStyle::initial();
        first.grid_row_start = grid_line(2);
        let mut second = ComputedStyle::initial();
        second.grid_row_start = grid_line(2);
        let mut third = ComputedStyle::initial();
        third.grid_row_start = grid_line(2);
        let children = [
            anonymous_grid_child_with_style(first),
            anonymous_grid_child_with_style(second),
            anonymous_grid_child_with_style(third),
        ];

        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Row,
                2,
                0,
                &row_lines(0),
                &row_lines(2),
                &children
            ),
            Some(3)
        );
        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Row,
                2,
                1,
                &row_lines(1),
                &row_lines(2),
                &children
            ),
            Some(3)
        );
    }

    #[test]
    fn implicit_column_count_honors_generated_named_row_constraints() {
        let mut first = ComputedStyle::initial();
        first.grid_row_start = named_grid_line("slot-start");
        first.grid_row_end = named_grid_line("slot-end");
        let mut second = first.clone();
        let mut third = first.clone();
        second.grid_column_start = css::GridPlacement::Auto;
        third.grid_column_start = css::GridPlacement::Auto;
        let children = [
            anonymous_grid_child_with_style(first),
            anonymous_grid_child_with_style(second),
            anonymous_grid_child_with_style(third),
        ];
        let named_row_lines = vec![
            Vec::new(),
            vec!["slot-start".to_string()],
            vec!["slot-end".to_string()],
        ];

        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Row,
                2,
                1,
                &row_lines(1),
                &named_row_lines,
                &children
            ),
            Some(3)
        );
    }

    #[test]
    fn implicit_column_count_accepts_template_area_named_lines() {
        let mut named_area = ComputedStyle::initial();
        named_area.grid_column_start = named_grid_line("main-start");
        named_area.grid_column_end = named_grid_line("main-end");
        let children = [anonymous_grid_child_with_style(named_area)];

        assert_eq!(
            simple_implicit_column_count(
                css::GridAutoFlow::Row,
                1,
                2,
                &row_lines(2),
                &row_lines(1),
                &children
            ),
            Some(2)
        );
    }

    #[test]
    fn grid_item_measurement_extracts_typed_content_box_lengths() {
        let mut estimate = GridItemEstimate {
            metrics: IntrinsicItemMetrics {
                width: content_box_pt(40.0),
                height: content_box_pt(50.0),
                min_width: content_box_pt(10.0),
                min_height: content_box_pt(20.0),
                content_width: content_box_pt(90.0),
                content_height: content_box_pt(100.0),
                preferred_aspect_ratio: None,
                first_baseline: None,
                last_baseline: None,
            },
            swaps_physical_axes: false,
            replaced_used_size: None,
        };

        let min_content = measure_grid_item(
            taffy_layout::Size {
                width: None,
                height: None,
            },
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::MinContent,
                height: taffy_layout::AvailableSpace::MinContent,
            },
            Some(&mut estimate),
        );
        assert_eq!(min_content.width, 10.0);
        assert_eq!(min_content.height, 20.0);

        let max_content = measure_grid_item(
            taffy_layout::Size {
                width: None,
                height: None,
            },
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::MaxContent,
                height: taffy_layout::AvailableSpace::MaxContent,
            },
            Some(&mut estimate),
        );
        assert_eq!(max_content.width, 90.0);
        assert_eq!(max_content.height, 100.0);

        let preferred = measure_grid_item(
            taffy_layout::Size {
                width: None,
                height: None,
            },
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::Definite(300.0),
                height: taffy_layout::AvailableSpace::Definite(300.0),
            },
            Some(&mut estimate),
        );
        assert_eq!(preferred.width, 40.0);
        assert_eq!(preferred.height, 50.0);
    }

    #[test]
    fn vertical_grid_item_measurement_projects_logical_contributions_to_physical_axes() {
        let mut estimate = GridItemEstimate {
            metrics: IntrinsicItemMetrics {
                width: content_box_pt(40.0),
                height: content_box_pt(50.0),
                min_width: content_box_pt(10.0),
                min_height: content_box_pt(20.0),
                content_width: content_box_pt(90.0),
                content_height: content_box_pt(100.0),
                preferred_aspect_ratio: None,
                first_baseline: None,
                last_baseline: None,
            },
            swaps_physical_axes: true,
            replaced_used_size: None,
        };

        let max_content = measure_grid_item(
            taffy_layout::Size {
                width: None,
                height: None,
            },
            taffy_layout::Size {
                width: taffy_layout::AvailableSpace::MaxContent,
                height: taffy_layout::AvailableSpace::MaxContent,
            },
            Some(&mut estimate),
        );

        assert_eq!(max_content.width, 100.0);
        assert_eq!(max_content.height, 90.0);
    }

    #[test]
    fn intrinsic_auto_repeat_expands_once_for_indefinite_queries() {
        let components = [css::GridTrackListComponent::Repeat(
            Vec::new(),
            css::GridRepeat {
                count: css::GridRepeatCount::AutoFill,
                tracks: vec![css::GridTrackListComponent::Track(
                    Vec::new(),
                    fixed_track(20.0),
                )],
                trailing_names: Vec::new(),
            },
        )];

        let expanded = expanded_grid_tracks(
            &components,
            &["end".to_string()],
            &css::GridTemplateAreas::None,
        )
        .expect("auto-repeat should expand for intrinsic sizing");
        assert_eq!(expanded.sizes, vec![fixed_track(20.0)]);
        assert_eq!(
            expanded.line_names,
            vec![Vec::<String>::new(), vec!["end".to_string()]]
        );
    }

    #[test]
    fn intrinsic_scrollbar_contribution_follows_logical_axis_in_vertical_grid() {
        let mut style = ComputedStyle::initial();
        style.writing_mode = WritingMode::VerticalLr;
        style.overflow_x = css::Overflow::Scroll;
        let reservation = ScrollbarGutterReservation::for_style(
            &style,
            UsedOverflowAxes::from_style(&style),
            false,
            false,
        );

        assert_eq!(
            intrinsic_grid_scrollbar_axis_extent(&style, GridIntrinsicAxis::Inline, reservation)
                .points(),
            15.0 * css::CSS_PX_TO_PT,
        );
        assert_eq!(
            intrinsic_grid_scrollbar_axis_extent(&style, GridIntrinsicAxis::Block, reservation)
                .points(),
            0.0,
        );
    }
}
