use super::*;
use crate::layout::builder::{page_for_context, positioned_layer_fragment};
use crate::layout::flex::compute::effective_align_self;

/// Convert Flexbox's main-axis content alignment into the equivalent
/// self-alignment used to retain an abspos child's static-position rectangle.
/// Distributed values use the one-item fallback required by Flexbox.
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
/// <https://drafts.csswg.org/css-align-3/#distribution-fallback>
fn flex_static_main_alignment(alignment: css::ContentAlignment) -> css::SelfAlignment {
    let keyword = match alignment.keyword {
        css::ContentAlignmentKeyword::Normal
        | css::ContentAlignmentKeyword::Start
        | css::ContentAlignmentKeyword::FlexStart
        | css::ContentAlignmentKeyword::Stretch
        | css::ContentAlignmentKeyword::SpaceBetween
        | css::ContentAlignmentKeyword::Baseline
        | css::ContentAlignmentKeyword::LastBaseline => SelfAlignmentKeyword::Start,
        css::ContentAlignmentKeyword::End | css::ContentAlignmentKeyword::FlexEnd => {
            SelfAlignmentKeyword::End
        }
        css::ContentAlignmentKeyword::Left => SelfAlignmentKeyword::Left,
        css::ContentAlignmentKeyword::Right => SelfAlignmentKeyword::Right,
        css::ContentAlignmentKeyword::Center
        | css::ContentAlignmentKeyword::SpaceAround
        | css::ContentAlignmentKeyword::SpaceEvenly => SelfAlignmentKeyword::Center,
    };
    css::SelfAlignment {
        keyword,
        safety: alignment.safety,
    }
}

/// Record the visible block-end of one committed nested table fragment.
///
/// Flex owns the outer source range, but a table's repeated chrome can make
/// its final child fragment shorter than that range. The following flex sibling
/// resumes after the visible child fragment rather than after unused scratch
/// fragmentainer capacity.
/// <https://www.w3.org/TR/css-break-3/#box-splitting>
fn record_table_replay_fragment_bottom(
    bottoms: &mut Vec<Option<f32>>,
    fragmentainer_index: usize,
    fragment: &PaintFragment,
) {
    let Some(bounds) = fragment.bounds() else {
        return;
    };
    if bottoms.len() <= fragmentainer_index {
        bottoms.resize(fragmentainer_index + 1, None);
    }
    bottoms[fragmentainer_index] = Some(
        bottoms[fragmentainer_index]
            .map(|bottom| bottom.min(bounds.y()))
            .unwrap_or_else(|| bounds.y()),
    );
}

/// Mutable continuation state owned by one split flex item.
///
/// The source fragment sequence, its local layout end, and the target-page
/// cursor it contributes are one replay transaction. Keeping them together
/// prevents a caller from advancing sibling placement without retaining the
/// matching source fragment.
pub(in crate::layout::flex) struct SplitFlexItemReplayState<'a> {
    pub(in crate::layout::flex) fragments: &'a mut Vec<PaintFragment>,
    pub(in crate::layout::flex) local_block_ends: &'a mut Vec<Option<f32>>,
    pub(in crate::layout::flex) table_fragment_bottoms: &'a mut Vec<Option<f32>>,
    pub(in crate::layout::flex) destination_block_end: &'a mut Option<f32>,
}

impl<'a> LayoutBuilder<'a> {
    /// Build an atomic inline fragment for an `inline-flex` container.
    ///
    /// CSS Display makes `inline-flex` an inline-level atomic flex container,
    /// while CSS Flexbox defines both its flex item layout and the baseline it
    /// contributes to the parent inline formatting context:
    /// <https://www.w3.org/TR/css-display-3/#the-display-properties> and
    /// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines>.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::layout) fn inline_flex_atom_for_element(
        &mut self,
        element: &Element,
        signature: &ElementSignature,
        style: &ComputedStyle,
        child_boxes: &[box_tree::FormattingBox<'_>],
        stylesheets: &Stylesheets<'_>,
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> InlineAtom {
        let containment = used_property_containment(element, style);
        let available_outer_width = layout_pt(
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size),
        );
        let mut used_style =
            FlexUsedStyle::from_normalized(self.style_with_current_used_lengths(style));
        let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            self.current_content_logical_inline_percentage_basis(),
        );
        let style = &used_style;
        let border_widths = box_metrics.border.to_css_edges();
        let horizontal_non_content = box_metrics.horizontal_non_content_length();
        let vertical_non_content = box_metrics.vertical_non_content_length();
        let (mut children, mut positioned_children) =
            flex_child_lists_from_boxes(element, signature, style, child_boxes);
        self.resolve_styled_children_used_lengths(&mut children);
        self.resolve_styled_children_used_lengths(&mut positioned_children);

