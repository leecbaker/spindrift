use super::*;
use crate::units::content_box_to_margin_box_length;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct BlockIntrinsicContentSizes {
    min_inline: LogicalInlineContentSize,
    max_inline: LogicalInlineContentSize,
    min_block: LogicalBlockContentSize,
    max_block: LogicalBlockContentSize,
    /// Block-axis size after laying out with the available max-content inline
    /// size. In an orthogonal flow this is the physical-width contribution;
    /// it is not ordered with the block size after min-content wrapping.
    block_size_at_max_inline: LogicalBlockContentSize,
}

/// Inputs for a formatting context's intrinsic logical block contribution.
///
/// The inline measure and descendant percentage basis are layout constraints,
/// while the frozen boxes are the normalized source used by final layout.
/// Keeping the request at this boundary lets Flex, Grid, Table, replaced
/// elements, and ordinary flow retain ownership of their layout semantics.
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
/// <https://www.w3.org/TR/css-display-3/#formatting-context>
pub(in crate::layout) struct IntrinsicBlockContributionRequest<'boxes, 'dom> {
    pub(in crate::layout) available_inline_size: LogicalInlineContentSize,
    pub(in crate::layout) descendant_percentage_basis: BlockSizePercentageBasis,
    pub(in crate::layout) child_boxes: Option<&'boxes [box_tree::FormattingBox<'dom>]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(value: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(value),
        )
    }

    fn cyclic_length(length: f32, percentage: f32) -> css::ComputedLengthPercentageOrAuto {
        css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_affine(layout_pt(length), percentage, true),
        )
    }

    fn contribution(
        style: &ComputedStyle,
        descendant_min: f32,
        descendant_max: f32,
        vertical_non_content: f32,
    ) -> IntrinsicPhysicalHeightContributions {
        non_replaced_intrinsic_physical_height_contributions(
            style,
            content_box_pt(descendant_min),
            content_box_pt(descendant_max),
            non_content_pt(vertical_non_content),
        )
    }

    #[test]
    fn fixed_physical_height_replaces_descendant_height_contributions() {
        let mut style = ComputedStyle::initial();
        style.box_values.height = css::PhysicalHeight::from_computed(length(10.0));

        let contributions = contribution(&style, 4.0, 20.0, 0.0);

        assert_eq!(contributions.min.content_box_length(), content_box_pt(10.0));
        assert_eq!(contributions.max.content_box_length(), content_box_pt(10.0));
    }

    #[test]
    fn cyclic_max_height_does_not_clamp_a_fixed_intrinsic_height() {
        let mut style = ComputedStyle::initial();
        style.box_values.height = css::PhysicalHeight::from_computed(length(100.0));
        style.box_values.max_height = cyclic_length(0.0, 1.0);

        let contributions = contribution(&style, 0.0, 0.0, 0.0);

        assert_eq!(
            contributions.min.content_box_length(),
            content_box_pt(100.0)
        );
        assert_eq!(
            contributions.max.content_box_length(),
            content_box_pt(100.0)
        );
    }

    #[test]
    fn border_box_physical_height_converts_to_content_box_before_contributing() {
        let mut style = ComputedStyle::initial();
        style.box_sizing = css::BoxSizing::BorderBox;
        style.box_values.height = css::PhysicalHeight::from_computed(length(10.0));

        let contributions = contribution(&style, 0.0, 0.0, 4.0);

        assert_eq!(contributions.min.content_box_length(), content_box_pt(6.0));
        assert_eq!(contributions.max.content_box_length(), content_box_pt(6.0));
    }

    #[test]
    fn fixed_physical_height_constraints_bound_the_contribution() {
        let mut min_style = ComputedStyle::initial();
        min_style.box_values.height = css::PhysicalHeight::from_computed(length(10.0));
        min_style.box_values.min_height = length(12.0);
        let min_contributions = contribution(&min_style, 0.0, 0.0, 0.0);
        assert_eq!(
            min_contributions.min.content_box_length(),
            content_box_pt(12.0)
        );

        let mut max_style = ComputedStyle::initial();
        max_style.box_values.height = css::PhysicalHeight::from_computed(length(10.0));
        max_style.box_values.max_height = length(8.0);
        let max_contributions = contribution(&max_style, 0.0, 0.0, 0.0);
        assert_eq!(
            max_contributions.max.content_box_length(),
            content_box_pt(8.0)
        );
    }

    #[test]
    fn cyclic_preferred_and_max_heights_fall_back_while_min_height_uses_zero_percent() {
        let mut preferred_and_max = ComputedStyle::initial();
        preferred_and_max.box_values.height =
            css::PhysicalHeight::from_computed(cyclic_length(10.0, 0.5));
        preferred_and_max.box_values.max_height = cyclic_length(8.0, 0.5);
        let fallback = contribution(&preferred_and_max, 4.0, 20.0, 0.0);
        assert_eq!(fallback.min.content_box_length(), content_box_pt(4.0));
        assert_eq!(fallback.max.content_box_length(), content_box_pt(20.0));

        let mut minimum = ComputedStyle::initial();
        minimum.box_values.min_height = cyclic_length(10.0, 0.5);
        let minimum_contribution = contribution(&minimum, 4.0, 8.0, 0.0);
        assert_eq!(
            minimum_contribution.min.content_box_length(),
            content_box_pt(10.0)
        );
        assert_eq!(
            minimum_contribution.max.content_box_length(),
            content_box_pt(10.0)
        );
    }
}

/// Min/max intrinsic content-box contributions projected onto the physical
/// height axis.
///
/// Physical height remains distinct from a logical inline or block size until
/// the caller explicitly projects it through a writing mode. This matters for
/// a vertical parent, whose logical inline axis is physical height.
/// <https://www.w3.org/TR/css-writing-modes-4/#dimensional-mapping>
#[derive(Debug, Clone, Copy, PartialEq)]
struct IntrinsicPhysicalHeightContributions {
    min: PhysicalContentHeight,
    max: PhysicalContentHeight,
}

/// Resolve a non-replaced block child's intrinsic physical-height
/// contribution from its content-derived fallback and physical height rules.
///
/// Intrinsic contributions are calculated with an indefinite percentage basis.
/// The used-value helpers therefore apply CSS Sizing's cyclic-percentage
/// rules: cyclic preferred and maximum sizes behave as their initial values,
/// while cyclic minimum sizes resolve against zero. A definite physical
/// `height` fixes the child's content box and consequently replaces, rather
/// than floors, its descendant-derived contribution.
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
/// <https://www.w3.org/TR/css-sizing-3/#cyclic-percentage-contribution>
fn non_replaced_intrinsic_physical_height_contributions(
    style: &ComputedStyle,
    descendant_min: ContentBoxLength,
    descendant_max: ContentBoxLength,
    vertical_non_content: NonContentLength,
) -> IntrinsicPhysicalHeightContributions {
    let context = DescendantBlockPercentageContext::ContentSized;
    let constrain = |value| constrain_non_replaced_content_height(style, value, context);

    if let Some(height) =
        used_non_replaced_content_box_height_or_auto(style, context, vertical_non_content)
    {
        let height = PhysicalContentHeight::new(constrain(height));
        return IntrinsicPhysicalHeightContributions {
            min: height,
            max: height,
        };
    }

    IntrinsicPhysicalHeightContributions {
        min: PhysicalContentHeight::new(constrain(descendant_min)),
        max: PhysicalContentHeight::new(constrain(descendant_max)),
    }
}

/// Convert a physical content-height contribution to the child's physical
/// margin-box contribution exactly once.
///
/// CSS Sizing defines intrinsic contributions using the outer size, with auto
/// margins treated as zero. `intrinsic_box_metrics` provides that used-margin
/// behavior for this intrinsic-sizing boundary.
/// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
fn intrinsic_physical_height_margin_box_contribution(
    content_height: PhysicalContentHeight,
    vertical_non_content: NonContentLength,
    vertical_margins: LayoutLength,
) -> MarginBoxLength {
    content_box_to_margin_box_length(
        content_height.content_box_length(),
        vertical_non_content,
        vertical_margins,
    )
}

/// A physical-width result together with the orthogonal inline measure that
/// selected it.
///
/// A vertical auto-width block first negotiates its logical inline measure to
/// determine its physical block contribution. The final inline pass must use
/// that exact same measure rather than collecting and fitting the same text a
/// second time.
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
#[derive(Clone)]
struct ResolvedBlockPhysicalContentWidth {
    content_width: PhysicalContentWidth,
    selected_logical_inline_size: Option<LogicalInlineContentSize>,
    selected_orthogonal_inline_layout: Option<SelectedOrthogonalInlineLayout>,
}

impl ResolvedBlockPhysicalContentWidth {
    fn ordinary(content_width: PhysicalContentWidth) -> Self {
        Self {
            content_width,
            selected_logical_inline_size: None,
            selected_orthogonal_inline_layout: None,
        }
    }
}

impl BlockIntrinsicContentSizes {
    pub(in crate::layout) fn physical_width_min_max(
        self,
        axes: FlowAxes,
    ) -> (ContentBoxLength, ContentBoxLength) {
        if axes.writing_mode().has_vertical_lines() {
            // A vertical formatting context's logical block axis is physical
            // width. Its cross-size is selected after the inline content has
            // its max-content available space; using the block extent after
            // min-content wrapping would turn each wrapped line into a new
            // physical column and overstate an auto float's width.
            // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
            let width = self.block_size_at_max_inline.content_box_length();
            (width, width)
        } else {
            (
                axes.physical_width_from_logical_content_sizes(self.min_inline, self.min_block)
                    .content_box_length(),
                axes.physical_width_from_logical_content_sizes(self.max_inline, self.max_block)
                    .content_box_length(),
            )
        }
    }
}

