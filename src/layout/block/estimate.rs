use super::super::{
    BlockAutoWidthRole, BlockContentWidthInputs, BlockSizeBasisSource, ComputedStyle,
    DescendantBlockPercentageContext, DisplayInner, Element, ElementSignature, Float, FloatContext,
    FloatId, FloatPlacementAxes, FloatShape, IntrinsicBlockBasis,
    IntrinsicInlinePercentageBasisSource, LayoutBuilder, NodeKind, PageInlineSpan,
    PageTopBlockPosition, PageTopRect, PercentageBasis, PhysicalContentHeight,
    PhysicalContentWidth, Position, ReplacedBoxSizingContext, SemanticLengthExt, Stylesheets,
    UsedFloatSide, WritingMode, apply_used_box_metrics, box_tree, constrain_content_height,
    constrain_content_width, constrain_height_with_intrinsic, constrain_width_with_intrinsic,
    content_box_pt, css, dom, element_sibling_signature_list, element_signature,
    formatting_box_has_inline_content, has_atomic_inline_formatting_box, has_auto_height,
    has_direct_flow_child_with_font_metrics, has_direct_inline_content_box,
    has_direct_inline_replaced_child, has_non_inline_formatting_box, inline_text_for_style,
    inline_text_from_formatting_boxes, intrinsic, is_replaced_element, layout_pt,
    margin_box_size_pt, needs_intrinsic_height_contribution, needs_intrinsic_width_contribution,
    own_inline_text_for_style, resolve_replaced_element, used_border_widths, used_box_metrics,
    used_content_box_height_or_auto, used_content_box_size_with_basis,
    used_content_box_width_or_auto, used_length_percentage, used_max_height, used_min_height,
    used_property_containment,
};
use super::float::{freeze_float_replay_height, freeze_float_replay_width};

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn estimate_element_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_outer_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Option<f32> {
        #[cfg(feature = "layout-profile")]
        let _profile_scope = crate::layout::layout_profile::block_height_estimate_scope();
        if style.display.is_none() {
            Some(0.0)
        } else if let Some(replaced) = resolve_replaced_element(
            element,
            style,
            ReplacedBoxSizingContext {
                available_width: content_box_pt(available_outer_width),
                inline_percentage_basis: PercentageBasis::definite_from(
                    content_box_pt(available_outer_width),
                    IntrinsicInlinePercentageBasisSource::MeasurementAvailableWidth,
                ),
                block_basis: IntrinsicBlockBasis::Indefinite,
            },
            self.base_url,
            self.root_url,
            self.resource_cache,
        ) {
            let geometry = replaced.geometry();
            Some(style.margin.top + geometry.border_box_size.height + style.margin.bottom)
        } else if style.display.is_inline_level() && style.display.is_flow() {
            let text = child_boxes
                .map(inline_text_from_formatting_boxes)
                .unwrap_or_else(|| inline_text_for_style(element, style));
            (!text.is_empty()).then(|| {
                self.estimate_text_height(
                    &text,
                    style,
                    available_outer_width,
                    style.padding.left,
                    style.padding.right,
                )
            })
        } else {
            if style.display.is_table() {
                let built_child_boxes;
                let table_children = if let Some(children) = child_boxes {
                    children
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
                let fragment = box_tree::build_frozen_table_fragment(
                    element,
                    &signature,
                    style,
                    table_children,
                );
                Some(self.estimate_table_height(
                    element,
                    style,
                    stylesheets,
                    available_outer_width,
                    &fragment,
                ))
            } else {
                Some(self.estimate_block_like_height(
                    element,
                    style,
                    stylesheets,
                    available_outer_width,
                    child_boxes,
                ))
            }
        }
    }

    pub(in crate::layout) fn estimate_block_like_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        available_outer_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> f32 {
        let containment = used_property_containment(element, style);
        let mut used_style = self.style_with_current_used_lengths(style);
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
        );
        let style = &used_style;
        let horizontal_extras = box_metrics.horizontal_non_content_length();
        let vertical_extras = box_metrics.vertical_non_content_length();
        // Descendant percentage heights resolve from this box's definite
        // content block-size while it is being estimated. Keep the parent's
        // basis separately: this box's own min/max constraints still resolve
        // against its containing block, never against itself.
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        let containing_block_size_basis = self
            .block_percentage_context_stack
            .current_percentage_basis();
        let definite_content_height = used_content_box_size_with_basis(
            style.box_values.height.value().clone(),
            style.box_sizing,
            containing_block_size_basis,
            vertical_extras,
        );
        self.block_percentage_context_stack.push_context(
            DescendantBlockPercentageContext::formatting_context(
                definite_content_height,
                BlockSizeBasisSource::ContainingBlock,
            ),
        );
        let intrinsic_sizes = (needs_intrinsic_width_contribution(style.box_values.width.clone())
            || needs_intrinsic_width_contribution(style.box_values.min_width.clone())
            || needs_intrinsic_width_contribution(style.box_values.max_width.clone()))
        .then(|| {
            self.block_intrinsic_physical_widths(
                element,
                style,
                stylesheets,
                child_boxes,
                available_outer_width,
            )
        });
        let requested_content_width = if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) = intrinsic_sizes;
            PhysicalContentWidth::new(intrinsic::content_box_width_from_intrinsic(
                style,
                layout_pt(available_outer_width),
                horizontal_extras,
                min_content,
                max_content,
                intrinsic::IntrinsicAutoWidth::FillAvailable,
            ))
        } else {
            self.used_block_physical_content_width(
                element,
                style,
                stylesheets,
                child_boxes,
                BlockContentWidthInputs {
                    available_outer_width: layout_pt(available_outer_width),
                    percentage_basis: PercentageBasis::definite(layout_pt(available_outer_width)),
                    horizontal_non_content: horizontal_extras,
                    definite_content_height: definite_content_height
                        .map(PhysicalContentHeight::new),
                    auto_width_role: BlockAutoWidthRole::NormalFlow,
                },
            )
        };
        let content_width = if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) = intrinsic_sizes;
            constrain_width_with_intrinsic(
                style,
                requested_content_width.content_box_length(),
                min_content,
                max_content,
                PercentageBasis::definite(content_box_pt(available_outer_width)),
                horizontal_extras,
            )
            .points()
        } else {
            constrain_content_width(
                style,
                requested_content_width.content_box_length(),
                PercentageBasis::definite(layout_pt(available_outer_width)),
            )
            .points()
        }
        .max(style.font_size);

        let establishes_multicol = matches!(style.column_count, css::ColumnCount::Count(_))
            || matches!(style.column_width, css::ComputedColumnWidth::Length(_))
            || matches!(style.column_height, css::ComputedColumnHeight::Length(_));
        if establishes_multicol
            && has_auto_height(style)
            && let Some(child_boxes) = child_boxes
            && let Some(mut content_height) = self.estimate_multicol_auto_block_size(
                style,
                stylesheets,
                child_boxes,
                content_width,
            )
        {
            // A multicol formatting context's auto block size is the height of
            // its column rows and spanners. Summing descendant block sizes as
            // ordinary flow double-counts content that has already been
            // distributed among columns, and makes an ancestor fragmentainer
            // manufacture visual-overflow columns.
            // <https://www.w3.org/TR/css-multicol-1/#the-multi-column-model>
            if containment.size {
                content_height = 0.0;
            }
            content_height = constrain_content_height(
                style,
                content_box_pt(
                    used_content_box_height_or_auto(
                        style,
                        layout_pt(content_height),
                        vertical_extras,
                    )
                    .map(SemanticLengthExt::points)
                    .unwrap_or(content_height),
                ),
                PercentageBasis::definite(layout_pt(content_width)),
            )
            .points();
            let borders = used_border_widths(style);
            let outer_height = style.margin.top
                + borders.top
                + style.padding.top
                + content_height
                + style.padding.bottom
                + borders.bottom
                + style.margin.bottom;
            self.block_percentage_context_stack.pop();
            return outer_height;
        }

        let mut content_height = 0.0;
        let mut estimated_float_context = FloatContext { shapes: Vec::new() };
        let mut estimated_float_bottom = 0.0f32;
        if let Some(child_boxes) = child_boxes {
            if !has_non_inline_formatting_box(child_boxes)
                && (has_direct_inline_content_box(child_boxes)
                    || has_atomic_inline_formatting_box(child_boxes))
            {
                // Atomic inline descendants contribute their own used box
                // metrics to the enclosing line. In particular, an
                // inline-block or replaced element can be taller than the
                // parent's line-height; a float's clearance must use that
                // line's actual block size rather than a synthetic strut.
                // <https://www.w3.org/TR/css-inline-3/#line-layout>
                // <https://www.w3.org/TR/CSS22/visuren.html#floats>
                let (inline_height, line_count) = self.intrinsic_inline_block_metrics_for_boxes(
                    child_boxes,
                    style,
                    stylesheets,
                    content_width,
                );
                if line_count > 0 {
                    content_height += inline_height;
                }
            }
            for child_box in child_boxes {
                if let box_tree::FormattingBox::AnonymousBlock(box_) = child_box {
                    content_height += self.estimate_anonymous_block_height(
                        &box_.style,
                        stylesheets,
                        content_width,
                        &box_.children,
                    );
                    continue;
                }
                let Some((child_element, _, child_style, child_children)) =
                    child_box.element_parts()
                else {
                    continue;
                };
                // Positioned and running descendants are out of flow and do
                // not contribute to the containing block's auto height.
                // Floats are measured separately below because a BFC root's
                // auto height includes its float exclusion bounds.
                // <https://www.w3.org/TR/CSS22/visudet.html#root-height>
                if child_style.display.is_none()
                    || matches!(child_style.position, Position::Absolute | Position::Fixed)
                    || child_style.position.is_running()
                {
                    continue;
                }
                if child_style.float != Float::None
                    && let Some(float_bottom) = self.estimate_child_float_bottom(
                        &mut estimated_float_context,
                        child_element,
                        child_style,
                        stylesheets,
                        content_width,
                        Some(child_children),
                    )
                {
                    estimated_float_bottom = estimated_float_bottom.min(float_bottom);
                    continue;
                }
                if (child_style.display.is_block_level()
                    || self
                        .document_canvas_overflow
                        .is_document_canvas_flow_element(element)
                    || is_replaced_element(child_element))
                    && let Some(child_height) = self.estimate_element_height(
                        child_element,
                        child_style,
                        stylesheets,
                        content_width,
                        Some(child_children),
                    )
                {
                    content_height += child_height;
                }
            }
        } else {
            let has_direct_inline_replaced_row = has_direct_inline_replaced_child(element)
                && !has_direct_flow_child_with_font_metrics(
                    element,
                    style,
                    stylesheets,
                    &mut self.font_system,
                );
            let has_direct_flow_child = has_direct_flow_child_with_font_metrics(
                element,
                style,
                stylesheets,
                &mut self.font_system,
            );
            let text = if self
                .document_canvas_overflow
                .is_document_canvas_flow_element(element)
                || has_direct_flow_child
            {
                // CSS 2.2 lays direct block-flow children in block formatting
                // context; their descendant text must not also contribute as a
                // direct anonymous inline line in the parent estimate.
                // <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>
                own_inline_text_for_style(element, style)
            } else {
                inline_text_for_style(element, style)
            };
            if !text.is_empty() {
                // A positioned ancestor's padding box is an absolute
                // containing block, so its provisional auto height must use
                // the same nested inline line metrics as its principal flow.
                // Flattening this DOM subtree to text loses descendant
                // font/line-height and `vertical-align` contributions; that
                // makes an abspos inline's static line disagree with the
                // containing block it is resolved against.
                // <https://drafts.csswg.org/css-inline-3/#line-layout>
                // <https://drafts.csswg.org/css-position-3/#def-cb>
                let (inline_height, line_count) = self.intrinsic_inline_block_metrics_for_element(
                    element,
                    style,
                    stylesheets,
                    None,
                    content_width,
                );
                if line_count > 0 {
                    content_height += inline_height;
                }
            }
            if has_direct_inline_replaced_row {
                let (_, row_height) = self.measure_direct_inline_row(
                    element,
                    style,
                    stylesheets,
                    IntrinsicBlockBasis::Indefinite,
                );
                content_height += row_height;
            }

            let sibling_tags = element_sibling_signature_list(element);
            let mut element_index = 0usize;
            if !has_direct_inline_replaced_row {
                for child in &element.children {
                    let NodeKind::Element(child_element) = &child.kind else {
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
                    if child_style.display.is_none()
                        || matches!(child_style.position, Position::Absolute | Position::Fixed)
                        || child_style.position.is_running()
                    {
                        continue;
                    }
                    if child_style.float != Float::None
                        && let Some(float_bottom) = self.estimate_child_float_bottom(
                            &mut estimated_float_context,
                            child_element,
                            &child_style,
                            stylesheets,
                            content_width,
                            None,
                        )
                    {
                        estimated_float_bottom = estimated_float_bottom.min(float_bottom);
                        continue;
                    }
                    if (child_style.display.is_block_level()
                        || self
                            .document_canvas_overflow
                            .is_document_canvas_flow_element(element)
                        || is_replaced_element(child_element))
                        && let Some(child_height) = self.estimate_element_height(
                            child_element,
                            &child_style,
                            stylesheets,
                            content_width,
                            None,
                        )
                    {
                        content_height += child_height;
                    }
                }
            }
        }
        content_height = content_height.max(-estimated_float_bottom);
        if containment.size {
            // CSS size containment contributes the intrinsic size of an empty
            // principal box while descendants are laid out only after that
            // used size has been fixed.
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            content_height = style
                .contain_intrinsic_size
                .height
                .clone()
                .map(|height| {
                    used_length_percentage(
                        height,
                        PercentageBasis::definite(layout_pt(content_width.max(0.0))),
                    )
                    .points()
                })
                .unwrap_or(0.0);
        }

        let height_depends_on_intrinsic_content =
            needs_intrinsic_height_contribution(style.box_values.height.value().clone())
                || needs_intrinsic_height_contribution(style.box_values.min_height.clone())
                || needs_intrinsic_height_contribution(style.box_values.max_height.clone());
        // Height constraints resolve percentages against the containing
        // block's block-size, not the inline width used to estimate this
        // formatting context.  An auto-height containing block deliberately
        // contributes an indefinite basis, leaving only a fixed calc term.
        // <https://www.w3.org/TR/css-sizing-3/#percentage-sizing>
        let block_size_percentage_basis = containing_block_size_basis;
        if !has_auto_height(style)
            || used_min_height(style, block_size_percentage_basis).is_some()
            || used_max_height(style, block_size_percentage_basis).is_some()
            || height_depends_on_intrinsic_content
        {
            let requested_content_height =
                used_content_box_height_or_auto(style, layout_pt(content_height), vertical_extras)
                    .map(SemanticLengthExt::points)
                    .unwrap_or(content_height);
            content_height = if height_depends_on_intrinsic_content {
                constrain_height_with_intrinsic(
                    style,
                    content_box_pt(requested_content_height),
                    content_box_pt(content_height),
                    content_box_pt(content_height),
                    block_size_percentage_basis,
                    vertical_extras,
                )
                .points()
            } else {
                constrain_content_height(
                    style,
                    content_box_pt(requested_content_height),
                    block_size_percentage_basis,
                )
                .points()
            };
        }

        let borders = used_border_widths(style);
        let outer_height = style.margin.top
            + borders.top
            + style.padding.top
            + content_height
            + style.padding.bottom
            + borders.bottom
            + style.margin.bottom;
        self.block_percentage_context_stack.pop();
        outer_height
    }

    fn estimate_anonymous_block_height(
        &mut self,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        content_width: f32,
        child_boxes: &[box_tree::FormattingBox<'_>],
    ) -> f32 {
        let mut content_height = 0.0;
        let mut estimated_float_context = FloatContext { shapes: Vec::new() };
        let mut estimated_float_bottom = 0.0f32;

        if formatting_box_has_inline_content(child_boxes) {
            content_height += self
                .intrinsic_inline_measurement_for_boxes(
                    child_boxes,
                    style,
                    stylesheets,
                    content_width,
                )
                .height();
        }

        for child_box in child_boxes {
            if let box_tree::FormattingBox::AnonymousBlock(box_) = child_box {
                content_height += self.estimate_anonymous_block_height(
                    &box_.style,
                    stylesheets,
                    content_width,
                    &box_.children,
                );
                continue;
            }

            let Some((child_element, _, child_style, child_children)) = child_box.element_parts()
            else {
                continue;
            };
            if child_style.display.is_none()
                || matches!(child_style.position, Position::Absolute | Position::Fixed)
                || child_style.position.is_running()
            {
                continue;
            }
            if child_style.float != Float::None
                && let Some(float_bottom) = self.estimate_child_float_bottom(
                    &mut estimated_float_context,
                    child_element,
                    child_style,
                    stylesheets,
                    content_width,
                    Some(child_children),
                )
            {
                estimated_float_bottom = estimated_float_bottom.min(float_bottom);
                continue;
            }
            if child_style.display.is_block_level()
                || is_replaced_element(child_element)
                || self
                    .document_canvas_overflow
                    .is_document_canvas_flow_element(child_element)
            {
                if let Some(child_height) = self.estimate_element_height(
                    child_element,
                    child_style,
                    stylesheets,
                    content_width,
                    Some(child_children),
                ) {
                    content_height += child_height;
                }
            } else if matches!(
                child_box,
                box_tree::FormattingBox::InlineSplitBlockContext(_)
                    | box_tree::FormattingBox::Inline(_)
            ) {
                content_height += self.estimate_anonymous_block_height(
                    child_style,
                    stylesheets,
                    content_width,
                    child_children,
                );
            }
        }

        content_height.max(-estimated_float_bottom)
    }

    pub(in crate::layout) fn estimate_child_float_bottom(
        &mut self,
        float_context: &mut FloatContext,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &Stylesheets<'_>,
        containing_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Option<f32> {
        let specified_side = style.float;
        let mut placed_style = style.clone();
        if placed_style.display.is_inline_level() {
            placed_style.display = placed_style.display.blockified();
        }
        if placed_style.display.is_flow() {
            placed_style.display.inner = DisplayInner::FlowRoot;
        }
        self.resolve_style_current_viewport_lengths(&mut placed_style);
        let placement_axes = FloatPlacementAxes::new(
            self.containing_block_writing_mode,
            self.containing_block_direction,
        );
        let float_side = UsedFloatSide::from_float(specified_side, placement_axes)?;
        placed_style.float = Float::None;
        apply_used_box_metrics(
            &mut placed_style,
            PercentageBasis::definite(layout_pt(containing_width)),
        );
        let _ = freeze_float_replay_height(
            &mut placed_style,
            self.block_percentage_context_stack
                .current_percentage_basis(),
            element.document_compatibility_mode == dom::DocumentCompatibilityMode::Quirks,
        );
        let inline_size = self.resolved_float_inline_size(
            element,
            &placed_style,
            stylesheets,
            containing_width,
            child_boxes,
            None,
        );
        freeze_float_replay_width(&mut placed_style, inline_size);
        let width = inline_size.margin_box_width.points();
        let height = self
            .float_margin_box_height(
                element,
                &placed_style,
                stylesheets,
                inline_size,
                child_boxes,
                None,
            )
            .points();
        let placement = float_context.avoiding_position(
            0,
            PageTopBlockPosition::new(0.0),
            margin_box_size_pt(width, height),
            placed_style.clear,
            placement_axes,
            PageInlineSpan::from_edges(0.0, containing_width),
        );
        let band_left = placement.available_span.left_x();
        let top = PageTopBlockPosition::new(placement.origin.top_y());
        let available_width = placement.available_span.width();
        let margin_box_left = match float_side {
            UsedFloatSide::Left => band_left,
            UsedFloatSide::Right => band_left + (available_width - width).max(0.0),
            UsedFloatSide::Top | UsedFloatSide::Bottom => band_left,
        };
        let shape = FloatShape::from_rect(
            FloatId(0),
            specified_side,
            float_side,
            0,
            0,
            PageTopRect::new(margin_box_left, top.points(), width, height),
        );
        float_context.shapes.push(shape);
        Some(top.toward_block_end(layout_pt(height)).points())
    }

    pub(in crate::layout) fn estimate_text_height(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        padding_right: f32,
    ) -> f32 {
        let available_width = (available_width - padding_left - padding_right).max(1.0);
        self.intrinsic_inline_measurement_for_text(text, style, available_width)
            .height()
    }

    pub(in crate::layout) fn estimate_text_physical_height(
        &mut self,
        text: &str,
        style: &ComputedStyle,
        available_width: f32,
        padding_left: f32,
        padding_right: f32,
    ) -> f32 {
        let available_width = (available_width - padding_left - padding_right).max(1.0);
        self.intrinsic_inline_measurement_for_text(text, style, available_width)
            .physical_height(style)
    }

    /// Whether a block's normalized inline contents form one unbreakable line.
    ///
    /// A line box is monolithic in fragmentation. If its block is the only
    /// column-flow subject and even one line cannot fit the fragmentainer, the
    /// line overflows the originating fragment rather than being sliced into
    /// decoration-only continuations:
    /// <https://www.w3.org/TR/css-break-3/#possible-breaks>.
    pub(in crate::layout) fn block_has_single_unbreakable_inline_line(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        children: &[box_tree::FormattingBox<'_>],
        available_width: f32,
    ) -> bool {
        if style.writing_mode != WritingMode::HorizontalTb
            || !formatting_box_has_inline_content(children)
            || has_non_inline_formatting_box(children)
        {
            return false;
        }
        let text = inline_text_for_style(element, style);
        if text.contains('\n') || crate::text::trim_css_collapsible_whitespace(&text).is_empty() {
            return false;
        }
        let metrics =
            used_box_metrics(style, PercentageBasis::definite(layout_pt(available_width)));
        let inline_size = used_content_box_width_or_auto(
            style,
            layout_pt(available_width),
            metrics.horizontal_non_content_length(),
        )
        .map(SemanticLengthExt::points)
        .unwrap_or_else(|| {
            (available_width - metrics.horizontal_non_content_length().points()).max(1.0)
        });
        self.estimate_text_physical_height(&text, style, inline_size, 0.0, 0.0)
            <= style.line_height + 0.01
    }
}
