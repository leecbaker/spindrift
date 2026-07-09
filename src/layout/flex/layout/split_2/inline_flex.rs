use super::*;
use crate::layout::builder::positioned_layer_fragment;

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
        stylesheets: &[Stylesheet],
        baseline_shift: f32,
        link_target: Option<String>,
    ) -> InlineAtom {
        let available_width =
            (self.content_right - self.content_left - style.margin.left - style.margin.right)
                .max(style.font_size);
        let mut used_style =
            FlexUsedStyle::from_normalized(self.style_with_current_viewport_lengths(style));
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_width)),
        );
        let style = &used_style;
        let border_widths = box_metrics.border;
        let horizontal_extras = box_metrics.horizontal_non_content_length().points();
        let vertical_extras = box_metrics.vertical_non_content_length().points();
        let (mut children, mut positioned_children) =
            flex_child_lists_from_boxes(element, signature, style, child_boxes);
        self.resolve_styled_children_viewport_lengths(&mut children);
        self.resolve_styled_children_viewport_lengths(&mut positioned_children);

        let height_percentage_basis = self.flex_container_height_percentage_basis();
        let intrinsic_content_height = used_content_box_height_or_auto_with_basis(
            style,
            height_percentage_basis,
            non_content_pt(vertical_extras),
        )
        .map(SemanticLengthExt::points);
        // Size containment substitutes the intrinsic sizes of empty content
        // for the principal flex box. The real flex layout still runs below so
        // descendants paint, overflow, and contribute the exported baseline.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        let intrinsic = if style.contain.size {
            FlexItemEstimate::fixed(0.0, 0.0)
        } else {
            self.estimate_intrinsic_flex_container_size(
                &children,
                style,
                stylesheets,
                FlexAvailableSpace {
                    width: PhysicalContentWidth::new(content_box_pt(available_width.max(0.0))),
                    width_basis: flex_available_percentage_basis_from_points(
                        used_content_box_width_or_auto(
                            style,
                            layout_pt(available_width.max(0.0)),
                            non_content_pt(horizontal_extras),
                        )
                        .map(|_| available_width.max(0.0)),
                        FlexAvailableSizeSource::IntrinsicContainerSize,
                    ),
                    height: intrinsic_content_height
                        .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                    height_basis: flex_available_percentage_basis_from_points(
                        intrinsic_content_height,
                        FlexAvailableSizeSource::IntrinsicContainerSize,
                    ),
                },
            )
        };
        let requested_content_width = flex_container_content_width_from_intrinsic(
            style,
            available_width,
            horizontal_extras,
            intrinsic,
            true,
        );
        let content_width = constrain_content_width(
            style,
            requested_content_width,
            PercentageBasis::definite(layout_pt(available_width.max(0.0))),
        )
        .points();

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
                style.text_decoration.clone(),
                &mut items,
            );
            let measurement =
                self.intrinsic_inline_measurement_for_items(items.clone(), style, f32::MAX);
            let natural_width = measurement.contribution.max_content;
            let sequence = self.collect_inline_line_sequence_with_text_box_trim(
                items,
                style,
                natural_width,
                0.0,
                0.0,
            );
            let content_height = sequence.total_height().max(style.line_height);
            let border_box_height = content_height + vertical_extras;
            let line_baseline_offset =
                self.inline_box_sequence_baseline_offset(&sequence, style, border_widths);
            let baseline_offset =
                Self::inline_block_baseline_offset(style, border_box_height, line_baseline_offset);
            let overflow_offset = if style.flex_direction == FlexDirection::RowReverse
                && style.direction == Direction::Ltr
            {
                content_width - natural_width
            } else {
                0.0
            };
            return InlineAtom::new(
                InlineAtomContent::InlineBox { sequence },
                style.as_computed().clone(),
                None,
                InlineSize::new(
                    content_width + horizontal_extras + style.margin.left + style.margin.right,
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

        let height_constraint_basis = height_percentage_basis.points().unwrap_or(available_width);
        let explicit_content_height = used_content_box_height_or_auto_with_basis(
            style,
            height_percentage_basis,
            non_content_pt(vertical_extras),
        )
        .map(|height| {
            constrain_content_height(
                style,
                height,
                PercentageBasis::definite(layout_pt(height_constraint_basis)),
            )
            .points()
        });
        let definite_content_height = definite_flex_container_content_height(
            style,
            explicit_content_height,
            content_width,
            PercentageBasis::definite(layout_pt(height_constraint_basis)),
            horizontal_extras,
            vertical_extras,
        );
        let flex_available_content_height = flex_available_content_height(
            style,
            definite_content_height,
            PercentageBasis::definite(layout_pt(content_width)),
        );

        let flex_available = FlexAvailableSpace {
            width: PhysicalContentWidth::new(content_box_pt(content_width)),
            width_basis: flex_available_percentage_basis_from_points(
                Some(content_width),
                FlexAvailableSizeSource::ContainingBlock,
            ),
            height: flex_available_content_height
                .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
            height_basis: flex_available_percentage_basis_from_points(
                definite_content_height,
                FlexAvailableSizeSource::ContainingBlock,
            ),
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

        let total_content_height = if style.contain.size {
            definite_content_height.unwrap_or_else(|| {
                constrain_content_height(
                    style,
                    content_box_pt(0.0),
                    PercentageBasis::definite(layout_pt(height_constraint_basis)),
                )
                .points()
            })
        } else {
            constrain_content_height(
                style,
                content_box_pt(flex_layout.height),
                PercentageBasis::definite(layout_pt(content_width)),
            )
            .points()
        };
        debug_assert!(!flex_layout.lines.is_empty() || children.is_empty());
        debug_assert!(
            flex_layout.fragment_plan.is_empty()
                || flex_layout.fragment_plan.planned_item_fragment_count()
                    <= flex_layout.items.len()
        );
        let border_box_height = total_content_height + vertical_extras;
        let estimated_baseline_offset = (!style.contain.layout)
            .then_some(flex_layout.first_baseline)
            .flatten()
            .map(|baseline| border_widths.top + style.padding.top + baseline)
            .unwrap_or_else(|| inline_flex_synthesized_baseline_offset(style, border_box_height));

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let top = 10_000.0;
        let content_top = top - border_widths.top - style.padding.top;
        let inner_x = border_widths.left + style.padding.left;
        let inner_width = content_width.max(0.0);
        self.current_page = Page::new(content_width + horizontal_extras, top);
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
                    content_width + style.padding.left + style.padding.right,
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
            let item_width = item.width().max(0.0);
            let item_height = item.height().max(0.0);

            let mut replay_child_style = child.style.clone();
            freeze_replayed_item_padding(
                &mut replay_child_style,
                flex_item_used_padding(&child.style, style, flex_available),
            );
            let placed_style = placed_flex_item_style(
                &replay_child_style,
                item_width,
                item_height,
                style.flex_direction,
            );
            self.with_formatting_context_item_placement(
                FormattingContextItemPlacement {
                    content_left: inner_x + item.x(),
                    content_width: PhysicalContentWidth::new(content_box_pt(item_width)),
                    content_height: Some(PhysicalContentHeight::new(content_box_pt(item_height))),
                    table_wrapper_border_box_block_size: auto_table_wrapper_block_size_override(
                        &child.style,
                        border_box_pt(item_height),
                    ),
                    writing_mode: placed_style.writing_mode,
                    scope_content_logical_inline_size: child.anonymous_content().is_some()
                        && style.flex_direction.is_column_axis(),
                    cursor_y: content_top - item.y(),
                    page_start_margin_policy: PageStartMarginPolicy::Suppress,
                },
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
                        height: flex_available_content_height
                            .map(|height| PhysicalContentHeight::new(content_box_pt(height))),
                        height_basis: flex_available_percentage_basis_from_points(
                            definite_content_height,
                            FlexAvailableSizeSource::ContainingBlock,
                        ),
                    },
                    inner_x,
                    inner_width,
                    content_top,
                },
            );
        }

        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        let border_bottom = top - border_box_height;
        self.flush_positioned_layers_since(positioned_layer_start);
        let mut fragment = self.current_page.paint_fragment();
        if style.visibility == Visibility::Visible {
            let gap_gutters =
                flex_gap_decoration_gutters(&flex_layout, style, inner_width, total_content_height);
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
        // A flex container with no participating flex-item baseline falls
        // back to a synthesized baseline. Its captured inline descendants may
        // nevertheless have a concrete first line, which is the baseline
        // exported by the atomic inline formatting context. Preserve a real
        // flex baseline when one exists; only consult the captured fragment
        // for the no-baseline fallback.
        // <https://drafts.csswg.org/css-flexbox/#flex-baselines>
        let captured_first_line_can_supply_baseline = flex_layout.first_baseline.is_none()
            // A wrapping column container exports its main-axis baseline from
            // the first item with a parallel baseline, not from the captured
            // fragment's first painted line. That line can belong to a later
            // cross-axis flex line (including wrap-reverse).
            // <https://drafts.csswg.org/css-flexbox/#flex-baselines>
            && !(style.flex_direction.is_column_axis() && style.flex_wrap.wraps());
        let baseline_offset = captured_first_line_can_supply_baseline
            .then(|| {
                fragment
                    .first_line_y()
                    .map(|line_y| (border_box_height - line_y).max(0.0))
            })
            .flatten()
            .unwrap_or(estimated_baseline_offset);
        self.restore(snapshot);

        InlineAtom::new(
            InlineAtomContent::InlineFragment(Box::new(fragment)),
            style.as_computed().clone(),
            None,
            InlineSize::new(
                content_width + horizontal_extras + style.margin.left + style.margin.right,
                border_box_height + style.margin.top + style.margin.bottom,
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
        let mut hypothetical_child = child.clone();
        hypothetical_child.style.position = Position::Static;
        hypothetical_child.style.flex_grow = 0.0;
        hypothetical_child.style.flex_shrink = 0.0;
        hypothetical_child.style.flex_basis = css::ComputedFlexBasis::Auto;
        zero_auto_margins_for_static_flex_probe(&mut hypothetical_child.style);
        if hypothetical_child.style.display.is_inline_level() {
            hypothetical_child.style.display = hypothetical_child.style.display.blockified();
        }
        let hypothetical = self
            .compute_flex_layout(
                std::slice::from_ref(&hypothetical_child),
                context.container_style,
                context.stylesheets,
                context.available,
            )
            .and_then(|layout| layout.items.into_iter().next())
            .unwrap_or_else(|| {
                FlexItemLayout::new(ContainerRect::new(
                    ContainerPoint::new(0.0, 0.0),
                    ContainerSize::new(context.inner_width, child.style.line_height),
                ))
            });

        let static_left = context.inner_x + hypothetical.x();
        self.layout_positioned_formatting_context_child(
            child,
            context.stylesheets,
            PositionedChildStaticRect::new(
                static_left,
                static_left + hypothetical.width(),
                context.content_top - hypothetical.y(),
            ),
        );
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
        stylesheets: &[Stylesheet],
        context: SplitFlexItemPaintContext,
    ) {
        let item_width = context.item_width.points();
        let item_height = context.item_height.points();
        let slice_border_box = context.slice_border_box;
        let source_item_top = context.source_item_top;
        let content_slice_start = if context.replay_source_slice_offset {
            context
                .continuation
                .source_content_slice
                .block_start
                .points()
        } else {
            0.0
        };
        if slice_border_box.height() <= 0.0 {
            return;
        }

        let snapshot = self.snapshot();
        let positioned_layer_start = self.positioned_layers.len();
        let offpage_top = 10_000.0;
        self.current_page = Page::new(item_width.max(1.0), offpage_top);
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
                            + content_slice_start,
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
                content_width: PhysicalContentWidth::new(content_box_pt(item_width)),
                content_height: Some(PhysicalContentHeight::new(content_box_pt(item_height))),
                table_wrapper_border_box_block_size: auto_table_wrapper_block_size_override(
                    &child.style,
                    border_box_pt(item_height),
                ),
                writing_mode: placed_style.writing_mode,
                scope_content_logical_inline_size: false,
                cursor_y: offpage_top,
                page_start_margin_policy: PageStartMarginPolicy::Suppress,
            },
            |layout| {
                layout.layout_flex_item_contents(
                    child,
                    placed_style,
                    stylesheets,
                    context.percentage_height_basis,
                );
            },
        );

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

        let fragment_translation = PaintTranslation::new(
            slice_border_box.x(),
            source_item_top - offpage_top - content_slice_start,
        );
        let fragment = self
            .current_page
            .paint_fragment()
            .translated(fragment_translation)
            .clipped_to_rect(slice_border_box);
        self.restore(snapshot);

        self.append_split_flex_item_replay(placed_style, slice_border_box, fragment);

        let replay_translation = fragment_translation;
        for layer in escaped_positioned_layers {
            let mut fragment = positioned_layer_fragment(&layer).translated(replay_translation);
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
        let mut effects = policy.effects;
        effects.overflow_clip = Some(slice_border_box);
        effects.absolute_clip = Some(slice_border_box);
        let context = PaintStackingContext::from_banded_fragment(fragment, Vec::new())
            .with_source_order(self.next_paint_source_order())
            .with_effects(effects)
            .with_bounds(slice_border_box);
        self.current_page.append_paint_fragment_owned(
            PaintFragment::from_stacking_context_in_band(policy.parent_band, context),
            PaintTranslation::identity(),
        );
    }

    pub(in crate::layout::flex) fn resolve_styled_children_viewport_lengths(
        &self,
        children: &mut [StyledChild<'_>],
    ) {
        for child in children {
            self.resolve_style_current_viewport_lengths(&mut child.style);
            // Flex item sizing consumes the item style directly rather than
            // rebuilding it through a normal-flow used-style helper.
            child.style.apply_effective_zoom();
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
    border_box_height: f32,
) -> f32 {
    match style.writing_mode {
        WritingMode::HorizontalTb => border_box_height + style.margin.bottom,
        WritingMode::VerticalRl
        | WritingMode::VerticalLr
        | WritingMode::SidewaysRl
        | WritingMode::SidewaysLr => border_box_height,
    }
}