/// Place a resolved physical border-box width in its normal-flow containing
/// block's axes.
///
/// A vertical containing block's logical block axis is physical horizontal,
/// so its children's resolved physical-width spans begin at the containing
/// block's logical block-start edge. A horizontal containing block instead
/// keeps the span produced by the ordinary horizontal width equation. In
/// particular, an orthogonal vertical child must not select the right edge
/// merely because its *own* block-start is right.
///
/// The span is deliberately typed as [`PageInlineSpan`]: this is the
/// sizing-to-placement boundary where a child physical extent becomes a
/// position in the containing block's physical coordinate system.
/// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
fn normal_flow_border_box_inline_span_in_containing_axes(
    containing_axes: FlowAxes,
    containing_inline_span: PageInlineSpan,
    resolved_border_box_inline_span: PageInlineSpan,
    margin_left: f32,
    margin_right: f32,
    root_pseudo_projection: Option<RootPseudoBlockProjection>,
) -> PageInlineSpan {
    let Some((block_start, block_end_inset)) = root_pseudo_projection
        .map(|projection| (projection.block_start, projection.block_end_inset.points()))
        .or_else(|| {
            containing_axes
                .writing_mode()
                .has_vertical_lines()
                .then(|| (containing_axes.block_start_side(), 0.0))
        })
    else {
        return resolved_border_box_inline_span;
    };

    let border_box_width = resolved_border_box_inline_span.width();
    let start = match block_start {
        PhysicalSide::Left => containing_inline_span.left_x() + margin_left,
        PhysicalSide::Right => {
            containing_inline_span.right_x() - margin_right - block_end_inset - border_box_width
        }
        PhysicalSide::Top | PhysicalSide::Bottom => {
            unreachable!("a horizontal physical block axis must start at left or right")
        }
    };
    PageInlineSpan::new(start, border_box_width)
}

impl<'a> LayoutBuilder<'a> {
    /// Return this box's intrinsic content-box contribution in its logical
    /// block axis, dispatching through the box's formatting context.
    ///
    /// CSS Sizing asks the formatting context that owns the box to determine
    /// its intrinsic contribution. A flex row therefore contributes its line
    /// cross size, a grid contributes its block tracks, and a table contributes
    /// its wrapper rather than being reinterpreted as an ordinary block stack.
    /// Box-model edges remain outside this result and are added exactly once by
    /// the containing formatting context.
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
    /// <https://www.w3.org/TR/css-display-3/#formatting-context>
    pub(in crate::layout) fn intrinsic_block_contribution(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        request: IntrinsicBlockContributionRequest<'_, '_>,
    ) -> LogicalBlockContentSize {
        if used_property_containment(element, style).size {
            return LogicalBlockContentSize::new(content_box_pt(0.0));
        }

        let (available_physical_width, _) = FlowAxes::for_style(style).physical_size(
            Some(request.available_inline_size.content_box_length()),
            None,
        );
        let available_physical_width = PhysicalContentWidth::new(
            available_physical_width.unwrap_or_else(|| content_box_pt(0.0)),
        );

        if style.display.is_flex() {
            return self.estimate_flex_intrinsic_block_contribution(
                element,
                style,
                stylesheets,
                request.available_inline_size,
                request.descendant_percentage_basis,
                request.child_boxes,
            );
        }
        if style.display.is_grid() {
            let (_, block_size) = self.estimate_grid_intrinsic_block_sizes(
                element,
                style,
                stylesheets,
                available_physical_width.points(),
                request.child_boxes,
            );
            return LogicalBlockContentSize::new(content_box_pt(block_size.max(0.0)));
        }
        if style.display.is_table() {
            let built_child_boxes;
            let child_boxes = if let Some(child_boxes) = request.child_boxes {
                child_boxes
            } else {
                built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                    element,
                    stylesheets,
                    style,
                );
                &built_child_boxes
            };
            let signature = self
                .ancestors
                .last()
                .cloned()
                .unwrap_or_else(|| element_signature(element));
            let fragment =
                box_tree::build_frozen_table_fragment(element, &signature, style, child_boxes);
            return self
                .table_wrapper_flex_sizing_from_fragment(
                    element,
                    style,
                    stylesheets,
                    &fragment,
                    available_physical_width.points(),
                )
                .wrapper_intrinsic_block;
        }
        if let Some(replaced) = resolve_replaced_element(
            element,
            style,
            ReplacedBoxSizingContext {
                available_width: available_physical_width.content_box_length(),
                inline_percentage_basis: PercentageBasis::definite_from(
                    request.available_inline_size.content_box_length(),
                    IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                ),
                block_basis: IntrinsicBlockBasis::from_layout_percentage_basis(
                    request.descendant_percentage_basis,
                ),
            },
            self.base_url,
            self.root_url,
            self.resource_cache,
        ) {
            let content_size = replaced.geometry().content_size;
            return FlowAxes::for_style(style).logical_block_from_physical_content_sizes(
                PhysicalContentWidth::new(content_box_pt(content_size.width)),
                PhysicalContentHeight::new(content_box_pt(content_size.height)),
            );
        }

        if style.writing_mode.has_vertical_lines() {
            return self.block_logical_block_size_at_inline_size(
                element,
                style,
                stylesheets,
                request.child_boxes,
                request.available_inline_size,
                // This legacy scalar is the containing box's percentage
                // measure, not the box's physical width. In vertical writing
                // it remains the logical inline size (physical height).
                request.available_inline_size.points(),
            );
        }

