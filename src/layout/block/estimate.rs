use super::super::*;

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
                    let (_, height) = used_canvas_size(element, style, available_outer_width);
                    Some(style.margin.top + height + style.margin.bottom)
                }
                Some(ReplacedElementKind::Image) => {
                    Some(self.estimate_image_height(element, style, available_outer_width))
                }
                Some(ReplacedElementKind::Svg) => Some(estimate_svg_height(element, style)),
                None if style.display.is_table() || is_html_table_element(element) => {
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
        let box_metrics = apply_used_box_metrics(&mut used_style, available_outer_width.max(0.0));
        let style = &used_style;
        let horizontal_extras = non_content_pt(box_metrics.horizontal_non_content());
        let requested_content_width = self.used_block_content_width(
            element,
            style,
            stylesheets,
            child_boxes,
            BlockContentWidthInputs {
                available_outer_width,
                percentage_basis: available_outer_width,
                horizontal_non_content: horizontal_extras,
            },
        );
        let content_width = constrain_width(
            style,
            requested_content_width.points(),
            available_outer_width,
        )
        .max(style.font_size);

        let mut content_height = 0.0;
        let mut estimated_float_context = FloatContext { shapes: Vec::new() };
        let mut estimated_float_bottom = 0.0f32;
        if let Some(child_boxes) = child_boxes {
            if !has_non_inline_formatting_box(child_boxes)
                && (has_direct_inline_content_box(child_boxes)
                    || has_atomic_inline_formatting_box(child_boxes))
            {
                let text = inline_text_from_formatting_boxes(child_boxes);
                if !text.is_empty() {
                    content_height += self.estimate_text_height(
                        &text,
                        style,
                        content_width,
                        style.padding.left,
                        style.padding.right,
                    );
                } else if has_atomic_inline_formatting_box(child_boxes) {
                    content_height += style.line_height;
                }
            }
            for child_box in child_boxes {
                if let box_tree::FormattingBox::AnonymousBlock(box_) = child_box {
                    let text = inline_text_from_formatting_boxes(&box_.children);
                    if !text.is_empty() || has_atomic_inline_formatting_box(&box_.children) {
                        content_height += self
                            .estimate_text_height(
                                &text,
                                &box_.style,
                                content_width,
                                box_.style.padding.left,
                                box_.style.padding.right,
                            )
                            .max(box_.style.line_height);
                    }
                    continue;
                }
                let Some((child_element, _, child_style, child_children)) =
                    child_box.element_parts()
                else {
                    continue;
                };
                if child_style.float != Float::None
                    && !child_style.display.is_none()
                    && !matches!(child_style.position, Position::Absolute | Position::Fixed)
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
                    if child_style.float != Float::None
                        && !child_style.display.is_none()
                        && !matches!(child_style.position, Position::Absolute | Position::Fixed)
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

        if !has_auto_height(style)
            || used_min_height(style, content_width).is_some()
            || used_max_height(style, content_width).is_some()
        {
            let vertical_extras = box_metrics.vertical_non_content();
            let requested_content_height =
                used_content_height_or_auto(style, content_height, vertical_extras)
                    .unwrap_or(content_height);
            content_height = constrain_height(style, requested_content_height, content_width);
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
        self.resolve_style_current_viewport_lengths(&mut placed_style);
        let float_side = UsedFloatSide::from_float(
            specified_side,
            placed_style.writing_mode,
            placed_style.direction,
        )?;
        placed_style.float = Float::None;
        apply_used_box_metrics(&mut placed_style, containing_width);
        let width = self.float_margin_box_width(
            element,
            &placed_style,
            stylesheets,
            containing_width,
            child_boxes,
            None,
        );
        let height =
            self.float_margin_box_height(element, &placed_style, stylesheets, width, child_boxes);
        let placement = float_context.avoiding_position(
            0,
            0.0,
            width,
            height,
            placed_style.clear,
            placed_style.writing_mode,
            placed_style.direction,
            0.0,
            containing_width,
        );
        let band_left = placement.left();
        let top = placement.top();
        let available_width = placement.available_width();
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
            PageTopRect::new(margin_box_left, top, width, height),
        );
        float_context.shapes.push(shape);
        Some(top - height)
    }

    pub(in crate::layout) fn estimate_image_height(
        &self,
        element: &Element,
        style: &ComputedStyle,
        available_width: f32,
    ) -> f32 {
        let intrinsic_size = element
            .attrs
            .get("src")
            .and_then(|src| {
                load_image_source(src, self.base_url, self.root_url, self.resource_cache)
            })
            .map(|image| {
                let natural_size = image.natural_layout_size();
                (natural_size.width, natural_size.height)
            });
        let (intrinsic_width, intrinsic_height) =
            intrinsic_size.unwrap_or((style.font_size, style.line_height));
        if intrinsic_width <= 0.0 || intrinsic_height <= 0.0 {
            return 0.0;
        }
        let aspect_ratio = intrinsic_width / intrinsic_height;
        let attr_width = element.attrs.get("width").and_then(|value| {
            parse_html_length(value).filter(|width| *width > 0.0 && !value.contains('%'))
        });
        let attr_height = element.attrs.get("height").and_then(|value| {
            parse_html_length(value).filter(|height| *height > 0.0 && !value.contains('%'))
        });
        let mut width =
            used_length_percentage_or_auto(style.box_values.width, available_width).or(attr_width);
        let mut height = style
            .box_values
            .height
            .length_if_no_percent()
            .or(attr_height);
        match (width, height) {
            (Some(width_value), None) => height = Some(width_value / aspect_ratio),
            (None, Some(height_value)) => width = Some(height_value * aspect_ratio),
            (None, None) => {
                width = Some(intrinsic_width);
                height = Some(intrinsic_height);
            }
            (Some(_), Some(_)) => {}
        }
        let width = width.unwrap_or(intrinsic_width).min(available_width);
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
}
