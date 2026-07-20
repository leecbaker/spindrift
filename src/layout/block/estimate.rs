use super::super::*;
use super::float::{freeze_float_replay_height, freeze_float_replay_width};
use crate::LayoutSize;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn estimate_element_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> Option<f32> {
        if style.display.is_none() {
            Some(0.0)
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
            match replaced_element_kind(element) {
                Some(ReplacedElementKind::Canvas) => {
                    let canvas = used_canvas(
                        element,
                        style,
                        available_outer_width,
                        BlockSizePercentageBasis::indefinite(),
                    );
                    Some(style.margin.top + canvas.border_box_size.height + style.margin.bottom)
                }
                Some(ReplacedElementKind::Image) => {
                    Some(self.estimate_image_height(element, style, available_outer_width))
                }
                Some(ReplacedElementKind::Svg) => {
                    Some(estimate_svg_height(element, style, available_outer_width))
                }
                None if style.display.is_table() => {
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
                    let fragment =
                        box_tree::build_frozen_table_fragment(element, &signature, table_children);
                    Some(self.estimate_table_height(
                        element,
                        style,
                        stylesheets,
                        available_outer_width,
                        &fragment,
                    ))
                }
                None => Some(self.estimate_block_like_height(
                    element,
                    style,
                    stylesheets,
                    available_outer_width,
                    child_boxes,
                )),
            }
        }
    }

    pub(in crate::layout) fn estimate_block_like_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_outer_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
    ) -> f32 {
        let mut used_style = self.style_with_current_used_lengths(style);
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_outer_width.max(0.0))),
        );
        let style = &used_style;
        let horizontal_extras = box_metrics.horizontal_non_content_length();
        let intrinsic_sizes = (needs_intrinsic_width_contribution(style.box_values.width.clone())
            || needs_intrinsic_width_contribution(style.box_values.min_width.clone())
            || needs_intrinsic_width_contribution(style.box_values.max_width.clone()))
        .then(|| {
            self.block_intrinsic_content_sizes(
                element,
                style,
                stylesheets,
                child_boxes,
                available_outer_width,
            )
        });
        let requested_content_width = if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) =
                intrinsic_sizes.physical_width_min_max(FlowAxes::for_style(style));
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
                    definite_content_height: None,
                },
            )
        };
        let content_width = if let Some(intrinsic_sizes) = intrinsic_sizes {
            let (min_content, max_content) =
                intrinsic_sizes.physical_width_min_max(FlowAxes::for_style(style));
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

        let establishes_multicol = style.column_count.is_some()
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
            if style.contain.size {
                content_height = 0.0;
            }
            let vertical_extras = box_metrics.vertical_non_content_length();
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
            return style.margin.top
                + borders.top
                + style.padding.top
                + content_height
                + style.padding.bottom
                + borders.bottom
                + style.margin.bottom;
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
                    || child_style.running_element_name.is_some()
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
                    || is_document_canvas_element(element)
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
            let text = if is_document_canvas_element(element) || has_direct_flow_child {
                // CSS 2.2 lays direct block-flow children in block formatting
                // context; their descendant text must not also contribute as a
                // direct anonymous inline line in the parent estimate.
                // <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>
                own_inline_text_for_style(element, style)
            } else {
                inline_text_for_style(element, style)
            };
            if !text.is_empty() {
                content_height += self.estimate_text_height(
                    &text,
                    style,
                    content_width,
                    style.padding.left,
                    style.padding.right,
                );
            }
            if has_direct_inline_replaced_row {
                let (_, row_height) = self.measure_direct_inline_row(element, style, stylesheets);
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
                        || child_style.running_element_name.is_some()
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
                        || is_document_canvas_element(element)
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
        if style.contain.size {
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
            needs_intrinsic_height_contribution(style.box_values.height.clone())
                || needs_intrinsic_height_contribution(style.box_values.min_height.clone())
                || needs_intrinsic_height_contribution(style.box_values.max_height.clone());
        if !has_auto_height(style)
            || used_min_height(style, PercentageBasis::definite(layout_pt(content_width))).is_some()
            || used_max_height(style, PercentageBasis::definite(layout_pt(content_width))).is_some()
            || height_depends_on_intrinsic_content
        {
            let vertical_extras = box_metrics.vertical_non_content_length();
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
                    PercentageBasis::definite(content_box_pt(content_width)),
                    vertical_extras,
                )
                .points()
            } else {
                constrain_content_height(
                    style,
                    content_box_pt(requested_content_height),
                    PercentageBasis::definite(layout_pt(content_width)),
                )
                .points()
            };
        }

        let borders = used_border_widths(style);
        style.margin.top
            + borders.top
            + style.padding.top
            + content_height
            + style.padding.bottom
            + borders.bottom
            + style.margin.bottom
    }

    fn estimate_anonymous_block_height(
        &mut self,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
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
                || child_style.running_element_name.is_some()
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
                || is_document_canvas_element(child_element)
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
        stylesheets: &[Stylesheet],
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
        let float_side = UsedFloatSide::from_float(
            specified_side,
            placed_style.writing_mode,
            placed_style.direction,
        )?;
        placed_style.float = Float::None;
        apply_used_box_metrics(
            &mut placed_style,
            PercentageBasis::definite(layout_pt(containing_width)),
        );
        let _ = freeze_float_replay_height(
            &mut placed_style,
            self.definite_block_size_stack
                .last()
                .cloned()
                .unwrap_or_else(PercentageBasis::indefinite),
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
            placed_style.writing_mode,
            placed_style.direction,
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

    pub(in crate::layout) fn estimate_image_height(
        &self,
        element: &Element,
        style: &ComputedStyle,
        available_width: f32,
    ) -> f32 {
        if let Some(image) = used_image(
            element,
            style,
            available_width,
            BlockSizePercentageBasis::indefinite(),
            self.base_url,
            self.root_url,
            self.resource_cache,
        ) {
            return style.margin.top + image.border_box_size.height + style.margin.bottom;
        }
        let intrinsic_size = element
            .attrs
            .get("src")
            .and_then(|src| {
                load_resolved_image_source(
                    src,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                    style.image_orientation == css::ImageOrientation::FromImage,
                )
            })
            .map(|asset| asset.intrinsic_size());
        let intrinsic_size =
            intrinsic_size.unwrap_or_else(|| LayoutSize::new(style.font_size, style.line_height));
        if intrinsic_size.width <= 0.0 || intrinsic_size.height <= 0.0 {
            return 0.0;
        }
        let aspect_ratio = intrinsic_size.width / intrinsic_size.height;
        let attr_width = element.attrs.get("width").and_then(|value| {
            parse_html_length(value).filter(|width| *width > 0.0 && !value.contains('%'))
        });
        let attr_height = element.attrs.get("height").and_then(|value| {
            parse_html_length(value).filter(|height| *height > 0.0 && !value.contains('%'))
        });
        let mut width = used_length_percentage_or_auto(
            style.box_values.width.clone(),
            PercentageBasis::definite(layout_pt(available_width)),
        )
        .map(|width| width.points())
        .or(attr_width);
        let mut height = style
            .box_values
            .height
            .length_if_no_percent()
            .or(attr_height);
        match (width, height) {
            (Some(width_value), None) => height = Some(width_value / aspect_ratio),
            (None, Some(height_value)) => width = Some(height_value * aspect_ratio),
            (None, None) => {
                width = Some(intrinsic_size.width);
                height = Some(intrinsic_size.height);
            }
            (Some(_), Some(_)) => {}
        }
        let width = width.unwrap_or(intrinsic_size.width).min(available_width);
        let height = height.unwrap_or(width / aspect_ratio);
        style.margin.top + height + style.margin.bottom
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
