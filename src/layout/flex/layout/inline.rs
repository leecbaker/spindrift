use super::*;
use crate::units::Definite;
/// Combine the inline line formatter's item span with final flex-line
/// geometry for an automatic physical-column `inline-flex`.
///
/// `main_gap` has already resolved its flex percentage basis, so a cyclic
/// percentage gap arrives as zero.  The final line extent is retained as a
/// lower bound for used margins, minimums, and overflow-safe geometry.
/// <https://www.w3.org/TR/css-align-3/#gaps>
fn final_physical_column_inline_flex_line_span(
    normal_flow_item_span: PhysicalContentHeight,
    finalized_line_span: PhysicalContentHeight,
    main_gap: FlexMainSize,
    non_collapsed_item_count: usize,
) -> PhysicalContentHeight {
    let separator_count = non_collapsed_item_count.saturating_sub(1) as f32;
    let normal_flow_line_span = PhysicalContentHeight::new(content_box_pt(
        normal_flow_item_span.points() + main_gap.scale(separator_count).points(),
    ));
    PhysicalContentHeight::new(content_box_pt(
        normal_flow_line_span
            .points()
            .max(finalized_line_span.points()),
    ))
}

impl<'a> LayoutBuilder<'a> {
    /// Measure the automatic physical block span of a column `inline-flex`.
    ///
    /// The inline line formatter supplies leading that is not represented by
    /// Taffy's final item rectangles.  It must nevertheless be measured per
    /// *final* flex line and retain the resolved main-axis gutters between
    /// that line's non-collapsed items.  The final flex-line extent remains a
    /// lower bound so used margins and minimum sizes cannot disappear from an
    /// automatic container size.
    ///
    /// This is deliberately limited to a one-to-one itemization of the box
    /// tree.  Anonymous flex-item construction has no corresponding
    /// `FormattingBox` slice for the inline probe; in that case the final
    /// flex geometry is already the safe authoritative fallback.
    /// <https://www.w3.org/TR/css-flexbox-1/#layout-algorithm>
    /// <https://www.w3.org/TR/css-align-3/#gaps>
    fn automatic_physical_column_inline_flex_height(
        &mut self,
        flex_layout: &FlexLayout,
        style: &ComputedStyle,
        child_boxes: &[box_tree::FormattingBox<'_>],
        flex_item_count: usize,
        stylesheets: &Stylesheets<'_>,
        vertical_non_content: NonContentLength,
    ) -> Option<PhysicalContentHeight> {
        let mut order_modified_boxes = child_boxes
            .iter()
            .filter(|child| {
                !child.style().display.is_none()
                    && !matches!(child.style().position, Position::Absolute | Position::Fixed)
            })
            .cloned()
            .collect::<Vec<_>>();
        order_modified_boxes.sort_by_key(|child| child.style().order);

        // Flex itemization groups adjacent inline/text children into one
        // anonymous item.  Do not guess a source-box mapping in that case:
        // preserve the final flex result rather than measuring a different
        // formatting context.
        if order_modified_boxes.len() != flex_item_count {
            return None;
        }

        let reverse_main_axis = matches!(
            physical_flex_direction(style),
            FlexDirection::ColumnReverse | FlexDirection::RowReverse
        );
        let mut largest_line_span = PhysicalContentHeight::new(content_box_pt(0.0));

        for line in &flex_layout.lines {
            let mut line_boxes = line
                .item_indices
                .iter()
                .map(|&index| order_modified_boxes.get(index).cloned())
                .collect::<Option<Vec<_>>>()?;
            if line_boxes.is_empty() {
                continue;
            }
            if reverse_main_axis {
                line_boxes.reverse();
            }

            let atom = self.inline_fragment_atom_for_children(
                None,
                style,
                &line_boxes,
                stylesheets,
                0.0,
                None,
            );
            let normal_flow_item_span = PhysicalContentHeight::new(content_box_pt(
                (atom.size.height
                    - style.margin.top
                    - style.margin.bottom
                    - vertical_non_content.points())
                .max(0.0),
            ));
            let finalized_line_span = PhysicalContentHeight::new(content_box_pt(
                line.main_end
                    .relative_to(line.main_start)
                    .non_negative_size()
                    .points(),
            ));
            largest_line_span = PhysicalContentHeight::new(content_box_pt(
                largest_line_span.points().max(
                    final_physical_column_inline_flex_line_span(
                        normal_flow_item_span,
                        finalized_line_span,
                        flex_layout.main_gap,
                        line.item_indices.len(),
                    )
                    .points(),
                ),
            ));
        }

        Some(largest_line_span)
    }

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
        // Intrinsic sizing of an automatic `inline-flex` must retain cyclic
        // percentage padding as unresolved. Resolving its items against the
        // surrounding inline formatting context here would feed that context
        // width into the flex container's own shrink-to-fit width. Keep a
        // source-style snapshot for the intrinsic pass; final flex layout
        // below receives the ordinary used-length children after its content
        // width is known.
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        // <https://www.w3.org/TR/css-flexbox-1/#intrinsic-sizes>
        let intrinsic_children = children.clone();
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
                &intrinsic_children,
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
                style.text_decoration_origins.effective_layers_vec(),
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
                style.clone_used_style(),
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

        // A column inline-flex's automatic physical block size is the span
        // of its final in-flow lines, not Taffy's reduced item-rectangle
        // proxy.  The normal-flow probe retains line-box leading selected by
        // the inline formatter, and final flex geometry restores the main
        // gaps that plain normal flow does not contain.  Keep paint capture
        // and fragmentation independent from used layout geometry.
        //
        // This applies only to an automatic physical column: a row's final
        // cross-size is resolved by its flex lines, and an explicit height
        // remains the author's used content-box size.
        // <https://www.w3.org/TR/css-flexbox-1/#algo-main-container>
        // <https://www.w3.org/TR/css-inline-3/#line-box>
        let final_physical_column_content_height = (!containment.size
            && physical_flex_direction(style).is_column_axis()
            && definite_content_height.is_none())
        .then(|| {
            self.automatic_physical_column_inline_flex_height(
                &flex_layout,
                style,
                child_boxes,
                children.len(),
                stylesheets,
                vertical_non_content,
            )
        })
        .flatten();

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
                content_box_pt(
                    final_physical_column_content_height
                        .map(PhysicalContentHeight::points)
                        .unwrap_or_else(|| flex_layout.height.points()),
                ),
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
        let inline_atom_baselines = (!containment.layout).then(|| {
            flex_layout.baselines.into_inline_atom_baselines(
                layout_pt(border_widths.top + style.padding.top),
                layout_pt(border_widths.left + style.padding.left),
            )
        });
        // The off-page atom capture may lay out descendant lines, but they
        // are not principal lines of an ancestor list item. Keep the
        // ancestor's marker anchors out of this scratch coordinate space.
        let pending_outside_marker_anchors = self.pending_outside_marker_anchors.suspend();
        let snapshot = self.snapshot();
        // The scratch page below is only a flex-paint construction space.
        // Retain the real fallback containing block for descendants that are
        // not contained by this inline-flex itself.
        let escaped_atom_actual_containing_block = self.current_containing_block();
        let positioned_layer_start = self.positioned_layers.len();
        let top = 10_000.0;
        let content_top = top - border_widths.top - style.padding.top;
        let inner_x = border_widths.left + style.padding.left;
        let inner_width = content_width_points;
        self.current_page = Page::new(content_width_points + horizontal_non_content.points(), top);
        self.content_left = inner_x;
        self.content_right = inner_x + inner_width;
        self.cursor_y = content_top;
        // The scratch flex formatting context owns a fresh hypothetical line.
        // An enclosing inline static-position rectangle belongs to the parent
        // line and would be translated again when this atom is replayed.
        // <https://www.w3.org/TR/css-position-3/#static-position>
        self.inline_static_position = None;
        self.truncate_page_start_margins = false;
        let atomic_static_position = AtomicInlineStaticPosition::new(
            PageTopRect::new(
                inner_x,
                content_top,
                inner_width,
                total_content_height.max(0.0),
            ),
            WritingModeAxes::new(style.writing_mode, style.used_direction()),
        );
        let atomic_formatting_context_scope =
            self.begin_atomic_inline_formatting_context(style, atomic_static_position.content_rect);

        let previous_escaped_atom_positioning_context = self.escaped_atom_positioning_context;
        let previous_block_static_position_y_offset = self.block_static_position_y_offset;
        let previous_absolute_static_position = self.absolute_static_position;
        // Descendant positioned layout needs a local static-position
        // rectangle while retaining the outer containing block for explicit
        // insets. This is the same escaped-atom contract used by
        // inline-block capture.
        // <https://www.w3.org/TR/css-position-3/#static-position-rectangle>
        self.block_static_position_y_offset = Some(0.0);
        self.absolute_static_position = Some(atomic_static_position.in_atomic_space());
        self.escaped_atom_positioning_context = Some(EscapedAtomPositioningContext {
            actual_containing_block: escaped_atom_actual_containing_block,
            static_position: atomic_static_position,
        });
        self.escaped_atom_positioning_depth += 1;

        let positioning_containing_block_mode = PositionedContainingBlockMode::for_style(style);
        let positioned_containing_block_scope =
            if let Some(mode) = positioning_containing_block_mode {
                let containing_block = ContainingBlock::from_page_top_rect(PageTopRect::new(
                    border_widths.left,
                    top - border_widths.top,
                    content_width_points + style.padding.left + style.padding.right,
                    total_content_height + style.padding.top + style.padding.bottom,
                ));
                let scope = self.push_positioned_containing_block(mode, containing_block);
                Some(scope)
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
            let replay_content_height = Some(Definite::new(
                replay_dimensions.available_height_for_replay(),
            ));
            let replay_logical_inline_size = child
                .anonymous_content()
                .is_some()
                .then(|| {
                    replay_dimensions
                        .logical_inline_size_for_replay(WritingMode::HorizontalTb, None)
                })
                .flatten()
                .or_else(|| {
                    Some(replay_dimensions.logical_inline_content_size_for_replay(&placed_style))
                });
            self.with_placed_formatting_context(
                PlacedFormattingContext {
                    content_left: inner_x + item.x().points(),
                    content_width: replay_dimensions.available_width_for_replay(),
                    content_height: replay_content_height,
                    table_wrapper_border_box_block_size: auto_table_wrapper_block_size_override(
                        &child.style,
                        replay_dimensions.border_box_height(),
                    ),
                    replay_logical_inline_size,
                    cursor_y: content_top - item.y().points(),
                    page_start_margin_policy: PageStartMarginPolicy::Suppress,
                    float_scope: ReplayFloatScope::IsolatedFormattingContext,
                },
                &placed_style,
                |layout| {
                    layout.layout_flex_item_contents(
                        child,
                        &placed_style,
                        stylesheets,
                        item.percentage_height_basis,
                        PrincipalBoxPaintMode::RootPaints,
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

        self.escaped_atom_positioning_depth -= 1;
        self.block_static_position_y_offset = previous_block_static_position_y_offset;
        self.absolute_static_position = previous_absolute_static_position;
        self.escaped_atom_positioning_context = previous_escaped_atom_positioning_context;
        if let Some(scope) = positioned_containing_block_scope {
            self.pop_positioned_containing_block(scope);
        }
        self.end_atomic_inline_formatting_context(atomic_formatting_context_scope);
        let atom_bounds = PaintClip::from_paint_rect(paint_space_rect(
            0.0,
            top - border_box_height_points,
            content_width_points + horizontal_non_content.points(),
            border_box_height_points,
        ));
        let policy = StackingContextPolicy::for_atomic(style, PaintBand::Inline, atom_bounds);
        let escaped_positioned_layers =
            if matches!(policy.child_layer_policy, ChildLayerPolicy::EscapeAll)
                && positioned_layer_start < self.positioned_layers.len()
            {
                // An inline-flex is atomically painted for its in-flow
                // contents, but an abspos descendant whose containing block
                // is outside the flex container belongs to the parent
                // stacking context instead of its captured scratch fragment.
                // <https://www.w3.org/TR/CSS22/zindex.html>
                self.positioned_layers.split_off(positioned_layer_start)
            } else {
                Vec::new()
            };
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
        // Normalize the capture before restoring the outer builder. Keeping
        // a 10,000pt scratch origin until inline replay loses sub-pixel CSS
        // geometry through cancellation in the final atom translation.
        // Outer margins still participate only in the parent line's
        // margin-box placement.
        let scratch_border_box_origin = PaintPoint::new(0.0, top - border_box_height_points);
        let capture_frame =
            AtomicInlineCaptureFrame::for_scratch_border_box(scratch_border_box_origin);
        fragment = fragment.translated(PaintTranslation::new(
            -scratch_border_box_origin.x,
            -scratch_border_box_origin.y,
        ));
        let replay_coordinates = AtomicInlineFragmentReplayCoordinates::border_box_local();
        let escaped_positioned_layers = escaped_positioned_layers
            .into_iter()
            .map(|mut layer| {
                layer.escaped_atom_replay =
                    capture_frame.resolve_positioned_replay(layer.escaped_atom_replay);
                layer
            })
            .collect::<Vec<_>>();
        let escaped_positioned_layers = (!escaped_positioned_layers.is_empty())
            .then(|| escaped_positioned_layers.into_boxed_slice());
        self.restore(snapshot);
        self.pending_outside_marker_anchors
            .restore(pending_outside_marker_anchors);

        let atom = InlineAtom::new(
            InlineAtomContent::InlineFragment {
                fragment: Box::new(fragment),
                replay_coordinates,
                table_cell_context: None,
                contents_overflow_clip_applied: false,
            },
            style.clone_used_style(),
            escaped_positioned_layers,
            InlineSize::new(
                content_width_points
                    + horizontal_non_content.points()
                    + style.margin.left
                    + style.margin.right,
                border_box_height_points + style.margin.top + style.margin.bottom,
            ),
            0.0,
            baseline_shift,
            link_target,
            None,
        );
        match inline_atom_baselines {
            Some(baselines) => atom.with_flex_exported_baselines(baselines),
            None => atom.with_synthesized_border_box_block_end_baseline(),
        }
    }
}
#[cfg(test)]
mod inline_flex_baseline_tests {
    use super::*;

    #[test]
    fn final_column_line_span_adds_one_gap_between_each_noncollapsed_item() {
        assert_eq!(
            final_physical_column_inline_flex_line_span(
                PhysicalContentHeight::new(content_box_pt(40.0)),
                PhysicalContentHeight::new(content_box_pt(55.0)),
                FlexMainSize::new(10.0),
                4,
            ),
            PhysicalContentHeight::new(content_box_pt(70.0)),
        );
    }

    #[test]
    fn final_column_line_span_retains_final_geometry_and_zero_gap_behavior() {
        // The caller resolves cyclic percentage gaps through flex layout;
        // their used value is zero by the time it reaches this helper.
        assert_eq!(
            final_physical_column_inline_flex_line_span(
                PhysicalContentHeight::new(content_box_pt(24.0)),
                PhysicalContentHeight::new(content_box_pt(32.0)),
                FlexMainSize::new(0.0),
                3,
            ),
            PhysicalContentHeight::new(content_box_pt(32.0)),
        );
    }

    #[test]
    fn source_slice_replay_translation_keeps_a_continuation_in_its_destination_clip() {
        let destination = PaintClip::new(77.25, 320.25, 82.5, 60.0);
        let translation = source_slice_replay_translation(destination, 380.25, 10_000.0, 52.5);

        // The overflowing descendant occupies source page-top coordinates
        // 9_880..9_985. Its continuation must intersect the destination
        // slice 320.25..380.25 after the source offset is applied.
        let translated_bottom = 9_880.0 + translation.y;
        let translated_top = 9_985.0 + translation.y;
        assert_eq!(translation.y, -9_567.25);
        assert!(translated_bottom < destination.y() + destination.height());
        assert!(translated_top > destination.y());
    }

    #[test]
    fn largest_wrapped_column_line_span_controls_automatic_height() {
        let first_line = final_physical_column_inline_flex_line_span(
            PhysicalContentHeight::new(content_box_pt(20.0)),
            PhysicalContentHeight::new(content_box_pt(20.0)),
            FlexMainSize::new(10.0),
            2,
        );
        let second_line = final_physical_column_inline_flex_line_span(
            PhysicalContentHeight::new(content_box_pt(36.0)),
            PhysicalContentHeight::new(content_box_pt(30.0)),
            FlexMainSize::new(10.0),
            2,
        );

        assert_eq!(first_line.points().max(second_line.points()), 46.0);
    }

    #[test]
    fn exported_content_baselines_convert_to_physical_border_box_coordinates() {
        let baselines = FlexContainerBaselineSets {
            vertical: FlexItemBaselinePair {
                first: Some(flex_vertical_baseline_from_points(6.0)),
                last: Some(flex_vertical_baseline_from_points(9.0)),
            },
            horizontal: FlexItemBaselinePair {
                first: Some(flex_horizontal_baseline_from_points(8.0)),
                last: Some(flex_horizontal_baseline_from_points(12.0)),
            },
            ..FlexContainerBaselineSets::default()
        }
        .into_inline_atom_baselines(layout_pt(4.0), layout_pt(5.0));
        assert_eq!(baselines.vertical.first.unwrap().points(), 10.0);
        assert_eq!(baselines.vertical.last.unwrap().points(), 13.0);
        assert_eq!(baselines.horizontal.first.unwrap().points(), 13.0);
        assert_eq!(baselines.horizontal.last.unwrap().points(), 17.0);
    }
}