        let height_percentage_basis = self.flex_container_height_percentage_basis();
        let intrinsic_content_height = used_content_box_height_or_auto_with_basis(
            style,
            height_percentage_basis,
            vertical_non_content,
        );
        // Size containment substitutes the intrinsic sizes of empty content
        // for the principal flex box. The real flex layout still runs below so
        // descendants paint, overflow, and contribute the exported baseline.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        let intrinsic = if containment.size {
            FlexItemEstimate::fixed(
                PhysicalContentWidth::new(content_box_pt(0.0)),
                PhysicalContentHeight::new(content_box_pt(0.0)),
            )
        } else {
            self.estimate_intrinsic_flex_container_size(
                &children,
                style,
                stylesheets,
                FlexAvailableSpace {
                    width: PhysicalContentWidth::new(content_box_pt(
                        available_outer_width.points().max(0.0),
                    )),
                    width_basis: flex_available_percentage_basis_from_points(
                        used_content_box_width_or_auto(
                            style,
                            layout_pt(available_outer_width.points().max(0.0)),
                            horizontal_non_content,
                        )
                        .map(|_| available_outer_width.points().max(0.0)),
                        FlexAvailableSizeSource::IntrinsicContainerSize,
                    ),
                    height: intrinsic_content_height.map(PhysicalContentHeight::new),
                    height_basis: intrinsic_content_height
                        .map(|height| {
                            PercentageBasis::definite_from(
                                height,
                                FlexAvailableSizeSource::IntrinsicContainerSize,
                            )
                        })
                        .unwrap_or_else(PercentageBasis::indefinite),
                },
            )
        };
        let requested_content_width = flex_container_content_width_from_intrinsic(
            style,
            available_outer_width,
            horizontal_non_content,
            intrinsic,
            true,
        );
        let content_width = PhysicalContentWidth::new(constrain_content_width(
            style,
            requested_content_width.content_box_length(),
            PercentageBasis::definite(available_outer_width),
        ));
        let content_width_points = content_width.points();

        // `content` replaces an element's children before flex itemization.
        // A generated inline-flex pseudo therefore owns one anonymous flex
        // item even though its durable box-tree child list is empty. Preserve
        // that generated item and its visible overflow, including the
        // zero-width row-reversed construction used by outside marker boxes.
        // <https://www.w3.org/TR/css-content-3/#content-property>
        // <https://www.w3.org/TR/css-flexbox-1/#flex-items>
        if style.content.is_generated() && child_boxes.is_empty() {
            let mut items = Vec::new();
            self.push_element_content_items_from_boxes(
                element,
                style,
                box_tree::CounterEventSource::Principal,
                child_boxes,
                stylesheets,
                link_target.clone(),
                0.0,
                InlineVisualOffset::zero(),
                style,
                style.text_decoration_layers.clone(),
                &mut items,
            );
            let measurement =
                self.intrinsic_inline_measurement_for_items(items.clone(), style, f32::MAX);
            let natural_width = measurement.contribution.max_content.points();
            let sequence = self.collect_inline_line_sequence_with_text_box_trim(
                items,
                style,
                natural_width,
                0.0,
                0.0,
            );
            let content_height = sequence.total_height().max(style.line_height);
            let border_box_height = content_height + vertical_non_content.points();
            let line_baseline_offset =
                self.inline_box_sequence_baseline_offset(&sequence, style, border_widths);
            let baseline_offset = Self::inline_block_baseline_offset(
                style,
                containment.layout,
                border_box_height,
                line_baseline_offset,
            );
            let overflow_offset = if style.flex_direction == FlexDirection::RowReverse
                && style.direction == Direction::Ltr
            {
                content_width_points - natural_width
            } else {
                0.0
            };
            return InlineAtom::new(
                InlineAtomContent::InlineBox { sequence },
                style.as_computed().clone(),
                None,
                InlineSize::new(
                    content_width_points
                        + horizontal_non_content.points()
                        + style.margin.left
                        + style.margin.right,
                    border_box_height + style.margin.top + style.margin.bottom,
                ),
                baseline_offset,
                baseline_shift,
                link_target,
                None,
            )
            .with_content_inline_offset(overflow_offset)
            .with_content_inline_paint_width(natural_width);
        }

        let height_constraint_basis = height_percentage_basis
            .value()
            .unwrap_or_else(|| content_box_pt(available_outer_width.points()));
        let explicit_content_height = used_content_box_height_or_auto_with_basis(
            style,
            height_percentage_basis,
            vertical_non_content,
        )
        .map(|height| {
            constrain_content_height(
                style,
                height,
                PercentageBasis::definite_from(
                    height_constraint_basis,
                    BlockSizeBasisSource::ContainingBlock,
                ),
            )
        });
        let definite_content_height = definite_flex_container_content_height(
            style,
            explicit_content_height,
            content_width.content_box_length(),
            PercentageBasis::definite_from(
                height_constraint_basis,
                BlockSizeBasisSource::ContainingBlock,
            ),
            horizontal_non_content,
            vertical_non_content,
        );
        let flex_available_content_height = flex_available_content_height(
            style,
            definite_content_height,
            PercentageBasis::definite_from(
                content_width.content_box_length(),
                BlockSizeBasisSource::ContainingBlock,
            ),
        );

        let flex_available = FlexAvailableSpace {
            width: content_width,
            width_basis: flex_available_percentage_basis_from_points(
                Some(content_width_points),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: flex_available_content_height.map(PhysicalContentHeight::new),
            height_basis: definite_content_height
                .map(|height| {
                    PercentageBasis::definite_from(height, FlexAvailableSizeSource::ContainingBlock)
                })
                .unwrap_or_else(PercentageBasis::indefinite),
        };
        let Some(flex_layout) =
            self.compute_flex_layout(&children, style, stylesheets, flex_available)
        else {
            return self.inline_fragment_atom_for_children(
                None,
                style,
                child_boxes,
                stylesheets,
                baseline_shift,
                link_target,
            );
        };

        let total_content_height = if containment.size {
            definite_content_height.unwrap_or_else(|| {
                constrain_content_height(
                    style,
                    content_box_pt(0.0),
                    PercentageBasis::definite_from(
                        height_constraint_basis,
                        BlockSizeBasisSource::ContainingBlock,
                    ),
                )
            })
        } else {
            constrain_content_height(
                style,
                flex_layout.height.content_box_length(),
                PercentageBasis::definite_from(
                    content_width.content_box_length(),
                    BlockSizeBasisSource::ContainingBlock,
                ),
            )
        }
        .points();
        debug_assert!(!flex_layout.lines.is_empty() || children.is_empty());
        debug_assert!(
            flex_layout.fragment_plan.is_empty()
                || flex_layout.fragment_plan.planned_item_fragment_count()
                    <= flex_layout.items.len()
        );
        let border_box_height = content_box_to_border_box_length(
            content_box_pt(total_content_height),
            vertical_non_content,
        );
        // Inline-fragment and paint APIs are legacy scalar boundaries. Keep
        // the border-box conversion above typed until this projection.
        let border_box_height_points = border_box_height.points();
        let estimated_baseline_offset = (!containment.layout)
            .then(|| inline_flex_exported_vertical_baseline(flex_layout.baselines))
            .flatten()
            .map(|baseline| border_widths.top + style.padding.top + baseline.points())
            .unwrap_or_else(|| {
                inline_flex_synthesized_baseline_offset(style, border_box_height).points()
            });
        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let top = 10_000.0;
        let content_top = top - border_widths.top - style.padding.top;
        let inner_x = border_widths.left + style.padding.left;
        let inner_width = content_width_points;
        self.current_page = Page::new(content_width_points + horizontal_non_content.points(), top);
        self.content_left = inner_x;
        self.content_right = inner_x + inner_width;
        self.cursor_y = content_top;
        self.truncate_page_start_margins = false;

        let positioning_containing_block_mode = PositionedContainingBlockMode::for_style(style);
        let positioned_containing_block_scope =
            if let Some(mode) = positioning_containing_block_mode {
                let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
                    border_widths.left,
                    top - border_widths.top,
                    content_width_points + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                ));
                Some(self.push_positioned_containing_block(mode, containing_block))
            } else {
                None
            };

        for (index, child) in children.iter().enumerate() {
            if flex_item_is_collapsed(&child.style) {
                continue;
            }
            let item = &flex_layout.items[index];
            let replay_dimensions = item.replay_dimensions();

            let mut replay_child_style = child.style.clone();
            freeze_replayed_item_padding(
                &mut replay_child_style,
                flex_item_used_padding(&child.style, style, flex_available),
            );
            let placed_style = placed_flex_item_style(
                &replay_child_style,
                replay_dimensions.border_box_width(),
                replay_dimensions.border_box_height(),
                PhysicalFlexDirection::new(physical_flex_direction(style)),
            );
            self.with_formatting_context_item_placement(
                FormattingContextItemPlacement {
                    content_left: inner_x + item.x().points(),
                    content_width: replay_dimensions.available_width_for_replay(),
                    content_height: Some(replay_dimensions.available_height_for_replay()),
                    table_wrapper_border_box_block_size: auto_table_wrapper_block_size_override(
                        &child.style,
                        replay_dimensions.border_box_height(),
                    ),
                    writing_mode: placed_style.writing_mode,
                    scope_content_logical_inline_size: child.anonymous_content().is_some(),
                    cursor_y: content_top - item.y().points(),
                    page_start_margin_policy: PageStartMarginPolicy::Suppress,
                },
                &placed_style,
                |layout| {
                    layout.layout_flex_item_contents(
                        child,
                        &placed_style,
                        stylesheets,
                        item.percentage_height_basis,
                    );
                },
            );
        }

        for child in &positioned_children {
            self.layout_positioned_flex_child(
                child,
                PositionedFlexStaticContext {
                    container_style: style,
                    stylesheets,
                    available: FlexAvailableSpace {
                        width: PhysicalContentWidth::new(content_box_pt(inner_width)),
                        width_basis: flex_available_percentage_basis_from_points(
                            Some(inner_width),
                            FlexAvailableSizeSource::ContainingBlock,
                        ),
                        height: flex_available_content_height.map(PhysicalContentHeight::new),
                        height_basis: definite_content_height
                            .map(|height| {
                                PercentageBasis::definite_from(
                                    height,
                                    FlexAvailableSizeSource::ContainingBlock,
                                )
                            })
                            .unwrap_or_else(PercentageBasis::indefinite),
                    },
                    inner_inline_span: PageInlineSpan::new(inner_x, inner_width),
                    content_height: PhysicalContentHeight::new(content_box_pt(
                        total_content_height,
                    )),
                    content_top: PageTopBlockPosition::new(content_top),
                    source_fragment_block_offset: FlexFragmentBlockOffset::new(0.0),
                    first_fragment_source_block_size: FlexFragmentBlockSize::new(0.0),
                },
            );
        }

        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        let border_bottom = top - border_box_height_points;
        self.flush_positioned_layers_since(positioned_layer_start);
        let mut fragment = self.current_page.paint_fragment();
        if style.visibility == Visibility::Visible {
            let gap_gutters = flex_gap_decoration_gutters(
                &flex_layout,
                style,
                PhysicalContentWidth::new(content_box_pt(inner_width)),
                PhysicalContentHeight::new(content_box_pt(total_content_height)),
            );
            fragment.prepend_primitives_in_band(
                PaintBand::BackgroundBorder,
                flex_gap_decoration_primitives_with_gutters(
                    style,
                    GapDecorationContainer::new(
                        inner_x,
                        content_top,
                        inner_width,
                        total_content_height,
                    ),
                    &flex_gap_decoration_items(&flex_layout),
                    &gap_gutters,
                ),
            );
        }
        let fragment = fragment.translated(PaintTranslation::new(0.0, -border_bottom));
        // The captured paint fragment is not a baseline-selection input.
        // Flexbox exports the resolved flex-line baseline, or synthesizes one
        // from the container border box when none is parallel to the parent
        // inline axis. A captured first line may belong to a later wrapped
        // flex line and therefore cannot repair missing flex metadata.
        // <https://drafts.csswg.org/css-flexbox/#flex-baselines>
        let baseline_offset = estimated_baseline_offset;
        self.restore(snapshot);

        InlineAtom::new(
            InlineAtomContent::InlineFragment {
                fragment: Box::new(fragment),
                table_cell_context: None,
            },
            style.as_computed().clone(),
            None,
            InlineSize::new(
                content_width_points
                    + horizontal_non_content.points()
                    + style.margin.left
                    + style.margin.right,
                border_box_height_points + style.margin.top + style.margin.bottom,
            ),
            baseline_offset,
            baseline_shift,
            link_target,
            None,
        )
    }

    /// Lays out an absolutely positioned flex child from its flex static position.
    ///
    /// CSS Flexbox says an absolutely positioned child of a flex container does
    /// not participate in flex layout, but its static-position rectangle is
    /// derived from where it would be positioned as the sole flex item:
    /// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>.
    pub(in crate::layout::flex) fn layout_positioned_flex_child(
        &mut self,
        child: &StyledChild<'_>,
        context: PositionedFlexStaticContext<'_>,
    ) {
        let (static_rect, source_static_block_interval) =
            self.positioned_flex_child_static_rect(child, &context);
        if self.multicol_positioned_replay_capture_depth > 0 {
            // Only a physical-column flex progression encodes the
            // hypothetical item's main-axis static position in the same
            // physical-Y source coordinate that multicolumn fragmentation
            // slices. A physical row's static Y is cross-axis geometry and
            // already belongs to the local source fragmentainer.
            // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
            // <https://www.w3.org/TR/css-flexbox-1/#pagination>
            let positioning_containing_block =
                PositionedContainingBlockMode::for_style(context.container_style)
                    .zip(self.containing_blocks.last().copied());
            let fragment =
                PositionedFragmentReplay::unfragmented(static_rect, positioning_containing_block);
            // A physical-column flex child has a main-axis static position
            // in the same source block coordinate as the materialized flex
            // fragments, so its owner is known at capture time. For a
            // physical row, the static rectangle only describes cross-axis
            // placement; a definite inset can select a different source
            // fragment. Leave that record unresolved until positioned layout
            // has its final geometry instead of guessing the last temporary
            // multicolumn fragmentainer.
            // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            let physical_direction = physical_flex_direction(context.container_style);
            let final_block_inset_from_start = positioning_containing_block
                .and_then(|(_, containing_block)| used_inset_top(&child.style, containing_block))
                .or_else(|| child.style.box_values.inset_top.length_if_no_percent())
                .map(layout_pt);
            let final_block_inset_starts_later_fragment =
                final_block_inset_from_start.is_some_and(|inset| {
                    inset.points() > context.first_fragment_source_block_size.points() - 0.01
                });
            let fragment = if physical_direction.is_column_axis() {
                // Candidate selection must use the resolved physical block
                // interval when a definite inset moves the box away from its
                // hypothetical flex static position. The static rectangle
                // remains the positioned-layout fallback, but its source
                // interval no longer describes the painted box.
                // <https://www.w3.org/TR/css-position-3/#inset-properties>
                // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
                let source_block_interval = final_block_inset_from_start
                    .map(|inset| {
                        let source_size =
                            source_static_block_interval.1 - source_static_block_interval.0;
                        (inset, inset + source_size)
                    })
                    .unwrap_or(source_static_block_interval);
                let fragment = fragment
                    .with_source_fragment_block_offset(layout_pt(
                        context.source_fragment_block_offset.points(),
                    ))
                    .resolving_owner_from_source_block_interval(
                        source_block_interval.0,
                        source_block_interval.1,
                    );
                if final_block_inset_from_start.is_some() {
                    fragment.with_definite_block_inset_source_coordinates()
                } else {
                    fragment
                }
            } else if final_block_inset_starts_later_fragment {
                fragment.resolving_owner_from_final_block_inset(final_block_inset_from_start)
            } else {
                fragment
            };
            self.defer_multicol_positioned_fragment_child(child, fragment);
            return;
        }
        self.layout_positioned_formatting_context_child(child, context.stylesheets, static_rect);
    }

    /// Compute a flex positioned child's static rectangle before choosing
    /// whether normal positioned layout happens immediately or is deferred by
    /// an enclosing temporary multicolumn fragmentainer sequence.
    fn positioned_flex_child_static_rect(
        &mut self,
        child: &StyledChild<'_>,
        context: &PositionedFlexStaticContext<'_>,
    ) -> (PositionedChildStaticRect, (LayoutLength, LayoutLength)) {
        let mut hypothetical_child = child.clone();
        hypothetical_child.style.position = Position::Static;
        hypothetical_child.style.flex_grow = 0.0;
        hypothetical_child.style.flex_shrink = 0.0;
        hypothetical_child.style.flex_basis = css::ComputedFlexBasis::Auto;
        zero_auto_margins_for_static_flex_probe(&mut hypothetical_child.style);
        if hypothetical_child.style.display.is_inline_level() {
            hypothetical_child.style.display = hypothetical_child.style.display.blockified();
        }
        let mut hypothetical_container_style = context.container_style.clone();
        resolve_static_flex_probe_justify_content(&mut hypothetical_container_style);
        let hypothetical = self
            .compute_flex_layout(
                std::slice::from_ref(&hypothetical_child),
                &hypothetical_container_style,
                context.stylesheets,
                context.available,
            )
            .and_then(|layout| layout.items.into_iter().next())
            .unwrap_or_else(|| {
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(context.inner_inline_span.width(), child.style.line_height),
                ))
            });

        // CSS Flexbox aligns the hypothetical sole item by its margin box.
        // The static-position rectangle must therefore preserve those outer
        // bounds; starting from the border box shifts an abspos child by its
        // own margins a second time when normal positioned layout resolves
        // the final margin box.
        // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
        let static_left = context.inner_inline_span.left_x() + hypothetical.x().points()
            - child.style.margin.left;
        let static_right = context.inner_inline_span.left_x()
            + hypothetical.x().points()
            + hypothetical.width().points()
            + child.style.margin.right;
        let static_top =
            context.content_top.points() - hypothetical.y().points() + child.style.margin.top;
        let flex_axes = FlexAxes::for_style(context.container_style);
        let main_alignment =
            flex_static_main_alignment(hypothetical_container_style.justify_content);
        let cross_alignment = effective_align_self(&child.style, context.container_style);
        let container_axes = WritingModeAxes::new(
            context.container_style.writing_mode,
            context.container_style.used_direction(),
        );
        let main_is_inline = if flex_axes.is_main_row_axis() {
            !container_axes.swaps_physical_axes()
        } else {
            container_axes.swaps_physical_axes()
        };
        let (inline_alignment, block_alignment) = if main_is_inline {
            (main_alignment, cross_alignment)
        } else {
            (cross_alignment, main_alignment)
        };
        let hypothetical_outer_height =
            (hypothetical.height().points() + child.style.margin.top + child.style.margin.bottom)
                .max(0.0);
        // Flexbox gives the static-position rectangle the container's content
        // edges in the cross axis, while the sole hypothetical item's margin
        // edges determine it in the main axis. This distinction is essential
        // when the eventual abspos size differs from the sizing probe.
        // <https://www.w3.org/TR/css-flexbox-1/#abspos-items>
        let static_area = if flex_axes.is_main_row_axis() {
            PageTopRect::new(
                static_left,
                context.content_top.points(),
                (static_right - static_left).max(0.0),
                context.content_height.points(),
            )
        } else {
            PageTopRect::new(
                context.inner_inline_span.left_x(),
                static_top,
                context.inner_inline_span.width(),
                hypothetical_outer_height,
            )
        };
        let static_alignment = AbsposStaticAlignment::new(
            static_area,
            context.container_style.writing_mode,
            context.container_style.used_direction(),
            inline_alignment,
            block_alignment,
        );
        let source_static_block_start = layout_pt(hypothetical.y().points().max(0.0));
        let source_static_block_end = layout_pt(
            (hypothetical.y().points()
                + hypothetical.height().points()
                + child.style.margin.top
                + child.style.margin.bottom)
                .max(source_static_block_start.points()),
        );
        (
            PositionedChildStaticRect::new(static_left, static_right, static_top)
                .with_static_alignment(static_alignment),
            (source_static_block_start, source_static_block_end),
        )
    }

    /// Replay a split flex item from its original item layout and clip the
    /// selected page-local slice.
    ///
    /// CSS Fragmentation slices the visual fragment but preserves the source
    /// box's internal layout for continuations:
    /// <https://www.w3.org/TR/css-break-3/#box-splitting>.
    pub(in crate::layout::flex) fn paint_split_flex_item_fragment(
        &mut self,
        child: &StyledChild<'_>,
        placed_style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        context: SplitFlexItemPaintContext,
        replay: SplitFlexItemReplayState<'_>,
    ) {
        let SplitFlexItemReplayState {
            fragments: table_replay_fragments,
            local_block_ends: replay_fragment_local_block_ends,
            table_fragment_bottoms: table_replay_fragment_bottoms,
            destination_block_end: replay_destination_block_end,
        } = replay;
        let slice_border_box = context.slice_border_box;
        let source_item_top = context.source_item_top.points();
        let table_replay = child.style.display.is_table();
        let child_fragment_replay = !context.has_descendant_source_overflow;
        // Flex has already resolved the table wrapper's cross size for the
        // source item.  That frozen size remains the outer flex geometry, but
        // it is not a definite CSS block-size for the table's own row
        // pagination.  Replaying it as one would turn its fragmentable body
        // into a fixed-height table and suppress repeated header state on the
        // continuation.  Keep the resolved inline size while returning the
        // table's block size to `auto` inside this isolated child replay.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        // <https://www.w3.org/TR/css-tables-3/#computing-the-table-height>
        let mut table_replay_style = placed_style.clone();
        if child_fragment_replay {
            table_replay_style
                .box_values
                .height
                .replace_with_used(css::ComputedLengthPercentageOrAuto::Auto);
        }
        let replay_style = if child_fragment_replay {
            &table_replay_style
        } else {
            placed_style
        };
        // The parent flex fragment has already committed the ordinary
        // item's used principal decoration at each materialized slice. This
        // source-canvas replay contributes only the item's formatting
        // context; allowing block layout to paint the same principal again
        // produces duplicate background edges on continuation pages.
        // Tables and replaced elements retain their dedicated decoration
        // replay paths.
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let replay_owns_principal_decoration = !table_replay
            && !child
                .element_parts()
                .is_some_and(|(element, _, _)| is_replaced_element(element));
        let replay_has_descendant_paint = child
            .element_parts()
            .is_none_or(|(_, _, children)| children.is_some_and(|children| !children.is_empty()));
        let table_first_capacity = context.continuation.first_fragmentainer_capacity.points();
        let table_continuation_capacity = context
            .continuation
            .continuation_fragmentainer_capacity
            .points();
        let source_canvas_slice_start = match context.replay_origin {
            // Visible descendant overflow belongs to one frozen item source
            // canvas. Every committed flex continuation must translate that
            // canvas by its own recorded source start, irrespective of the
            // flex main axis or whether the container wraps.
            FlexItemReplayOrigin::SourceSlice => {
                context.continuation.source_canvas_block_start.points()
            }
            // A child that fragments on its own is replayed from its
            // committed local fragment selected by continuation ordinal.
            FlexItemReplayOrigin::ChildFragment => 0.0,
        };
        if slice_border_box.height() <= 0.0 {
            return;
        }

        let table_fragment_ordinal = context.continuation.continuation_ordinal;
        // A child formatter owns its own pagination decisions. Once its first
        // flex slice has committed those child fragments, later flex slices
        // must replay the matching child fragment instead of laying out the
        // complete child tree again against a new scratch page. Re-running a
        // table loses its consumed row position and can duplicate or omit
        // rows/header chrome.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        if child_fragment_replay
            && let Some(fragment) = table_replay_fragments.get(table_fragment_ordinal)
        {
            let fragment = fragment
                .clone()
                .translated(PaintTranslation::new(
                    slice_border_box.x(),
                    // Each cached child fragment is already expressed in its
                    // own destination fragmentainer's page-local block
                    // coordinates. Reapplying the outer flex slice's block
                    // offset shifts the final partial fragment below its
                    // clip and drops nested overflow paint.
                    // <https://www.w3.org/TR/css-break-3/#box-splitting>
                    0.0,
                ))
                // A primitive clip would flatten nested stacking contexts and
                // lose effects such as descendant opacity. Keep the committed
                // child paint tree intact and apply the flex slice as an
                // overflow effect scope instead.
                // <https://www.w3.org/TR/css-break-3/#box-splitting>
                // <https://www.w3.org/TR/css-color-4/#transparency>
                .with_contents_effect_scoped_to_rect(slice_border_box);
            if table_replay {
                record_table_replay_fragment_bottom(
                    table_replay_fragment_bottoms,
                    context.continuation.fragmentainer_index,
                    &fragment,
                );
            }
            if !table_replay
                && let Some(Some(local_block_end)) =
                    replay_fragment_local_block_ends.get(table_fragment_ordinal)
            {
                *replay_destination_block_end = Some(slice_border_box.y() + *local_block_end);
            }
            if !replay_owns_principal_decoration || replay_has_descendant_paint {
                self.append_split_flex_item_replay(placed_style, slice_border_box, fragment);
            }
            return;
        }

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let offpage_top = if child_fragment_replay {
            table_first_capacity.max(1.0)
        } else {
            10_000.0
        };
        // A split-item replay uses a local off-page canvas. Its page context
        // must describe that same zero-inset coordinate system: retaining the
        // document page context causes a nested multicolumn replay to clip a
        // document-canvas inset and then apply that inset again on translation.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
        let replay_page_context = PageContext {
            size: PageSize::from_points(context.item_width.points().max(1.0), offpage_top),
            margins: PageMargins::all_points(0.0),
            edges: PageBoxEdges::ZERO,
            rotation: snapshot.current_page_context.rotation,
        };
        self.current_page = page_for_context(replay_page_context);
        self.current_page_context = replay_page_context;
        if child_fragment_replay {
            let continuation_context = PageContext {
                size: PageSize::from_points(
                    context.item_width.points().max(1.0),
                    table_continuation_capacity.max(1.0),
                ),
                margins: PageMargins::all_points(0.0),
                edges: PageBoxEdges::ZERO,
                rotation: snapshot.current_page_context.rotation,
            };
            self.fragmentainer_override = Some(FragmentainerOverride {
                kind: FragmentainerKind::Page,
                initial_context: replay_page_context,
                initial_fragmentainer_count: 1,
                context: continuation_context,
                relax_widows_orphans: false,
            });
        }
        self.overflow_clips.clear();
        self.fragment_top_offsets.clear();

        // Replay is laid out in an off-page coordinate system and translated
        // back to the selected source slice below. A positioned descendant of
        // a fragmented flex item must nevertheless resolve its insets against
        // the flex container's *fragment-local* containing block, rather than
        // against the original page coordinates retained by the outer layout.
        //
        // The transformed containing block keeps normal positioned layout
        // responsible for sizing and inset resolution; this function only
        // changes coordinate spaces before replay.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let replay_positioning_containing_block =
            context
                .positioning_containing_block
                .map(|containing_block| {
                    ContainingBlock::from_page_top_rect(PageTopRect::new(
                        containing_block.x() - slice_border_box.x(),
                        offpage_top + containing_block.top_y() - source_item_top
                            + source_canvas_slice_start,
                        containing_block.width(),
                        containing_block.height(),
                    ))
                });
        let replay_positioned_containing_block_scope =
            replay_positioning_containing_block.map(|containing_block| {
                self.push_positioned_containing_block(
                    if context.establishes_fixed_containing_block {
                        PositionedContainingBlockMode::FixedAndAbsolute
                    } else {
                        PositionedContainingBlockMode::AbsoluteOnly
                    },
                    containing_block,
                )
            });

        self.with_formatting_context_item_placement(
            FormattingContextItemPlacement {
                content_left: 0.0,
                content_width: context.available_width_for_replay(),
                content_height: Some(if child_fragment_replay {
                    PhysicalContentHeight::new(content_box_pt(table_first_capacity))
                } else {
                    context.available_height_for_replay()
                }),
                table_wrapper_border_box_block_size: (!table_replay)
                    .then(|| {
                        auto_table_wrapper_block_size_override(&child.style, context.item_height)
                    })
                    .flatten(),
                writing_mode: replay_style.writing_mode,
                scope_content_logical_inline_size: child.anonymous_content().is_some(),
                cursor_y: offpage_top,
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
            },
            replay_style,
            |layout| {
                let previous_suppress_principal_box_decoration =
                    layout.suppress_next_principal_box_decoration;
                if replay_owns_principal_decoration {
                    layout.suppress_next_principal_box_decoration = true;
                }
                if child_fragment_replay {
                    layout.layout_split_flex_item_continuation_contents(
                        child,
                        replay_style,
                        stylesheets,
                        context.percentage_height_basis,
                    );
                } else {
                    layout.layout_flex_item_contents(
                        child,
                        replay_style,
                        stylesheets,
                        context.percentage_height_basis,
                    );
                }
                layout.suppress_next_principal_box_decoration =
                    previous_suppress_principal_box_decoration;
            },
        );

        // A descendant-overflow replay lays the complete child tree once on
        // its off-page source canvas, then lets the flex fragment plan select
        // visible slices. Unlike an independently fragmenting child it does
        // not populate `replay_fragment_local_block_ends`, but its final
        // logical block end is still available from the scratch cursor. Export
        // it for the one destination slice that contains that end so the
        // automatic flex wrapper and following normal flow do not claim the
        // unused tail of the final page.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        // <https://www.w3.org/TR/css-flexbox-1/#pagination>
        if !table_replay && !child_fragment_replay {
            let source_end = offpage_top - self.cursor_y;
            let source_slice = context.continuation.source_content_slice;
            if source_end >= source_slice.block_start.points() - 0.01
                && source_end <= source_slice.block_end.points() + 0.01
            {
                *replay_destination_block_end = Some(
                    slice_border_box.y()
                        - (source_end - source_slice.block_start.points()).max(0.0),
                );
            }
        }

        if child_fragment_replay && table_replay_fragments.is_empty() {
            table_replay_fragments.extend(
                self.pages
                    .iter()
                    .chain(std::iter::once(&self.current_page))
                    .map(Page::paint_fragment),
            );
            replay_fragment_local_block_ends.resize(table_replay_fragments.len(), None);
            // Only the active local page exposes its final layout cursor.
            // Earlier pages may be fully occupied, but their exact cursor is
            // neither needed nor recoverable from paint bounds alone.
            if !table_replay && let Some(last) = replay_fragment_local_block_ends.last_mut() {
                *last = Some(self.cursor_y);
            }
        }

        // Positioned descendants whose containing block is the flex
        // container escape the item border box. Keep their layers separate
        // from the in-flow replay so they are clipped by the container's
        // fragment, not by a zero-sized or otherwise split item.
        // A nested speculative layout may restore a checkpoint taken before
        // this replay and therefore discard the replay's provisional layers.
        // That is a valid empty result, not an invalid layer checkpoint: only
        // layers still owned by this replay may be extracted.
        let mut escaped_positioned_layers = if positioned_layer_start < self.positioned_layers.len()
        {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        escaped_positioned_layers.sort_by_key(|layer| layer.stack_level.sort_key());

        if let Some(scope) = replay_positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }

        // A table's replay page is a real local fragmentainer. Its first
        // page starts at the first remaining capacity, while every committed
        // continuation starts at a full continuation capacity.  Translate
        // from that selected page's local origin rather than retaining the
        // first fragment's origin for every slice.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
        let fragment_translation = if child_fragment_replay {
            PaintTranslation::new(slice_border_box.x(), slice_border_box.y())
        } else {
            PaintTranslation::new(
                slice_border_box.x(),
                source_item_top - offpage_top - source_canvas_slice_start,
            )
        };
        let source_fragment = if child_fragment_replay {
            table_replay_fragments
                .get(table_fragment_ordinal)
                .cloned()
                .unwrap_or_else(|| self.current_page.paint_fragment())
        } else {
            self.current_page.paint_fragment()
        };
        let fragment = source_fragment.translated(fragment_translation);
        // A descendant-overflow source replay contains only the item's
        // independently formatted subtree; its principal flex decoration was
        // committed separately from the materialized item record above.
        // Clip that whole subtree structurally so descendant backgrounds are
        // sliced with their content while nested stacking contexts (notably
        // opacity) retain their effects. A contents-only overflow scope
        // deliberately leaves BackgroundBorder paint outside the clip, which
        // lets a tall descendant background cover a later flex item's owner.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        // <https://www.w3.org/TR/css-color-4/#transparency>
        // The selected flex slice constrains every descendant paint band,
        // including an ordinary nested block's background. A contents-only
        // overflow scope leaves that nested BackgroundBorder band outside the
        // continuation clip, so a final partial slice either disappears under
        // the flex container's background or paints through later siblings.
        // Preserve the nested paint-tree structure while clipping it at the
        // common fragment-span boundary.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let fragment =
            fragment.with_primitives_clipped_to_rect_preserving_structure(slice_border_box);
        // The geometric fragmentainer slice above has already trimmed the
        // captured primitive tree. Do not wrap it in a second PDF overflow
        // clip: applying one around the complete stacking context
        // antialiases a background at every page edge, unlike the equivalent
        // ordinary block which paints each used slice directly.
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        if table_replay {
            record_table_replay_fragment_bottom(
                table_replay_fragment_bottoms,
                context.continuation.fragmentainer_index,
                &fragment,
            );
        }
        if !table_replay
            && let Some(Some(local_block_end)) =
                replay_fragment_local_block_ends.get(table_fragment_ordinal)
        {
            *replay_destination_block_end = Some(slice_border_box.y() + *local_block_end);
        }
        self.restore(snapshot);

        // An empty size-contained flex item has no descendant paint to replay:
        // its durable flex fragment span has already materialized the full
        // principal border-box decoration for every destination page. Replaying
        // the scratch tree would add that same decoration a second time, with
        // a different local fragmentainer origin on continuations.
        // <https://drafts.csswg.org/css-contain-1/#containment-size>
        // <https://drafts.csswg.org/css-break-3/#box-splitting>
        if !replay_owns_principal_decoration || replay_has_descendant_paint {
            self.append_split_flex_item_replay(placed_style, slice_border_box, fragment);
        }

        // A layer which escaped the split item belongs to the flex
        // container's containing block. Positioned layout has therefore
        // already expressed its inline coordinate in the source page's
        // coordinate space; translating it by the item slice's inline origin
        // would apply that origin twice. Its block coordinate is still on the
        // off-page replay canvas and needs the selected source-slice mapping.
        // <https://www.w3.org/TR/css-position-3/#def-cb>
        // <https://www.w3.org/TR/css-break-3/#box-splitting>
        let replay_translation = PaintTranslation::new(0.0, fragment_translation.y);
        for layer in escaped_positioned_layers {
            let layer_fragment = positioned_layer_fragment(&layer);
            let mut fragment = layer_fragment.translated(replay_translation);
            if let Some(clip) = context.positioned_descendant_clip {
                fragment = fragment.clipped_to_rect(clip);
            }
            if !fragment.is_empty() {
                self.current_page
                    .append_paint_fragment_owned(fragment, PaintTranslation::identity());
            }
        }
    }

    fn append_split_flex_item_replay(
        &mut self,
        placed_style: &ComputedStyle,
        slice_border_box: PaintClip,
        fragment: PaintFragment,
    ) {
        if fragment.is_empty() {
            return;
        }
        let policy = StackingContextPolicy::for_flex_item(placed_style, slice_border_box);
        let effects = policy.effects;
        let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(effects)
            .with_bounds(slice_border_box);
        self.current_page.append_paint_fragment_owned(
            PaintFragment::from_stacking_context_in_band(policy.parent_band, context),
            PaintTranslation::identity(),
        );
    }

    pub(in crate::layout::flex) fn resolve_styled_children_used_lengths(
        &mut self,
        children: &mut [StyledChild<'_>],
    ) {
        for child in children {
            child.style = self
                .style_with_current_used_lengths(&child.style)
                .into_computed();
        }
    }
}