        let mut intrinsic_style = style.clone();
        intrinsic_style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(
                request.available_inline_size.points().max(0.0),
            ),
        );
        intrinsic_style.box_values.min_width = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style.box_values.max_width = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style
            .box_values
            .height
            .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
        intrinsic_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;

        let mut used_style = self.style_with_current_used_lengths(&intrinsic_style);
        let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            PercentageBasis::definite(request.available_inline_size),
        );
        self.block_percentage_context_stack
            .push_percentage_basis(request.descendant_percentage_basis);
        let outer_height = self.estimate_block_like_height(
            element,
            &intrinsic_style,
            stylesheets,
            request.available_inline_size.points(),
            request.child_boxes,
        );
        self.block_percentage_context_stack.pop();
        LogicalBlockContentSize::new(content_box_pt(
            (outer_height
                - used_style.margin.top
                - box_metrics.border.top.points()
                - used_style.padding.top
                - used_style.padding.bottom
                - box_metrics.border.bottom.points()
                - used_style.margin.bottom)
                .max(0.0),
        ))
    }

    pub(in crate::layout) fn translate_aligned_block_descendant_bookmarks(
        &mut self,
        descendant_bookmark_start: usize,
        page_index: usize,
        x_offset: f32,
        y_offset: f32,
    ) {
        if x_offset.abs() <= 0.01 && y_offset.abs() <= 0.01 {
            return;
        }
        for bookmark in self.bookmarks.iter_mut().skip(descendant_bookmark_start) {
            if bookmark.page_index == page_index {
                bookmark.translate_target(x_offset, y_offset);
            }
        }
    }

    /// Resolve a block box's physical used content width, including intrinsic keywords.
    ///
    /// CSS Sizing defines `min-content`, `max-content`, and `fit-content()` as
    /// intrinsic sizing keywords. CSS Writing Modes keeps `width` physical
    /// while applying sizing algorithms in logical axes, so vertical writing
    /// modes resolve physical width from logical block-size contributions:
    /// <https://www.w3.org/TR/css-sizing-3/#sizing-values>,
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>, and
    /// <https://www.w3.org/TR/CSS22/visudet.html#blockwidth>.
    pub(in crate::layout) fn used_block_physical_content_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        width_inputs: BlockContentWidthInputs,
    ) -> PhysicalContentWidth {
        self.resolved_block_physical_content_width(
            element,
            style,
            stylesheets,
            child_boxes,
            width_inputs,
        )
        .content_width
    }

    fn resolved_block_physical_content_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        width_inputs: BlockContentWidthInputs,
    ) -> ResolvedBlockPhysicalContentWidth {
        // In vertical and sideways writing modes, physical `width` maps to
        // logical block-size. An automatic block-size is content-derived for
        // every non-principal block, whether its containing flow is parallel
        // or orthogonal. Treating a parallel vertical box as a horizontal
        // `width:auto` block incorrectly fills the remaining block track.
        // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
        // <https://www.w3.org/TR/CSS22/visudet.html#normal-block>
        let vertical_auto_physical_block_size =
            style.writing_mode.has_vertical_lines() && style.box_values.width.is_auto();
        // A horizontal child of a vertical formatting context is likewise an
        // orthogonal flow root. Its physical `width` is its own inline size,
        // but it occupies the parent's logical block axis; `width:auto` must
        // therefore use its intrinsic fit-content contribution rather than
        // the horizontal block model's fill-available default.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let horizontal_orthogonal_auto_inline_size = !style.writing_mode.has_vertical_lines()
            && self.containing_block_writing_mode.has_vertical_lines()
            && style.box_values.width.is_auto();
        if vertical_auto_physical_block_size {
            if width_inputs.auto_width_role == BlockAutoWidthRole::NormalFlow
                && (element.tag.eq_ignore_ascii_case("html")
                    || self.principal_flow.is_source_body(element))
            {
                // The root principal box, or the selected body that supplies
                // the principal flow, is sized by the initial containing
                // block. Its propagated vertical writing mode changes the
                // logical axes used for layout, but does not turn the
                // viewport-sized document canvas into a shrink-to-fit box.
                //
                // <https://www.w3.org/TR/css-writing-modes-4/#principal-flow>
                // <https://www.w3.org/TR/css-display-3/#initial-containing-block>
                return ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                    content_box_pt(
                        (width_inputs.available_outer_width.points()
                            - width_inputs.horizontal_non_content.points())
                        .max(0.0),
                    ),
                ));
            }
            // A DOM-backed block normally constructs its child formatting
            // tree only for final layout. An orthogonal auto block must first
            // obtain the descendants' logical block contribution, however;
            // treating an absent frozen tree as an empty child list makes a
            // nested horizontal block contribute zero physical width.
            //
            // Keep the same frozen representation that final layout uses so
            // intrinsic selection and the resulting physical block size
            // describe one source flow.
            // <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
            // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
            let owned_child_boxes = child_boxes.is_none().then(|| {
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style)
            });
            let child_boxes = child_boxes.or(owned_child_boxes.as_deref());
            if style.display.is_grid() {
                // A vertical grid's physical width is its logical block
                // size. Its track algorithm already computes that physical
                // contribution from the items; treating it as an ordinary
                // block stack instead measures a synthetic page-axis height
                // and expands the grid past its intrinsic tracks.
                // <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes>
                // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
                let (min_content, max_content) = self.estimate_grid_intrinsic_block_sizes(
                    element,
                    style,
                    stylesheets,
                    width_inputs.available_outer_width.points(),
                    child_boxes,
                );
                let content_width = crate::layout::intrinsic::content_box_width_from_intrinsic(
                    style,
                    width_inputs.available_outer_width,
                    width_inputs.horizontal_non_content,
                    content_box_pt(min_content),
                    content_box_pt(max_content),
                    crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit,
                );
                return ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                    constrain_width_with_intrinsic(
                        style,
                        content_width,
                        content_box_pt(min_content),
                        content_box_pt(max_content),
                        width_inputs
                            .percentage_basis
                            .map_value(|basis| content_box_pt(basis.points())),
                        width_inputs.horizontal_non_content,
                    ),
                ));
            }
            // The auto physical width is the logical block contribution at
            // this box's *used* logical inline measure.  That measure is the
            // same fit-content negotiation used by final inline layout,
            // including an initial-containing-block fallback.  Measuring at
            // max-content here while final layout wraps at the ICB produces
            // columns outside the auto physical width.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
            let inline_size = width_inputs
                .definite_content_height
                .map(|height| LogicalInlineContentSize::new(height.content_box_length()))
                .unwrap_or_else(|| {
                    self.block_content_logical_inline_size(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        PhysicalContentWidth::new(content_box_pt(0.0)),
                        None,
                    )
                });
            let selected_orthogonal_inline_layout = width_inputs
                .definite_content_height
                .is_none()
                .then(|| {
                    self.select_orthogonal_inline_layout(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        inline_size,
                    )
                })
                .flatten();
            let block_size = if let Some(selected) = &selected_orthogonal_inline_layout {
                selected.logical_block_contribution.points()
            } else {
                let items = self.intrinsic_inline_items_for_element(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                );
                if items.is_empty() {
                    self.estimate_block_child_intrinsic_logical_block_size(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        width_inputs.available_outer_width.points(),
                        Some(inline_size),
                    )
                } else {
                    self.inline_items_logical_block_size(items, style, inline_size.points())
                }
            };
            let content_width = content_box_pt(block_size.max(0.0));
            return ResolvedBlockPhysicalContentWidth {
                content_width: PhysicalContentWidth::new(constrain_width_with_intrinsic(
                    style,
                    content_width,
                    content_width,
                    content_width,
                    width_inputs
                        .percentage_basis
                        .map_value(|basis| content_box_pt(basis.points())),
                    width_inputs.horizontal_non_content,
                )),
                selected_logical_inline_size: width_inputs
                    .definite_content_height
                    .is_none()
                    .then_some(inline_size),
                selected_orthogonal_inline_layout,
            };
        }
        let needs_intrinsic = vertical_auto_physical_block_size
            || horizontal_orthogonal_auto_inline_size
            || matches!(
                style.box_values.width,
                css::ComputedLengthPercentageOrAuto::MinContent
                    | css::ComputedLengthPercentageOrAuto::MaxContent
                    | css::ComputedLengthPercentageOrAuto::FitContent(_)
            );
        if !needs_intrinsic {
            if style.display.is_table() && has_auto_width(style) {
                return ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                    self.used_intrinsic_or_shrink_to_fit_width(
                        element,
                        style,
                        stylesheets,
                        width_inputs.available_outer_width,
                        width_inputs.horizontal_non_content,
                        child_boxes,
                        None,
                    ),
                ));
            }
            return ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                used_normal_flow_block_content_box_width(
                    style,
                    width_inputs
                        .percentage_basis
                        .value()
                        .unwrap_or(width_inputs.available_outer_width),
                    width_inputs.horizontal_non_content,
                ),
            ));
        }

        let (min_content, max_content) = self.block_intrinsic_physical_widths(
            element,
            style,
            stylesheets,
            child_boxes,
            width_inputs.available_outer_width.points(),
        );
        ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
            crate::layout::intrinsic::content_box_width_from_intrinsic(
                style,
                width_inputs.available_outer_width,
                width_inputs.horizontal_non_content,
                min_content,
                max_content,
                if vertical_auto_physical_block_size || horizontal_orthogonal_auto_inline_size {
                    crate::layout::intrinsic::IntrinsicAutoWidth::ShrinkToFit
                } else {
                    crate::layout::intrinsic::IntrinsicAutoWidth::FillAvailable
                },
            ),
        ))
    }

    /// Estimate block min/max-content sizes in logical axes.
    ///
    /// CSS Sizing defines intrinsic sizes per axis, and CSS Writing Modes maps
    /// physical width/height properties through logical inline/block sizing
    /// rules. Keeping both axes typed prevents a vertical physical `width`
    /// from accidentally consuming inline-size contributions:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>.
    pub(in crate::layout) fn block_intrinsic_content_sizes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> BlockIntrinsicContentSizes {
        let (min_inline, max_inline) = self.block_intrinsic_content_inline_widths(
            element,
            style,
            stylesheets,
            child_boxes,
            available_outer_width,
        );
        let (min_block, block_size_at_max_inline) = self.block_intrinsic_content_block_sizes(
            element,
            style,
            stylesheets,
            child_boxes,
            min_inline,
            available_outer_width,
        );
        BlockIntrinsicContentSizes {
            min_inline: LogicalInlineContentSize::new(content_box_pt(min_inline)),
            max_inline: LogicalInlineContentSize::new(content_box_pt(max_inline)),
            min_block: LogicalBlockContentSize::new(content_box_pt(min_block)),
            max_block: LogicalBlockContentSize::new(content_box_pt(
                block_size_at_max_inline.max(min_block),
            )),
            block_size_at_max_inline: LogicalBlockContentSize::new(content_box_pt(
                block_size_at_max_inline,
            )),
        }
    }

    /// Estimate only a block container's logical inline min/max-content
    /// sizes.
    ///
    /// Callers negotiating an orthogonal flow's used line measure do not need
    /// the logical block contribution.  Computing that contribution performs
    /// a second line-layout query at the min-content measure, which is both
    /// unrelated to fit-content selection and especially costly for nested
    /// `word-break: break-all` flows.
    ///
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
    pub(in crate::layout) fn block_intrinsic_content_inline_sizes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> (LogicalInlineContentSize, LogicalInlineContentSize) {
        let (min_inline, max_inline) = self.block_intrinsic_content_inline_widths(
            element,
            style,
            stylesheets,
            child_boxes,
            available_outer_width,
        );
        (
            LogicalInlineContentSize::new(content_box_pt(min_inline)),
            LogicalInlineContentSize::new(content_box_pt(max_inline)),
        )
    }

    /// Return a block container's intrinsic physical-width contribution.
    ///
    /// A horizontal flow projects physical width directly from logical inline
    /// sizes.  Do not calculate its independent logical block sizes merely to
    /// reach that same projection: that would re-run line layout at an
    /// unrelated intrinsic measure.  Vertical flows genuinely need their
    /// logical block contribution for a physical width, so they retain the
    /// complete measurement.
    ///
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
    pub(in crate::layout) fn block_intrinsic_physical_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> (ContentBoxLength, ContentBoxLength) {
        if !style.writing_mode.has_vertical_lines() {
            let (min_inline, max_inline) = self.block_intrinsic_content_inline_sizes(
                element,
                style,
                stylesheets,
                child_boxes,
                available_outer_width,
            );
            return (
                min_inline.content_box_length(),
                max_inline.content_box_length(),
            );
        }
        let width = self.vertical_intrinsic_physical_width_at_max_inline(
            element,
            style,
            stylesheets,
            child_boxes,
            available_outer_width,
        );
        (width, width)
    }

    /// Measure the physical width of a vertical flow at its max-content
    /// logical inline size.
    ///
    /// Physical width is the vertical flow's logical block size. Its
    /// min-content block size is not part of that projection, so calculating
    /// it only duplicates the paragraph layout used to measure the selected
    /// max-content contribution.
    ///
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes> and
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
    fn vertical_intrinsic_physical_width_at_max_inline(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> ContentBoxLength {
        debug_assert!(style.writing_mode.has_vertical_lines());
        // The root's inline pseudo-elements coexist with a block-level body
        // canvas, which is a special complete-intrinsic-size case below.
        if element.tag.eq_ignore_ascii_case("html") {
            return self
                .block_intrinsic_content_sizes(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    available_outer_width,
                )
                .physical_width_min_max(FlowAxes::for_style(style))
                .1;
        }
        if used_property_containment(element, style).size {
            let width = style
                .contain_intrinsic_size
                .height
                .clone()
                .map(|height| {
                    used_length_percentage(
                        height,
                        PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
                    )
                    .points()
                })
                .unwrap_or(0.0);
            return content_box_pt(width);
        }
        let items =
            self.intrinsic_inline_items_for_element(element, style, stylesheets, child_boxes);
        if items.is_empty() {
            return content_box_pt(self.estimate_block_child_intrinsic_logical_block_size(
                element,
                style,
                stylesheets,
                child_boxes,
                available_outer_width,
                None,
            ));
        }
        content_box_pt(self.inline_items_logical_block_size(items, style, f32::MAX))
    }

    /// Estimate block min-content and max-content content-box inline sizes.
    ///
    /// CSS Sizing computes intrinsic contributions from text soft-wrap
    /// opportunities and descendant intrinsic widths. This helper covers the
    /// normal block text paths used by block layout and falls back to the
    /// existing shrink-to-fit estimator for non-inline descendants until block
    /// intrinsic sizing is fully structured across every formatting context:
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic>.
    pub(in crate::layout) fn block_intrinsic_content_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        self.block_intrinsic_content_inline_widths(
            element,
            style,
            stylesheets,
            child_boxes,
            available_outer_width,
        )
    }

    fn block_intrinsic_content_inline_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
    ) -> (f32, f32) {
        // CSS Containment sizes the content box as empty while preserving the
        // containment box's own formatting-context contributions. In
        // particular, an empty grid still contributes explicit tracks and an
        // empty multicol still contributes authored column geometry.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if intrinsic_inline_size_is_contained(style) {
            if let Some(width) = contained_intrinsic_logical_inline_size(style) {
                let width = used_length_percentage(
                    width,
                    PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
                )
                .points();
                return (width, width);
            }
            if style.display.is_grid() {
                return self.size_contained_grid_intrinsic_widths(style);
            }
            return size_contained_multicol_intrinsic_inline_sizes(style).unwrap_or((0.0, 0.0));
        }
        if style.display.is_flex() {
            let contributions = self.estimate_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                PhysicalContentWidth::new(content_box_pt(available_outer_width)),
                child_boxes,
            );
            return (
                contributions.min_content.points(),
                contributions.max_content.points(),
            );
        }
        if style.display.is_grid() {
            return self.estimate_grid_intrinsic_widths(
                element,
                style,
                stylesheets,
                available_outer_width,
                child_boxes,
            );
        }
        // A block container's intrinsic inline contribution is the largest
        // contribution of its in-flow block formatting-context descendants.
        // Inline collection deliberately omits block children, so recursively
        // query every block child here. This is particularly important for an
        // atomic inline formatting context, whose shrink-to-fit width must
        // include its block descendants.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution> and
        // <https://www.w3.org/TR/css-grid-1/#intrinsic-sizes>.
        let built_child_boxes;
        let child_boxes = match child_boxes {
            Some(child_boxes) => Some(child_boxes),
            None => {
                built_child_boxes = self.build_frozen_child_boxes_with_current_ancestors(
                    element,
                    stylesheets,
                    style,
                );
                Some(built_child_boxes.as_slice())
            }
        };
        if let Some(child_boxes) = child_boxes {
            let mut block_child_min = 0.0_f32;
            let mut block_child_max = 0.0_f32;
            for child in child_boxes {
                // A table's intrinsic inline contribution is its table-wrapper
                // margin box, after CSS Tables has clamped an auto-layout grid
                // to its min-content width.  The generic block path only sees
                // the table's preferred `inline-size`, which can incorrectly
                // make a `width: min-content` ancestor narrower than an
                // unbreakable table cell.
                // <https://drafts.csswg.org/css-tables-3/#computing-the-table-width>
                // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
                if let box_tree::FormattingBox::Table(table) = child {
                    let (child_min, child_max) = self.table_outer_intrinsic_widths_from_fragment(
                        table.core.element,
                        &table.core.style,
                        stylesheets,
                        &table.fragment,
                        available_outer_width,
                    );
                    block_child_min = block_child_min.max(child_min);
                    block_child_max = block_child_max.max(child_max);
                    continue;
                }
                let Some((child_element, _, child_style, child_children)) = child.element_parts()
                else {
                    continue;
                };
                // Inline descendants participate together in the parent's
                // inline formatting context. Treating them independently as
                // block children would discard their unbreakable sequence's
                // max-content contribution (for example two adjacent atomic
                // inlines), so recurse here only for block-level children.
                // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
                if !child_style.display.is_block_level() {
                    continue;
                }
                let metrics = intrinsic_box_metrics(child_style);
                if style.writing_mode.has_vertical_lines() {
                    // A vertical parent's logical inline axis is physical
                    // height. Its block children therefore contribute their
                    // physical outer height, not their physical width. This
                    // is essential for nested orthogonal roots: a horizontal
                    // child can obtain its height from a vertical grandchild
                    // even though neither child has inline text of its own.
                    // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
                    let specified_width = used_content_box_size(
                        child_style.box_values.width.clone(),
                        child_style.box_sizing,
                        PercentageBasis::definite(content_box_pt(available_outer_width)),
                        metrics.horizontal_non_content_length(),
                    );
                    let horizontal_child_line_measure =
                        (child_style.writing_mode == WritingMode::HorizontalTb).then(|| {
                            specified_width.unwrap_or_else(|| {
                                // An auto-sized horizontal child of a vertical
                                // flow root shrink-to-fits its physical width.
                                // Its parent's logical inline contribution is
                                // therefore the height at that final line
                                // measure, not the min-content wrapped height.
                                // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
                                let (min_width, max_width) = self.block_intrinsic_physical_widths(
                                    child_element,
                                    child_style,
                                    stylesheets,
                                    Some(child_children),
                                    available_outer_width,
                                );
                                let available_width = content_box_pt(
                                    (available_outer_width
                                        - metrics.margin.left.points()
                                        - metrics.margin.right.points()
                                        - metrics.horizontal_non_content_length().points())
                                    .max(0.0),
                                );
                                crate::layout::intrinsic::shrink_to_fit_width(
                                    min_width,
                                    max_width,
                                    available_width,
                                )
                            })
                        });
                    let (descendant_min, descendant_max) =
                        if let Some(line_measure) = horizontal_child_line_measure {
                            // A horizontal child's used line measure determines
                            // its physical height contribution. Measuring at
                            // min-content would wrap text into extra lines and
                            // incorrectly make the vertical parent taller.
                            // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
                            let items = self.intrinsic_inline_items_for_element(
                                child_element,
                                child_style,
                                stylesheets,
                                Some(child_children),
                            );
                            let height = if items.is_empty() {
                                self.estimate_block_child_intrinsic_content_height(
                                    child_element,
                                    child_style,
                                    stylesheets,
                                    Some(child_children),
                                    line_measure.points(),
                                    available_outer_width,
                                )
                            } else {
                                self.inline_items_logical_block_size(
                                    items,
                                    child_style,
                                    line_measure.points(),
                                )
                            };
                            (height, height)
                        } else if child_style.writing_mode.has_vertical_lines() {
                            // Parallel vertical flows share their physical
                            // height with the logical inline axis. Preserve
                            // the child's independent min/max inline
                            // contributions directly; asking for complete
                            // physical geometry here also measures an
                            // unrelated logical block size and can collapse
                            // both contributions to max-content.
                            let (child_min, child_max) = self.block_intrinsic_content_inline_sizes(
                                child_element,
                                child_style,
                                stylesheets,
                                Some(child_children),
                                available_outer_width,
                            );
                            (child_min.points(), child_max.points())
                        } else {
                            let child_sizes = self.block_intrinsic_content_sizes(
                                child_element,
                                child_style,
                                stylesheets,
                                Some(child_children),
                                available_outer_width,
                            );
                            let child_axes = FlowAxes::for_style(child_style);
                            (
                                child_axes
                                    .physical_height_from_logical_content_sizes(
                                        child_sizes.min_inline,
                                        child_sizes.min_block,
                                    )
                                    .points(),
                                child_axes
                                    .physical_height_from_logical_content_sizes(
                                        child_sizes.max_inline,
                                        child_sizes.max_block,
                                    )
                                    .points(),
                            )
                        };
                    let descendant_min = content_box_pt(descendant_min);
                    let descendant_max = content_box_pt(descendant_max);
                    let vertical_non_content = metrics.vertical_non_content_length();
                    let vertical_margins = metrics.margin.top + metrics.margin.bottom;
                    let contributions = non_replaced_intrinsic_physical_height_contributions(
                        child_style,
                        descendant_min,
                        descendant_max,
                        vertical_non_content,
                    );
                    block_child_min = block_child_min.max(
                        intrinsic_physical_height_margin_box_contribution(
                            contributions.min,
                            vertical_non_content,
                            vertical_margins,
                        )
                        .points(),
                    );
                    block_child_max = block_child_max.max(
                        intrinsic_physical_height_margin_box_contribution(
                            contributions.max,
                            vertical_non_content,
                            vertical_margins,
                        )
                        .points(),
                    );
                } else {
                    let (child_min, child_max) = if writing_modes_are_orthogonal(
                        style.writing_mode,
                        child_style.writing_mode,
                    ) {
                        // The parent's logical inline axis is physical
                        // horizontal here. Project an orthogonal child's two
                        // logical contributions through its own flow axes;
                        // using its logical inline size directly would use a
                        // vertical child's physical height as a width.
                        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
                        let specified_width = used_content_box_size(
                            child_style.box_values.width.clone(),
                            child_style.box_sizing,
                            PercentageBasis::definite(content_box_pt(available_outer_width)),
                            metrics.horizontal_non_content_length(),
                        )
                        .map(SemanticLengthExt::points);
                        if let Some(width) = specified_width {
                            (width, width)
                        } else {
                            let (child_min, child_max) = self.block_intrinsic_physical_widths(
                                child_element,
                                child_style,
                                stylesheets,
                                Some(child_children),
                                available_outer_width,
                            );
                            (child_min.points(), child_max.points())
                        }
                    } else {
                        self.block_intrinsic_content_widths(
                            child_element,
                            child_style,
                            stylesheets,
                            Some(child_children),
                            available_outer_width,
                        )
                    };
                    let horizontal_non_content = metrics.horizontal_non_content_length();
                    let (child_min, child_max) = non_replaced_intrinsic_width_contributions(
                        child_style,
                        content_box_pt(child_min),
                        content_box_pt(child_max),
                        horizontal_non_content,
                    );
                    block_child_min = block_child_min.max(
                        child_min.points()
                            + horizontal_non_content.points()
                            + metrics.margin.left.points()
                            + metrics.margin.right.points(),
                    );
                    block_child_max = block_child_max.max(
                        child_max.points()
                            + horizontal_non_content.points()
                            + metrics.margin.left.points()
                            + metrics.margin.right.points(),
                    );
                }
            }
            if block_child_min > 0.0 || block_child_max > 0.0 {
                return (block_child_min, block_child_max.max(block_child_min));
            }
        }
        let contribution =
            self.with_intrinsic_inline_percentage_basis(PercentageBasis::indefinite(), |layout| {
                layout.intrinsic_inline_contribution_for_element(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                )
            });
        if contribution.max_content.points() > 0.0 || contribution.min_content.points() > 0.0 {
            return (
                contribution.min_content.points(),
                contribution.max_content.points(),
            );
        }
        let shrink_to_fit = self
            .estimate_shrink_to_fit_width(
                element,
                style,
                stylesheets,
                content_box_pt(available_outer_width),
                child_boxes,
                None,
            )
            .points();
        (shrink_to_fit, shrink_to_fit)
    }

    fn block_intrinsic_content_block_sizes(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        min_inline: f32,
        available_outer_width: f32,
    ) -> (f32, f32) {
        if used_property_containment(element, style).size {
            let height = style
                .contain_intrinsic_size
                .height
                .clone()
                .map(|height| {
                    used_length_percentage(
                        height,
                        PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
                    )
                    .points()
                })
                .unwrap_or(0.0);
            return (height, height);
        }
        let items =
            self.intrinsic_inline_items_for_element(element, style, stylesheets, child_boxes);
        if items.is_empty() {
            let block_size = if style.writing_mode.has_vertical_lines() {
                self.estimate_block_child_intrinsic_logical_block_size(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    available_outer_width,
                    Some(LogicalInlineContentSize::new(content_box_pt(min_inline))),
                )
            } else {
                self.estimate_block_child_intrinsic_content_height(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    min_inline,
                    available_outer_width,
                )
            };
            return (block_size, block_size);
        }

        let min_block = self.inline_items_logical_block_size(
            items.clone(),
            style,
            // A zero font size produces no text advance or inline strut.
            // Intrinsic measurement must keep that zero available span rather
            // than manufacturing one pixel (or one em) of text contribution.
            // <https://drafts.csswg.org/css-fonts-4/#font-size-prop>
            min_inline.max(0.0),
        );
        let max_block = self.inline_items_logical_block_size(items, style, f32::MAX);
        if element.tag.eq_ignore_ascii_case("html") {
            // The root's tree-abiding inline pseudo-elements coexist with its
            // block-level body canvas. Inline collection intentionally omits
            // that block child, but its intrinsic block contribution still
            // determines the root's used principal-flow span.
            // <https://www.w3.org/TR/css-sizing-3/#intrinsic-contribution>
            // <https://www.w3.org/TR/css-pseudo-4/#generated-content>
            let block_child_size = self.estimate_block_child_intrinsic_content_height(
                element,
                style,
                stylesheets,
                child_boxes,
                min_inline,
                available_outer_width,
            );
            (
                min_block.max(block_child_size),
                max_block.max(block_child_size),
            )
        } else {
            (min_block, max_block)
        }
    }

    /// Estimate a vertical block's logical block-size from its block children.
    ///
    /// The block axis of a vertical formatting context is physical horizontal.
    /// Reusing the ordinary auto-height estimator here would instead add each
    /// child's physical height, which is its logical inline contribution and
    /// can double the physical width of nested orthogonal boxes.
    /// <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
    pub(in crate::layout) fn estimate_block_child_intrinsic_logical_block_size(
        &mut self,
        _element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
        selected_parent_logical_inline_measure: Option<LogicalInlineContentSize>,
    ) -> f32 {
        let Some(child_boxes) = child_boxes else {
            return 0.0;
        };
        let parent_logical_inline_measure = selected_parent_logical_inline_measure
            .map(LogicalInlineContentSize::content_box_length)
            .or_else(|| {
                used_content_box_size(
                    style.box_values.height.value().clone(),
                    style.box_sizing,
                    PercentageBasis::definite(content_box_pt(available_outer_width)),
                    intrinsic_box_metrics(style).vertical_non_content_length(),
                )
            });
        let mut block_size = 0.0;
        for child in child_boxes {
            let Some((child_element, _, child_style, child_children)) = child.element_parts()
            else {
                continue;
            };
            let mut used_child_style = self.style_with_current_used_lengths(child_style);
            let child_metrics = apply_used_box_metrics_for_logical_inline_basis(
                &mut used_child_style,
                PercentageBasis::definite(LogicalInlineContentSize::new(
                    parent_logical_inline_measure
                        .unwrap_or_else(|| content_box_pt(available_outer_width.max(0.0))),
                )),
            );
            let child_style = &used_child_style;
            if !child_style.display.is_block_level()
                || child_style.float != Float::None
                || matches!(child_style.position, Position::Absolute | Position::Fixed)
            {
                continue;
            }
            let physical_width_percentage_basis =
                if writing_modes_are_orthogonal(style.writing_mode, child_style.writing_mode) {
                    self.current_child_available_space()
                        .orthogonal_physical_width_percentage_basis
                        .points()
                } else {
                    available_outer_width
                };
            let wrapped_vertical_child_width = (child_style.writing_mode.has_vertical_lines()
                && child_style.box_values.width.is_auto())
            .then(|| {
                parent_logical_inline_measure.and_then(|inline_measure| {
                    let items = self.intrinsic_inline_items_for_element(
                        child_element,
                        child_style,
                        stylesheets,
                        Some(child_children),
                    );
                    (!items.is_empty()).then(|| {
                        self.inline_items_logical_block_size(
                            items,
                            child_style,
                            inline_measure.points(),
                        )
                    })
                })
            })
            .flatten();
            let specified_width = used_content_box_size(
                child_style.box_values.width.clone(),
                child_style.box_sizing,
                PercentageBasis::definite(content_box_pt(physical_width_percentage_basis)),
                child_metrics.horizontal_non_content_length(),
            )
            .map(SemanticLengthExt::points);
            let child_width = specified_width
                .or(wrapped_vertical_child_width)
                .unwrap_or_else(|| {
                    self.block_intrinsic_physical_widths(
                        child_element,
                        child_style,
                        stylesheets,
                        Some(child_children),
                        available_outer_width,
                    )
                    .1
                    .points()
                });
            block_size += child_width
                + child_metrics.horizontal_non_content_length().points()
                + child_metrics.margin.left.points()
                + child_metrics.margin.right.points();
        }
        block_size
    }

    /// Export the last compatible inline baseline from a vertical block-child
    /// stack during intrinsic atomic sizing.
    ///
    /// This is the baseline half of the same geometry query above: it walks
    /// the normalized block flow, ignores out-of-flow children, and never
    /// creates committed fragments. A child with parallel axes can export its
    /// last line; an orthogonal or layout-contained child cannot.
    /// <https://drafts.csswg.org/css-align-3/#baseline-export>
    pub(in crate::layout) fn estimate_block_child_intrinsic_last_baseline(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        available_outer_width: f32,
        selected_parent_logical_inline_measure: LogicalInlineContentSize,
    ) -> Option<LayoutLength> {
        let child_boxes = child_boxes?;
        for (index, child) in child_boxes.iter().enumerate().rev() {
            let Some((child_element, _, child_style, child_children)) = child.element_parts()
            else {
                continue;
            };
            if !child_style.display.is_block_level()
                || child_style.float != Float::None
                || matches!(child_style.position, Position::Absolute | Position::Fixed)
                || writing_modes_are_orthogonal(style.writing_mode, child_style.writing_mode)
                || used_property_containment(child_element, child_style).layout
            {
                continue;
            }
            let preceding_extent = self.estimate_block_child_intrinsic_logical_block_size(
                element,
                style,
                stylesheets,
                Some(&child_boxes[..index]),
                available_outer_width,
                Some(selected_parent_logical_inline_measure),
            );
            let metrics = intrinsic_box_metrics(child_style);
            let block_start_margin = match block_start_side(style.writing_mode) {
                PhysicalSide::Right => metrics.margin.right.points(),
                PhysicalSide::Left => metrics.margin.left.points(),
                PhysicalSide::Top => metrics.margin.top.points(),
                PhysicalSide::Bottom => metrics.margin.bottom.points(),
            };
            let items = self.intrinsic_inline_items_for_element(
                child_element,
                child_style,
                stylesheets,
                Some(child_children),
            );
            if !items.is_empty() {
                let sequence = self.collect_inline_line_sequence_with_text_box_trim(
                    items,
                    child_style,
                    selected_parent_logical_inline_measure.points(),
                    0.0,
                    0.0,
                );
                let baseline = self.inline_box_sequence_baseline_offset(
                    &sequence,
                    child_style,
                    metrics.border.to_css_edges(),
                )?;
                return Some(layout_pt(preceding_extent + block_start_margin + baseline));
            }
            if let Some(descendant_baseline) = self.estimate_block_child_intrinsic_last_baseline(
                child_element,
                child_style,
                stylesheets,
                Some(child_children),
                available_outer_width,
                selected_parent_logical_inline_measure,
            ) {
                return Some(layout_pt(
                    preceding_extent + block_start_margin + descendant_baseline.points(),
                ));
            }
        }
        None
    }

    fn estimate_block_child_intrinsic_content_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        min_inline: f32,
        available_outer_width: f32,
    ) -> f32 {
        let mut intrinsic_style = style.clone();
        intrinsic_style.box_values.width = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(min_inline.max(0.0)),
        );
        intrinsic_style.box_values.min_width = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style.box_values.max_width = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style
            .box_values
            .height
            .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
        intrinsic_style.box_values.min_height = css::ComputedLengthPercentageOrAuto::Auto;
        intrinsic_style.box_values.max_height = css::ComputedLengthPercentageOrAuto::Auto;

        let mut used_style = self.style_with_current_used_lengths(&intrinsic_style);
        let inline_basis = available_outer_width.max(min_inline).max(0.0);
        let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            PercentageBasis::definite(LogicalInlineContentSize::new(content_box_pt(inline_basis))),
        );
        let outer_height = self.estimate_block_like_height(
            element,
            &intrinsic_style,
            stylesheets,
            inline_basis,
            child_boxes,
        );
        (outer_height
            - used_style.margin.top
            - box_metrics.border.top.points()
            - used_style.padding.top
            - used_style.padding.bottom
            - box_metrics.border.bottom.points()
            - used_style.margin.bottom)
            .max(0.0)
    }

    fn inline_items_logical_block_size(
        &mut self,
        items: Vec<InlineItem>,
        style: &ComputedStyle,
        available_inline_size: f32,
    ) -> f32 {
        let sequence = self.collect_inline_line_sequence_with_text_box_trim(
            items,
            style,
            available_inline_size,
            0.0,
            0.0,
        );
        // Atomic margin boxes already contribute their logical block size to
        // the collected line metrics. Do not subtract the block-start margin
        // again here: paint keeps the line-box anchor stable and applies the
        // margin exactly once when placing the atom's border box.
        sequence.total_height().max(0.0)
    }

    /// Measure a vertical block container's logical block contribution at a
    /// definite logical inline size.
    ///
    /// A vertical float with a definite `inline-size` wraps its inline
    /// contents at that measure before shrink-to-fit resolves its automatic
    /// physical width. Measuring at max-content inline size would collapse
    /// the text into one column and understate that physical width:
    /// <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>,
    /// <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>, and
    /// <https://www.w3.org/TR/CSS22/visudet.html#float-width>.
    pub(in crate::layout) fn block_logical_block_size_at_inline_size(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        logical_inline_size: LogicalInlineContentSize,
        available_outer_width: f32,
    ) -> LogicalBlockContentSize {
        debug_assert!(style.writing_mode.has_vertical_lines());
        if let Some(selected) = self.select_orthogonal_inline_layout(
            element,
            style,
            stylesheets,
            child_boxes,
            logical_inline_size,
        ) {
            return selected.logical_block_contribution;
        }
        let items =
            self.intrinsic_inline_items_for_element(element, style, stylesheets, child_boxes);
        if items.is_empty() {
            return LogicalBlockContentSize::new(content_box_pt(
                self.estimate_block_child_intrinsic_logical_block_size(
                    element,
                    style,
                    stylesheets,
                    child_boxes,
                    available_outer_width,
                    Some(logical_inline_size),
                ),
            ));
        }
        LogicalBlockContentSize::new(content_box_pt(self.inline_items_logical_block_size(
            items,
            style,
            logical_inline_size.points(),
        )))
    }

    /// Select the exact simple vertical line stack that gives an orthogonal
    /// auto-sized block both its logical block contribution and final paint.
    ///
    /// The selector is intentionally narrower than general inline layout:
    /// any source with float, break, or continuation behavior leaves this
    /// `None` and keeps the established final-layout path.
    /// <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
    fn select_orthogonal_inline_layout(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        logical_inline_measure: LogicalInlineContentSize,
    ) -> Option<SelectedOrthogonalInlineLayout> {
        // The selected sequence must be built from the same frozen-box
        // collection semantics as final inline layout. Intrinsic collection
        // deliberately omits positioned descendants and has separate atomic
        // construction, so using it here can select columns that final paint
        // cannot replay.
        // <https://drafts.csswg.org/css-inline-3/#line-box>
        let owned_child_boxes = child_boxes.is_none().then(|| {
            self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style)
        });
        let child_boxes = child_boxes.or(owned_child_boxes.as_deref())?;
        let frozen_replay_input = self.collect_frozen_inline_replay_input(
            child_boxes,
            stylesheets,
            element.attrs.get("href").cloned(),
            0.0,
            InlineVisualOffset::zero(),
            style,
            style.text_decoration_origins.effective_layers_vec(),
        );
        if !frozen_replay_input.is_replay_safe() {
            return None;
        }
        let mut items = frozen_replay_input.selection_items();
        if let Some(marker) =
            self.marker_for_list_item(element, style, self.containing_block_direction)
            && marker.participates_in_first_line()
            && marker.has_in_flow_content()
        {
            // An inside marker is generated content in the list item's
            // principal inline flow. Include it in the selected sequence so
            // orthogonal auto sizing and final paint share one line stack.
            // <https://drafts.csswg.org/css-lists-3/#list-style-position-property>
            let mut marker_items = Vec::new();
            self.push_inside_marker_items(&marker, style, None, &mut marker_items);
            marker_items.append(&mut items);
            items = marker_items;
        }
        // A replay-selected inline sequence is only authoritative when it
        // actually represents inline content. An empty sequence would mask
        // block children, whose intrinsic logical block contribution is the
        // auto physical width of a vertical orthogonal root.
        // <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
        if items.is_empty() {
            return None;
        }
        let sequence = self.select_replay_safe_vertical_inline_sequence(
            &mut items,
            style,
            logical_inline_measure.points(),
            0.0,
            0.0,
            stylesheets,
        )?;
        Some(SelectedOrthogonalInlineLayout {
            logical_inline_measure,
            logical_block_contribution: LogicalBlockContentSize::new(
                sequence.logical_block_stack_extent(),
            ),
            line_sequence: sequence,
            frozen_replay_input,
        })
    }

    pub(in crate::layout) fn block_layout_geometry(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> BlockLayoutGeometry {
        if let Some(constraint) = self.direct_block_layout_constraint_for(element) {
            return self.block_layout_geometry_in_inline_span(
                element,
                style,
                stylesheets,
                child_boxes,
                constraint,
            );
        }
        let containing_inline_size = (self.content_right - self.content_left).max(0.0);
        let child_available_space = self.current_child_available_space();
        // CSS Box percentages use the containing block's logical inline
        // size, even when this block establishes an orthogonal flow.
        // <https://drafts.csswg.org/css-writing-modes-4/#orthogonal-flows>
        let percentage_basis = child_available_space
            .logical_inline_percentage_basis_for(child_available_space.writing_mode);
        let physical_width_percentage_basis = if crate::layout::block::writing_modes_are_orthogonal(
            child_available_space.writing_mode,
            style.writing_mode,
        ) {
            child_available_space
                .orthogonal_physical_width_percentage_basis
                .points()
        } else if self.active_fragmentainer_kind() == FragmentainerKind::Column
            && crate::layout::block::writing_modes_are_orthogonal(
                self.containing_block_writing_mode,
                style.writing_mode,
            )
        {
            // An orthogonal flow root fragments through the multicol
            // container's block axis. Its auto physical width is therefore
            // resolved against the multicol content box exported to its
            // children, rather than against one anonymous column slice.
            // <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto>
            child_available_space.physical_content_width.points()
        } else {
            containing_inline_size
        };
        self.block_layout_geometry_in_inline_span(
            element,
            style,
            stylesheets,
            child_boxes,
            BlockLayoutInlineConstraint {
                containing_inline_span: PageInlineSpan::from_edges(
                    self.content_left,
                    self.content_right,
                ),
                percentage_basis,
                physical_width_percentage_basis: PhysicalContentWidth::new(content_box_pt(
                    physical_width_percentage_basis,
                )),
                auto_border_box_width: None,
            },
        )
    }

    pub(in crate::layout) fn block_layout_geometry_in_inline_span(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        constraint: BlockLayoutInlineConstraint,
    ) -> BlockLayoutGeometry {
        let containing_inline_span = constraint.containing_inline_span;
        let percentage_basis = constraint.percentage_basis;
        let physical_width_percentage_basis = constraint.physical_width_percentage_basis.points();
        let containing_inline_size = containing_inline_span.width();
        let mut used_style = self.style_with_current_used_lengths(style);
        let box_metrics =
            apply_used_box_metrics_for_logical_inline_basis(&mut used_style, percentage_basis);
        let relative_offset = self.normal_flow_relative_position_offset(&used_style);
        let has_indefinite_orthogonal_containing_width =
            crate::layout::block::writing_modes_are_orthogonal(
                self.containing_block_writing_mode,
                used_style.writing_mode,
            ) && !self
                .current_child_available_space()
                .physical_width_is_definite;
        // An auto-sized vertical containing block has an indefinite physical
        // width while its horizontal child is being sized.  The child's auto
        // inline measure therefore uses the Writing Modes fallback, rather
        // than the parent's eventual content-derived zero width.  This is an
        // available-space constraint only: it does not make percentages
        // definite.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let auto_inline_constraint =
            if has_indefinite_orthogonal_containing_width && has_auto_width(&used_style) {
                physical_width_percentage_basis
            } else {
                containing_inline_size
            };
        let available_outer_width =
            normal_flow_block_available_outer_width(&used_style, layout_pt(auto_inline_constraint));
        // Intrinsic sizing normally treats a percentage-dependent width as
        // auto until its containing-block basis is known. At this layout
        // boundary the physical width basis is known separately from the
        // available inline span for an orthogonal flow, so retain that basis
        // when resolving the specified physical width.
        // <https://www.w3.org/TR/css-sizing-3/#intrinsic-sizes>
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let intrinsic_width_percentage_basis = matches!(
            used_style.box_values.width,
            css::ComputedLengthPercentageOrAuto::LengthPercentage(ref value)
                if value.needs_percentage_basis()
        )
        .then_some(layout_pt(physical_width_percentage_basis))
        .unwrap_or(available_outer_width);
        let border_edges = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content_length();
        let vertical_extras = box_metrics.vertical_non_content_length();
        let containing_block_percentage_context =
            self.block_percentage_context_stack.current_context();
        let containing_block_content_height =
            containing_block_percentage_context.percentage_basis();
        let containing_block_stretch_height = containing_block_content_height
            .map_value(crate::units::IntoLayoutLength::into_layout_length)
            .value()
            .unwrap_or_else(|| layout_pt(0.0));
        let height_depends_on_intrinsic_content =
            needs_intrinsic_height_contribution(used_style.box_values.height.value().clone())
                || needs_intrinsic_height_contribution(used_style.box_values.min_height.clone())
                || needs_intrinsic_height_contribution(used_style.box_values.max_height.clone());
        // An `auto`-basis calc-size on the dependent block axis can be made
        // definite by a definite inline size and preferred aspect ratio. Do
        // not confuse that transfer with an intrinsic sizing query: the
        // transfer establishes the `auto` basis before calc-size applies its
        // arithmetic.
        // <https://drafts.csswg.org/css-values-5/#calc-size> and
        // <https://www.w3.org/TR/css-sizing-4/#aspect-ratio>.
        let aspect_ratio_resolves_auto_basis_height = used_style
            .box_values
            .height
            .calc_size_with_auto_basis()
            .is_some()
            && !has_auto_width(&used_style)
            && !needs_intrinsic_width_contribution(used_style.box_values.width.clone());
        let explicit_content_height = (!height_depends_on_intrinsic_content
            || aspect_ratio_resolves_auto_basis_height)
            .then(|| {
                used_content_box_height_or_auto_with_basis(
                    &used_style,
                    containing_block_content_height,
                    vertical_extras,
                )
                .map(SemanticLengthExt::points)
            })
            .flatten();
        // At this normal-flow layout boundary the physical-width percentage
        // basis is already definite.  Percentages need intrinsic fallback
        // only during an intrinsic query with an indefinite containing block;
        // treating ordinary `width`/`min-width: 0%` as such a query forces
        // every block descendant through min/max-content measurement.
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        let width_needs_intrinsic_sizes = |value: &css::ComputedLengthPercentageOrAuto| {
            !matches!(
                value,
                css::ComputedLengthPercentageOrAuto::LengthPercentage(_)
            ) && needs_intrinsic_width_contribution(value.clone())
        };
        let needs_intrinsic_sizes = width_needs_intrinsic_sizes(&used_style.box_values.width)
            || width_needs_intrinsic_sizes(&used_style.box_values.min_width)
            || width_needs_intrinsic_sizes(&used_style.box_values.max_width)
            || (used_style.box_values.width.is_auto()
                && used_style.box_values.min_width.is_auto()
                && used_style
                    .aspect_ratio
                    .preferred_ratio_for_non_replaced(false)
                    .is_some());
        let intrinsic_sizes = if needs_intrinsic_sizes {
            // A block's definite preferred height is already known while its
            // automatic inline size queries descendant intrinsic widths. Make
            // that value visible as the descendant percentage basis for this
            // one measurement. Otherwise a `height: 100%` balanced column
            // flex container sees only the fragmentainer's provisional
            // height, forms one intrinsic column, and leaves its final
            // balanced columns outside the parent's max-content width.
            //
            // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
            // <https://drafts.csswg.org/css-flexbox-2/#algo-balance>
            let descendant_percentage_basis = explicit_content_height
                .map(|height| {
                    PercentageBasis::definite_from(
                        content_box_pt(height),
                        BlockSizeBasisSource::ContainingBlock,
                    )
                })
                .or_else(|| {
                    // This mirrors browser quirks mode's legacy propagation
                    // through auto-height block wrappers. It is deliberately
                    // restricted to intrinsic measurement for a quirks
                    // document, not the standards-mode percentage algorithm.
                    (element.document_compatibility_mode == dom::DocumentCompatibilityMode::Quirks
                        && containing_block_content_height.is_definite())
                    .then_some(containing_block_content_height)
                });
            let pushed_definite_height = descendant_percentage_basis.map(|basis| {
                self.block_percentage_context_stack
                    .push_percentage_basis(basis);
            });
            let sizes = self.block_intrinsic_physical_widths(
                element,
                &used_style,
                stylesheets,
                child_boxes,
                available_outer_width.points(),
            );
            if pushed_definite_height.is_some() {
                self.block_percentage_context_stack.pop();
            }
            Some(sizes)
        } else {
            None
        };
        // A principal vertical flow can constrain a direct child to its
        // remaining physical block track. That is an available placement
        // interval, not the used logical block-size of a vertical
        // `width:auto` child: CSS block-size auto sizing remains
        // content-derived unless this box owns the document canvas.
        // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
        let vertical_non_canvas_auto_block_size = used_style.writing_mode.has_vertical_lines()
            && used_style.box_values.width.is_auto()
            && !element.tag.eq_ignore_ascii_case("html")
            && !self.principal_flow.is_source_body(element);
        let width_resolution = if let Some(auto_border_box_width) = constraint
            .auto_border_box_width
            .filter(|_| has_auto_width(&used_style) && !vertical_non_canvas_auto_block_size)
        {
            ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(content_box_pt(
                (auto_border_box_width.points() - horizontal_extras.points()).max(0.0),
            )))
        } else if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) = intrinsic_sizes;
            ResolvedBlockPhysicalContentWidth::ordinary(PhysicalContentWidth::new(
                crate::layout::intrinsic::content_box_width_from_intrinsic(
                    &used_style,
                    intrinsic_width_percentage_basis,
                    horizontal_extras,
                    min_content,
                    max_content,
                    crate::layout::intrinsic::IntrinsicAutoWidth::FillAvailable,
                ),
            ))
        } else {
            self.resolved_block_physical_content_width(
                element,
                &used_style,
                stylesheets,
                child_boxes,
                BlockContentWidthInputs {
                    available_outer_width,
                    percentage_basis: PercentageBasis::definite(layout_pt(
                        physical_width_percentage_basis,
                    )),
                    horizontal_non_content: horizontal_extras,
                    definite_content_height: explicit_content_height
                        .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                    auto_width_role: BlockAutoWidthRole::NormalFlow,
                },
            )
        };
        let selected_logical_inline_size = width_resolution.selected_logical_inline_size;
        let selected_orthogonal_inline_layout = width_resolution.selected_orthogonal_inline_layout;
        let requested_content_width = width_resolution.content_width;
        let requested_content_width = if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) = intrinsic_sizes;
            PhysicalContentWidth::new(constrain_width_with_intrinsic(
                &used_style,
                requested_content_width.content_box_length(),
                min_content,
                max_content,
                PercentageBasis::definite(content_box_pt(
                    intrinsic_width_percentage_basis.points(),
                )),
                horizontal_extras,
            ))
        } else {
            requested_content_width
        };
        let requested_content_width = explicit_content_height
            .and_then(|height| {
                non_replaced_aspect_ratio_content_width(
                    &used_style,
                    height,
                    horizontal_extras.points(),
                    vertical_extras.points(),
                )
            })
            .map(|width| {
                // CSS Sizing's automatic content-based minimum does not
                // apply to the ratio-dependent inline axis of a scroll
                // container. The transferred preferred width still applies,
                // but overflow owns excess inline content instead.
                // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
                let automatic_minimum = if !used_style.overflow_x.is_scrollable() {
                    intrinsic_sizes
                        .map(|sizes| {
                            let (min_content, max_content) = sizes;
                            intrinsic_width_constraint(
                                used_style.box_values.min_width.clone(),
                                used_style.box_sizing,
                                PercentageBasis::definite(content_box_pt(containing_inline_size)),
                                horizontal_extras,
                                min_content,
                                max_content,
                            )
                            .unwrap_or(min_content)
                            .points()
                        })
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                PhysicalContentWidth::new(content_box_pt(width.max(automatic_minimum)))
            })
            .unwrap_or(requested_content_width);
        let mut width = resolve_normal_flow_block_inline_geometry(
            &mut used_style,
            containing_inline_span,
            requested_content_width,
            horizontal_extras,
            self.containing_block_direction,
            true,
        );
        // Sizing uses the child's logical axes, but static placement uses the
        // containing block's axes. This is significant for an orthogonal
        // vertical child of a horizontal block: its physical width is still
        // placed by the horizontal containing block's width equation, not at
        // the child's physical block-start edge.
        // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-flows>
        let root_pseudo_projection = self
            .root_pseudo_block_projection
            .filter(|projection| projection.element == element.id);
        if !matches!(used_style.position, Position::Absolute | Position::Fixed) {
            width.border_box_inline_span = normal_flow_border_box_inline_span_in_containing_axes(
                FlowAxes::new(
                    self.containing_block_writing_mode,
                    self.containing_block_direction,
                ),
                containing_inline_span,
                width.border_box_inline_span,
                used_style.margin.left,
                used_style.margin.right,
                root_pseudo_projection,
            );
        }
        let mut content_width = width.content_width;
        let mut content_width_points = content_width.points();
        let mut unconstrained_aspect_height = None;
        let mut definite_content_height = (!height_depends_on_intrinsic_content
            || aspect_ratio_resolves_auto_basis_height)
            .then(|| {
                explicit_content_height.or_else(|| {
                    non_replaced_aspect_ratio_content_height(
                        &used_style,
                        content_width_points,
                        horizontal_extras.points(),
                        vertical_extras.points(),
                    )
                })
            })
            .flatten()
            .map(|height| {
                unconstrained_aspect_height = Some(height);
                if containing_block_percentage_context.is_definite() {
                    constrain_height_with_stretch_fit(
                        &used_style,
                        content_box_pt(height),
                        containing_block_stretch_height,
                        layout_pt(used_style.margin.top + used_style.margin.bottom),
                        vertical_extras,
                    )
                } else {
                    constrain_non_replaced_content_height(
                        &used_style,
                        content_box_pt(height),
                        containing_block_percentage_context,
                    )
                }
                .points()
            });
        // A min/max block-size constraint can change the auto axis selected
        // through the preferred aspect ratio. Re-resolve the dependent inline
        // axis through the normal block-width equation, so its own min/max
        // constraints and auto margins remain authoritative.
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
        if unconstrained_aspect_height.is_some_and(|unconstrained| {
            definite_content_height
                .is_some_and(|constrained| (constrained - unconstrained).abs() > 0.01)
        }) && (has_auto_width(&used_style)
            || needs_intrinsic_width_contribution(used_style.box_values.width.clone()))
            && let Some(height) = definite_content_height
            && let Some(transferred_width) = non_replaced_aspect_ratio_content_width(
                &used_style,
                height,
                horizontal_extras.points(),
                vertical_extras.points(),
            )
        {
            width = resolve_normal_flow_block_inline_geometry(
                &mut used_style,
                containing_inline_span,
                PhysicalContentWidth::new(content_box_pt(transferred_width)),
                horizontal_extras,
                self.containing_block_direction,
                true,
            );
            content_width = width.content_width;
            content_width_points = content_width.points();
            definite_content_height = non_replaced_aspect_ratio_content_height(
                &used_style,
                content_width_points,
                horizontal_extras.points(),
                vertical_extras.points(),
            )
            .map(|height| {
                constrain_height_with_stretch_fit(
                    &used_style,
                    content_box_pt(height),
                    containing_block_stretch_height,
                    layout_pt(used_style.margin.top + used_style.margin.bottom),
                    vertical_extras,
                )
                .points()
            });
        }
        let definite_content_height = definite_content_height.map(|height| {
            DefinitePhysicalContentHeight::new(PhysicalContentHeight::new(content_box_pt(height)))
        });
        let content_logical_inline_size = selected_logical_inline_size
            .filter(|_| definite_content_height.is_none())
            .unwrap_or_else(|| {
                self.block_content_logical_inline_size(
                    element,
                    &used_style,
                    stylesheets,
                    child_boxes,
                    PhysicalContentWidth::new(content_width),
                    definite_content_height.map(DefinitePhysicalContentHeight::value),
                )
            });
        let outer_inline_span = PageInlineSpan::new(
            width.border_box_inline_span.left_x() + relative_offset.x(),
            width.border_box_inline_span.width(),
        );
        let inner_x =
            outer_inline_span.left_x() + border_edges.left.points() + used_style.padding.left;
        let content_inline_span = PageInlineSpan::new(inner_x, content_width.points());
        BlockLayoutGeometry {
            style: used_style,
            relative_offset,
            border_edges,
            vertical_non_content: vertical_extras,
            containing_block_content_height,
            containing_block_percentage_context,
            definite_content_height,
            content_logical_inline_size,
            selected_orthogonal_inline_layout,
            outer_inline: BlockBorderBoxInlineBounds::new(outer_inline_span),
            content_inline: BlockContentBoxInlineBounds::new(content_inline_span),
        }
    }

    /// Resolve the logical inline content size used by this block's inline layout.
    ///
    /// CSS Writing Modes defines orthogonal-flow auto inline sizing as a
    /// fit-content calculation against the containing block's available size.
    /// In vertical writing modes the logical inline axis is the physical
    /// height, so normal block layout must not reuse the physical content
    /// width as the text wrapping measure:
    /// <https://www.w3.org/TR/css-writing-modes-3/#orthogonal-auto>.
    pub(in crate::layout) fn block_content_logical_inline_size(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        content_width: PhysicalContentWidth,
        definite_content_height: Option<PhysicalContentHeight>,
    ) -> LogicalInlineContentSize {
        let points = if WritingModeAxes::new(style.writing_mode, style.direction)
            .swaps_physical_axes()
        {
            let containing_space = self.current_child_available_space();
            if let Some(definite_content_height) = definite_content_height {
                return LogicalInlineContentSize::new(definite_content_height.content_box_length());
            }
            if !writing_modes_are_orthogonal(containing_space.writing_mode, style.writing_mode) {
                // `height` is the logical inline size in vertical writing.
                // An auto-sized normal-flow block stretches through its
                // containing block's available inline size just as a
                // horizontal block's auto width does.
                // https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping
                let available = containing_space.logical_inline_size_for(style.writing_mode);
                let percentage_basis = containing_space
                    .logical_inline_percentage_basis_for(style.writing_mode)
                    .map_value(LogicalInlineContentSize::content_box_length);
                return LogicalInlineContentSize::new(constrain_content_height(
                    style,
                    available.content_box_length(),
                    percentage_basis,
                ));
            }
            // The orthogonal available size is the containing block's
            // available *outer* inline size.  Fit-content line layout needs
            // this box's content inline size, so remove its physical
            // top/bottom margin, padding, and border before choosing the
            // line-fitting measure.  Treating the available outer measure as
            // content-box space makes a constrained parent overflow by this
            // amount in nested orthogonal flows.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
            // <https://drafts.csswg.org/css-sizing-3/#fit-content-size>
            let borders = used_border_widths(style);
            let logical_inline_outer_non_content = style.margin.top
                + style.padding.top
                + borders.top
                + style.padding.bottom
                + borders.bottom
                + style.margin.bottom;
            let containing_stretch_fit = (LogicalInlineContentSize::new(
                containing_space
                    .orthogonal_inline_measure()
                    .value()
                    .content_box_length(),
            )
            .points()
                - logical_inline_outer_non_content)
                .max(0.0);
            // The box's own physical block-size constraints map to the
            // logical inline axis in vertical writing. They therefore bound
            // the fit-content measure before its inline contents are laid
            // out. Constraining only the final physical height would let a
            // vertical block first lay out against the ICB and then clip that
            // unwrapped line, instead of reflowing it into the constrained
            // inline measure.
            // <https://www.w3.org/TR/css-writing-modes-4/#dimension-mapping>
            // <https://drafts.csswg.org/css-sizing-3/#fit-content-size>
            let own_inline_constraint = orthogonal_fallback_physical_content_height(
                style,
                containing_space
                    .physical_height_percentage_basis()
                    .map_value(|height| height.content_box_length()),
            )
            .map(PhysicalContentHeight::points);
            let stretch_fit = own_inline_constraint
                .map(|constraint| containing_stretch_fit.min(constraint))
                .unwrap_or(containing_stretch_fit);
            // DOM-backed blocks normally defer formatting-box construction to
            // final layout. Orthogonal fit-content sizing needs the same
            // atomic-inline classification as that final pass, however: a
            // durable table box records table structure independently of its
            // outer `display`, and a raw DOM probe would otherwise lose an
            // `inline-table` entirely.
            // <https://www.w3.org/TR/css-writing-modes-4/#orthogonal-auto>
            // <https://drafts.csswg.org/css-display-3/#valdef-display-inline-table>
            let owned_child_boxes = child_boxes.is_none().then(|| {
                self.build_frozen_child_boxes_with_current_ancestors(element, stylesheets, style)
            });
            let child_boxes = child_boxes.or(owned_child_boxes.as_deref());
            let (min_content, max_content) =
                if child_boxes.is_some_and(has_non_inline_formatting_box) {
                    // Inline collection deliberately omits block children. For a
                    // vertical flow root its logical inline measure is physical
                    // height, so a nested horizontal block must be measured by
                    // the block intrinsic model before fit-content negotiation.
                    // <https://www.w3.org/TR/css-sizing-3/#intrinsic>
                    let (min_inline, max_inline) = self.block_intrinsic_content_inline_sizes(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                        content_width.points(),
                    );
                    (min_inline.points(), max_inline.points())
                } else {
                    let contribution = self.intrinsic_inline_contribution_for_element(
                        element,
                        style,
                        stylesheets,
                        child_boxes,
                    );
                    (
                        contribution.min_content.points(),
                        contribution.max_content.points(),
                    )
                };
            max_content.min(min_content.max(stretch_fit)).max(1.0)
        } else {
            content_width.points().max(1.0)
        };
        LogicalInlineContentSize::new(content_box_pt(points))
    }
}