/// Return the synthesized inline-level baseline for an `inline-flex` atom.
///
/// CSS Flexbox leaves an empty row flex container without a main-axis baseline
/// set; CSS Inline then synthesizes the atomic inline's baseline from its
/// margin box in the inline formatting context:
/// <https://drafts.csswg.org/css-flexbox/#flex-baselines> and
/// <https://www.w3.org/TR/css-inline-3/#atomic-inline>.
pub(in crate::layout::flex) fn inline_flex_synthesized_baseline_offset(
    style: &ComputedStyle,
    border_box_height: BorderBoxLength,
) -> FlexVerticalBaselineOffset {
    match style.writing_mode {
        // `InlineAtom` stores its exported baseline from the border-box
        // block start. The shared atomic-inline adapter adds the block-start
        // margin and compares it with the complete margin-box size, so adding
        // the block-end margin here would count it twice.
        WritingMode::HorizontalTb => FlexVerticalBaselineOffset::new(border_box_height.points()),
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => FlexVerticalBaselineOffset::new(border_box_height.points()),
    }
}

/// Select the finalized flex baseline that the current horizontal inline atom
/// transport can carry.
///
/// Flex preserves first/last baseline sets for both physical axes.  This
/// legacy atom boundary carries only a physical vertical offset, so it must
/// consume the vertical export deliberately instead of treating a horizontal
/// baseline as a y-coordinate.  A missing compatible export is left for CSS
/// Inline baseline synthesis from the atom's margin box:
/// <https://www.w3.org/TR/css-flexbox-1/#flex-baselines> and
/// <https://www.w3.org/TR/css-inline-3/#atomic-inline>.
pub(in crate::layout::flex) fn inline_flex_exported_vertical_baseline(
    baselines: FlexContainerBaselineEstimate,
) -> Option<FlexVerticalBaselineOffset> {
    baselines.vertical.first
}
