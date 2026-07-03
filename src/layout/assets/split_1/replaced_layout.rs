use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn layout_canvas(&mut self, element: &Element, style: &ComputedStyle) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let containing_block_height = self.definite_block_size_stack.last().copied().flatten();
        let (content_width, content_height) = used_canvas_size_with_height_basis(
            element,
            &used_style,
            available_width,
            containing_block_height,
        );
        let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let border_box_width = content_width + box_metrics.horizontal_non_content();
        let border_box_height = content_height + box_metrics.vertical_non_content();

        let (border_x, border_y) =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible
            && (style.background_color.is_some() || used_border_width(style) > 0.0)
        {
            let (rects, rounded_rects, paths, strokes) = block_paint_ops(
                border_x,
                border_y,
                border_box_width,
                border_box_height,
                style,
            );
            for rect in rects {
                self.push_rect_in_band(PaintBand::InFlowBlock, rect);
            }
            for rounded_rect in rounded_rects {
                self.push_rounded_rect_in_band(PaintBand::InFlowBlock, rounded_rect);
            }
            for path in paths {
                self.push_path_in_band(PaintBand::InFlowBlock, path);
            }
            for stroke in strokes {
                self.push_stroke_in_band(PaintBand::InFlowBlock, stroke);
            }
        }
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip::from_paint_rect(paint_space_rect(
                border_x,
                border_y,
                border_box_width,
                border_box_height,
            )),
            style,
            Vec::new(),
        );
        self.cursor_y -= border_box_height + style.margin.bottom;
    }

    pub(in crate::layout) fn layout_image(&mut self, element: &Element, style: &ComputedStyle) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let Some(image) = used_image(
            element,
            style,
            available_width,
            self.base_url,
            self.root_url,
            self.resource_cache,
        ) else {
            return;
        };
        let border_widths = box_metrics.border;
        let content_width = image.content_size.width;
        let content_height = image.content_size.height;
        let border_box_width = image.border_box_size.width;
        let border_box_height = image.border_box_size.height;

        let (border_x, border_y) =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible {
            if style.background_color.is_some() || used_border_width(style) > 0.0 {
                let (rects, rounded_rects, paths, strokes) = block_paint_ops(
                    border_x,
                    border_y,
                    border_box_width,
                    border_box_height,
                    style,
                );
                for rect in rects {
                    self.push_rect_in_band(PaintBand::InFlowBlock, rect);
                }
                for rounded_rect in rounded_rects {
                    self.push_rounded_rect_in_band(PaintBand::InFlowBlock, rounded_rect);
                }
                for path in paths {
                    self.push_path_in_band(PaintBand::InFlowBlock, path);
                }
                self.extend_strokes_in_band(PaintBand::InFlowBlock, strokes);
            }
            let image_x = border_x + border_widths.left + style.padding.left;
            let image_y = border_y + border_widths.bottom + style.padding.bottom;
            if let Some(fill) = solid_opaque_image_fill(&image.decoded) {
                self.push_rect(RenderedRect::from_paint_rect(
                    paint_space_rect(image_x, image_y, content_width, content_height),
                    Some(fill),
                ));
            } else {
                self.push_image(RenderedImage::from_paint_rect(
                    paint_space_rect(image_x, image_y, content_width, content_height),
                    false,
                    image.decoded.pixel_width,
                    image.decoded.pixel_height,
                    None,
                    false,
                    image.decoded.rgb,
                    image.decoded.alpha,
                    element.attrs.get("alt").cloned(),
                ));
            }
        }
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip::from_paint_rect(paint_space_rect(
                border_x,
                border_y,
                border_box_width,
                border_box_height,
            )),
            style,
            Vec::new(),
        );
        self.cursor_y -= border_box_height + style.margin.bottom;
    }

    pub(in crate::layout) fn layout_generated_image(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
    ) {
        let Content::Replacement {
            image: GeneratedContentPart::Image { image },
            ..
        } = &style.content
        else {
            return;
        };
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let image = used_generated_image_value(
            image,
            style,
            available_width,
            self.base_url,
            self.root_url,
            self.resource_cache,
        )
        .unwrap_or_else(|| used_invalid_replacement_image(style, available_width));
        let alt_text = self.generated_alt_text(element, style);
        let border_widths = box_metrics.border;
        let content_width = image.content_size.width;
        let content_height = image.content_size.height;
        let border_box_width = image.border_box_size.width;
        let border_box_height = image.border_box_size.height;
        let (border_x, border_y) =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible {
            if style.background_color.is_some() || used_border_width(style) > 0.0 {
                let (rects, rounded_rects, paths, strokes) = block_paint_ops(
                    border_x,
                    border_y,
                    border_box_width,
                    border_box_height,
                    style,
                );
                for rect in rects {
                    self.push_rect_in_band(PaintBand::InFlowBlock, rect);
                }
                for rounded_rect in rounded_rects {
                    self.push_rounded_rect_in_band(PaintBand::InFlowBlock, rounded_rect);
                }
                for path in paths {
                    self.push_path_in_band(PaintBand::InFlowBlock, path);
                }
                self.extend_strokes_in_band(PaintBand::InFlowBlock, strokes);
            }
            let image_x = border_x + border_widths.left + style.padding.left;
            let image_y = border_y + border_widths.bottom + style.padding.bottom;
            if content_width > 0.0 && content_height > 0.0 {
                self.push_image(RenderedImage::from_paint_rect(
                    paint_space_rect(image_x, image_y, content_width, content_height),
                    false,
                    image.decoded.pixel_width,
                    image.decoded.pixel_height,
                    None,
                    false,
                    image.decoded.rgb,
                    image.decoded.alpha,
                    alt_text,
                ));
            }
        }
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip::from_paint_rect(paint_space_rect(
                border_x,
                border_y,
                border_box_width,
                border_box_height,
            )),
            style,
            Vec::new(),
        );
        self.cursor_y -= border_box_height + style.margin.bottom;
    }

    pub(in crate::layout) fn layout_svg(&mut self, element: &Element, style: &ComputedStyle) {
        let Some((content_width, content_height, fill)) = svg_rect(element) else {
            return;
        };
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let border_widths = box_metrics.border;
        let border_box_width = content_width + box_metrics.horizontal_non_content();
        let border_box_height = content_height + box_metrics.vertical_non_content();
        let (border_x, border_y) =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible
            && (style.background_color.is_some() || used_border_width(style) > 0.0)
        {
            let (rects, rounded_rects, paths, strokes) = block_paint_ops(
                border_x,
                border_y,
                border_box_width,
                border_box_height,
                style,
            );
            for rect in rects {
                self.push_rect_in_band(PaintBand::InFlowBlock, rect);
            }
            for rounded_rect in rounded_rects {
                self.push_rounded_rect_in_band(PaintBand::InFlowBlock, rounded_rect);
            }
            for path in paths {
                self.push_path_in_band(PaintBand::InFlowBlock, path);
            }
            self.extend_strokes_in_band(PaintBand::InFlowBlock, strokes);
        }
        if style.visibility == Visibility::Visible {
            self.push_rect(RenderedRect::from_paint_rect(
                paint_space_rect(
                    border_x + border_widths.left + style.padding.left,
                    border_y + border_widths.bottom + style.padding.bottom,
                    content_width,
                    content_height,
                ),
                Some(fill),
            ));
        }
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip::from_paint_rect(paint_space_rect(
                border_x,
                border_y,
                border_box_width,
                border_box_height,
            )),
            style,
            Vec::new(),
        );
        self.cursor_y -= border_box_height + style.margin.bottom;
    }

    pub(in crate::layout) fn place_block_replaced_box(
        &mut self,
        style: &ComputedStyle,
        border_box_width: f32,
        border_box_height: f32,
    ) -> (f32, f32) {
        let margin_box_width = style.margin.left + border_box_width + style.margin.right;
        let margin_box_height = style.margin.top + border_box_height + style.margin.bottom;
        self.cursor_y -= style.margin.top;
        self.prebreak_bfc_margin_box_if_needed(margin_box_height, style.margin.top);
        let (margin_box_left, avoided_top, _) = self.place_float_avoiding_margin_box(
            self.cursor_y,
            margin_box_width,
            margin_box_height,
            style.clear,
            style.writing_mode,
            style.direction,
            self.containing_block_direction,
        );
        self.cursor_y = avoided_top;
        (
            margin_box_left + style.margin.left,
            self.cursor_y - border_box_height,
        )
    }

    /// Resolves and tiles a single CSS background image layer.
    ///
    /// CSS Backgrounds and Borders positions one tile in the background
    /// positioning area and repeats it according to `background-repeat`:
    /// <https://www.w3.org/TR/css-backgrounds-3/#background-repeat> and
    /// <https://www.w3.org/TR/css-backgrounds-3/#the-background-position>.
    pub(in crate::layout) fn background_images(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        style: &ComputedStyle,
    ) -> Vec<RenderedImage> {
        background_images_for_style(
            BackgroundPaintArea {
                x,
                y,
                width,
                height,
            },
            style,
            self.base_url,
            self.root_url,
            self.resource_cache,
        )
    }

    /// Resolve and paint CSS border-image slices.
    ///
    /// CSS Backgrounds and Borders Level 3 defines `border-image` as a
    /// nine-slice image mapped to the border image area. This first renderer
    /// supports URL sources, numeric/percentage slices, width/outset
    /// resolution, optional center `fill`, and `stretch` sizing:
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-images>.
    pub(in crate::layout) fn border_image_slices(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        style: &ComputedStyle,
    ) -> Vec<RenderedImage> {
        let Some(src) = style.border_image.source.as_ref() else {
            return Vec::new();
        };
        let decoded = match load_image_source(
            src,
            style
                .border_image
                .source_base_url
                .as_deref()
                .or(self.base_url),
            style.border_image.source_root_url.as_deref(),
            self.resource_cache,
        ) {
            Some(decoded) => decoded,
            None => return Vec::new(),
        };
        let slices = used_border_image_slices(
            style.border_image.slice.offsets,
            decoded.pixel_width,
            decoded.pixel_height,
        );
        let borders = used_border_widths(style);
        let outsets = used_border_image_outsets(style, borders);
        let area_width = width + outsets.left + outsets.right;
        let area_height = height + outsets.top + outsets.bottom;
        let image_widths = fit_border_image_widths_to_area(
            used_border_image_widths(style, borders, width, height, slices),
            area_width,
            area_height,
        );
        let area_x = x - outsets.left;
        let area_y = y - outsets.bottom;

        let dest_x = [
            area_x,
            area_x + image_widths.left,
            area_x + area_width - image_widths.right,
        ];
        let dest_width = [
            image_widths.left,
            (area_width - image_widths.left - image_widths.right).max(0.0),
            image_widths.right,
        ];
        let dest_y = [
            area_y,
            area_y + image_widths.bottom,
            area_y + area_height - image_widths.top,
        ];
        let dest_height = [
            image_widths.bottom,
            (area_height - image_widths.top - image_widths.bottom).max(0.0),
            image_widths.top,
        ];
        let source_x = [
            0,
            slices.left,
            decoded.pixel_width.saturating_sub(slices.right),
        ];
        let source_width = [
            slices.left,
            decoded
                .pixel_width
                .saturating_sub(slices.left)
                .saturating_sub(slices.right),
            slices.right,
        ];
        let source_y = [
            decoded.pixel_height.saturating_sub(slices.bottom),
            slices.top,
            0,
        ];
        let source_height = [
            slices.bottom,
            decoded
                .pixel_height
                .saturating_sub(slices.top)
                .saturating_sub(slices.bottom),
            slices.top,
        ];

        let mut images = Vec::new();
        for row in 0..3 {
            for col in 0..3 {
                if row == 1 && col == 1 && !style.border_image.slice.fill {
                    continue;
                }
                if dest_width[col] <= 0.0
                    || dest_height[row] <= 0.0
                    || source_width[col] == 0
                    || source_height[row] == 0
                {
                    continue;
                }
                let repeat_x = if col == 1 {
                    style.border_image.repeat.horizontal
                } else {
                    css::BorderImageRepeatKeyword::Stretch
                };
                let repeat_y = if row == 1 {
                    style.border_image.repeat.vertical
                } else {
                    css::BorderImageRepeatKeyword::Stretch
                };
                push_border_image_tiles(
                    &mut images,
                    &decoded,
                    RenderedImageTileRect::new(
                        dest_x[col],
                        dest_y[row],
                        dest_width[col],
                        dest_height[row],
                    ),
                    RenderedImageSourceRect {
                        x: source_x[col],
                        y: source_y[row],
                        width: source_width[col],
                        height: source_height[row],
                    },
                    repeat_x,
                    repeat_y,
                );
            }
        }
        images
    }

    pub(in crate::layout) fn layout_positioned_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let positioned_layer_start = self.positioned_layers.len();
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_inline_static_position = self.inline_static_position;
        let previous_block_static_position_y_offset = self.block_static_position_y_offset;
        let previous_absolute_static_position = self.absolute_static_position;

        let containing_block = if style.position == Position::Fixed {
            self.page_containing_block()
        } else {
            self.containing_blocks
                .last()
                .copied()
                .unwrap_or_else(|| self.page_containing_block())
        };
        let mut used_style = self.style_with_current_viewport_lengths(style);
        apply_used_box_metrics(&mut used_style, containing_block.width());
        if used_style.display.is_inline_level() {
            // CSS Display blockifies the outer display type of absolutely
            // positioned boxes for layout while preserving the static-position
            // source separately:
            // https://www.w3.org/TR/css-display-3/#transformations
            used_style.display = used_style.display.blockified();
        }
        let positioned_available_outer_width =
            (containing_block.width() - used_style.margin.left - used_style.margin.right)
                .max(used_style.font_size);
        if is_replaced_element(element)
            && let Some(image) = used_image(
                element,
                &used_style,
                positioned_available_outer_width,
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
        {
            // CSS 2.2 gives absolutely positioned replaced elements their own
            // auto-size rules: intrinsic dimensions and aspect ratio resolve
            // the content size before the absolute inset equation is solved.
            // https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-width
            // https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-height
            set_style_used_width(&mut used_style, image.content_size.width);
            set_style_used_height(&mut used_style, image.content_size.height);
        }
        let style = &used_style;
        let left_inset = used_inset_left(style, containing_block);
        let right_inset = used_inset_right(style, containing_block);
        let top_inset = used_inset_top(style, containing_block);
        let bottom_inset = used_inset_bottom(style, containing_block);
        let horizontal_insets_are_auto = left_inset.is_none() && right_inset.is_none();
        let vertical_insets_are_auto = top_inset.is_none() && bottom_inset.is_none();
        let absolute_static_position = self.absolute_static_position;
        let source_static_position = AbsoluteStaticPosition::from_page_rect(
            previous_left,
            previous_right,
            previous_cursor_y,
        );
        let inline_auto_static_y =
            style.abspos_static_source_was_inline_level && vertical_insets_are_auto;
        let inline_static_position = self.inline_static_position;
        let inline_static_uses_margin_box_top = inline_auto_static_y
            && inline_static_position.is_some_and(|position| position.use_margin_box_top);
        let inline_static_baseline_position = (inline_auto_static_y
            && !inline_static_uses_margin_box_top)
            .then_some(inline_static_position)
            .flatten();
        let inline_auto_static_x =
            style.abspos_static_source_was_inline_level && horizontal_insets_are_auto;
        let static_horizontal_position =
            if horizontal_insets_are_auto && let Some(position) = absolute_static_position {
                position.horizontal_position(containing_block)
            } else {
                inline_auto_static_x
                    .then_some(inline_static_position)
                    .flatten()
                    .map(|position| {
                        StaticHorizontalPosition::new(
                            position.start_x - containing_block.x(),
                            containing_block.x() + containing_block.width() - position.end_x,
                        )
                    })
                    .unwrap_or_else(|| source_static_position.horizontal_position(containing_block))
            };
        let static_vertical_base = source_static_position.vertical_start(containing_block);
        let mut static_vertical_start = if vertical_insets_are_auto
            && let Some(position) = absolute_static_position
        {
            position.vertical_start(containing_block)
        } else if inline_static_uses_margin_box_top && let Some(position) = inline_static_position {
            containing_block.top_y() - position.top_y
        } else {
            static_vertical_base.max(0.0)
        };
        if absolute_static_position.is_none()
            && !inline_auto_static_y
            && vertical_insets_are_auto
            && let Some(offset) = self.block_static_position_y_offset
        {
            // CSS 2.2 defines the auto vertical static position from the
            // hypothetical normal-flow box. A block-level abspos appearing
            // after buffered inline content uses the line boxes that would
            // precede that hypothetical block:
            // https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height
            static_vertical_start = static_vertical_base + offset;
        }
        let horizontal_non_content =
            style.padding.left + style.padding.right + horizontal_border_width(style);
        let auto_or_intrinsic_width = self.used_intrinsic_or_shrink_to_fit_width(
            element,
            style,
            stylesheets,
            positioned_available_outer_width,
            horizontal_non_content,
            child_boxes,
            table_fragment,
        );
        let positioned_x = resolve_absolute_horizontal(
            style,
            containing_block,
            auto_or_intrinsic_width,
            static_horizontal_position,
            self.containing_block_direction,
        );
        let positioned_content_width = positioned_x.size;

        let vertical_border_width_for_positioning =
            self.positioned_vertical_border_width(element, style, stylesheets, table_fragment);
        let auto_height = {
            let mut estimate_style = style.clone();
            estimate_style.position = Position::Static;
            estimate_style.margin = css::Edges::ZERO;
            set_style_used_width(&mut estimate_style, positioned_content_width);
            set_style_auto_height(&mut estimate_style);
            clear_position_insets(&mut estimate_style);
            self.measure_auto_positioned_block_height(
                element,
                &estimate_style,
                stylesheets,
                positioned_content_width,
                child_boxes,
                table_fragment,
            )
            .max(style.line_height)
        };
        let positioned_y = resolve_absolute_vertical(
            style,
            containing_block,
            auto_height,
            static_vertical_start,
            vertical_border_width_for_positioning,
        );
        let positioned_content_height = positioned_y.size;
        let positioned_border_box_width = positioned_content_width
            + style.padding.left
            + style.padding.right
            + horizontal_border_width(style);
        self.content_left = containing_block.x() + positioned_x.start + positioned_x.margin_start;
        self.content_right = self.content_left + positioned_border_box_width;
        let positioned_margin_top = if inline_auto_static_y && !inline_static_uses_margin_box_top {
            ((style.line_height - style.font_size) / 2.0).max(0.0)
        } else {
            positioned_y.margin_start
        };
        self.cursor_y = containing_block.top_y() - positioned_y.start - positioned_margin_top;
        let text_only_top_auto_height_position = top_inset.is_some()
            && bottom_inset.is_none()
            && style.box_values.height.is_auto()
            && !is_replaced_element(element)
            && style.background_color.is_none()
            && used_border_width(style) == 0.0
            && style.padding.top == 0.0
            && style.padding.right == 0.0
            && style.padding.bottom == 0.0
            && style.padding.left == 0.0;
        if text_only_top_auto_height_position {
            self.cursor_y += self.inline_text_box_metrics(style, None, 0.0).half_leading;
        }
        if !inline_auto_static_y
            && vertical_insets_are_auto
            && absolute_static_position.is_none()
            && positioned_box_is_orthogonal_to_containing_block(
                self.containing_block_writing_mode,
                style.writing_mode,
            )
        {
            self.cursor_y += positioned_content_height;
        }
        let positioned_border_box_height = positioned_content_height
            + style.padding.top
            + style.padding.bottom
            + vertical_border_width(style);
        let positioned_border_box = PageTopRect::new(
            self.content_left,
            self.cursor_y,
            positioned_border_box_width,
            positioned_border_box_height,
        )
        .paint_clip();
        // CSS Positioned Layout and CSS 2.2 Appendix E order positioned
        // boxes in tree order in their containing stacking context. Reserve
        // this box's order before laying out descendants so child positioned
        // contexts, including fixed descendants, sort after their parent.
        let positioned_source_order = self.next_paint_source_order();

        let mut flow_style = style.clone();
        flow_style.position = Position::Static;
        flow_style.margin = css::Edges::ZERO;
        set_style_used_width(&mut flow_style, positioned_content_width);
        set_style_used_height(&mut flow_style, positioned_content_height);
        clear_position_insets(&mut flow_style);
        let border_widths = used_border_widths(&flow_style);
        self.containing_blocks
            .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                self.content_left + border_widths.left,
                self.cursor_y - border_widths.top,
                positioned_content_width + flow_style.padding.left + flow_style.padding.right,
                positioned_content_height + flow_style.padding.top + flow_style.padding.bottom,
            )));
        self.push_page_name_scope_suppression();
        let previous_overflow_clips = self.overflow_clips.clone();
        self.overflow_clips =
            positioned_applicable_overflow_clips(&previous_overflow_clips, containing_block);
        self.layout_element_inner(
            element,
            &flow_style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
        );
        self.overflow_clips = previous_overflow_clips;
        self.pop_page_name_scope_suppression();
        self.containing_blocks.pop();
        let child_positioned_layers = if positioned_layer_start < self.positioned_layers.len() {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        self.ensure_absolute_positioned_page_span(
            style,
            containing_block,
            positioned_y,
            vertical_border_width_for_positioning,
            paint_page_index,
        );

        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.inline_static_position = previous_inline_static_position;
        self.block_static_position_y_offset = previous_block_static_position_y_offset;
        self.absolute_static_position = previous_absolute_static_position;

        let mut positioned_fragments =
            self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        for (_, positioned_fragment) in &mut positioned_fragments {
            if let (Some(static_position), Some(fragment_baseline_y)) = (
                inline_static_baseline_position,
                positioned_fragment.first_line_y(),
            ) {
                *positioned_fragment = positioned_fragment.clone().translated(PaintVector::new(
                    0.0,
                    static_position.baseline_y - fragment_baseline_y,
                ));
            }
        }
        if positioned_fragments
            .iter()
            .all(|(_, fragment)| fragment.is_empty())
            && child_positioned_layers.is_empty()
        {
            return;
        }
        let child_layers_by_page = child_positioned_layers;
        for (page_index, positioned_fragment) in positioned_fragments {
            let child_layers = child_layers_by_page
                .iter()
                .filter(|layer| layer.page_index == page_index)
                .cloned()
                .collect::<Vec<_>>();
            let mut links = positioned_fragment.links.clone();
            let bounds = positioned_border_box;
            let policy = StackingContextPolicy::for_positioned(element, style, bounds);
            let escaped_atom_translation = if self.escaped_atom_positioning_depth > 0 {
                let containing_block_is_atom_local =
                    self.escaped_atom_containing_block == Some(containing_block);
                EscapedAtomTranslation::from_positioned_static_axes(
                    containing_block,
                    containing_block_is_atom_local
                        || (horizontal_insets_are_auto && absolute_static_position.is_some()),
                    containing_block_is_atom_local
                        || (vertical_insets_are_auto && absolute_static_position.is_some()),
                    !containing_block_is_atom_local,
                )
            } else {
                EscapedAtomTranslation::none()
            };
            let isolates_positioned_descendants =
                policy.is_real_stacking_context && policy.captures_positioned_descendants;
            let child_contexts = if isolates_positioned_descendants {
                for layer in &child_layers {
                    links.extend(layer.links.clone());
                }
                child_layers
                    .iter()
                    .cloned()
                    .map(|layer| layer.context)
                    .collect()
            } else {
                Vec::new()
            };
            if !positioned_fragment.is_empty() || !child_contexts.is_empty() {
                let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                    policy.stack_level,
                    positioned_fragment,
                    child_contexts,
                )
                .with_source_order(positioned_source_order)
                .with_effects(policy.effects)
                .with_bounds(bounds);
                if style.position == Position::Fixed {
                    self.fixed_layers.push(FixedPaintLayer {
                        stack_level: policy.stack_level,
                        context,
                        links,
                    });
                    continue;
                }
                self.positioned_layers.push(PositionedPaintLayer {
                    page_index,
                    stack_level: policy.stack_level,
                    context,
                    links,
                    escaped_atom_translation,
                });
            }
            if !isolates_positioned_descendants {
                self.positioned_layers.extend(child_layers);
            }
        }
    }

    pub(in crate::layout) fn layout_positioned_block_with_inline_static_position(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        static_position: InlineStaticPosition,
    ) {
        let previous = self.inline_static_position;
        self.inline_static_position = Some(static_position);
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
        self.inline_static_position = previous;
    }

    pub(in crate::layout) fn layout_positioned_block_with_block_static_y_offset(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        static_y_offset: f32,
    ) {
        let previous = self.block_static_position_y_offset;
        self.block_static_position_y_offset = Some(static_y_offset);
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
        self.block_static_position_y_offset = previous;
    }

    /// Ensures absolutely positioned boxes that overflow page areas generate pages.
    ///
    /// CSS Positioned Layout makes absolutely positioned boxes out-of-flow, but
    /// CSS Fragmentation still fragments boxes whose margin boxes cross
    /// fragmentainer boundaries. In paged media that means a tall absolutely
    /// positioned box can extend the document to later page boxes even when
    /// those continuation fragments have no visible paint:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn ensure_absolute_positioned_page_span(
        &mut self,
        style: &ComputedStyle,
        containing_block: ContainingBlock,
        positioned_y: PositionedAxis,
        vertical_border_width: f32,
        paint_page_index: usize,
    ) {
        if style.position != Position::Absolute {
            return;
        }
        let page_height = self.page_area_height().max(1.0);
        let margin_box_top = containing_block.top_y() - positioned_y.start;
        let margin_box_height = positioned_y.margin_start
            + positioned_y.size
            + style.padding.top
            + style.padding.bottom
            + vertical_border_width
            + positioned_y.margin_end;
        if margin_box_height <= 0.0 {
            return;
        }
        let margin_box_bottom = margin_box_top - margin_box_height.max(0.0);
        let distance_from_page_top = (self.page_top() - margin_box_bottom).max(0.0);
        if distance_from_page_top <= 0.0 {
            return;
        }
        let target_page_index = paint_page_index
            + ((distance_from_page_top - 0.01).max(0.0) / page_height).floor() as usize;
        while self.pages.len() < target_page_index {
            if !self.current_page_has_content() {
                self.mark_current_page_flow_content();
            }
            self.push_page();
        }
        if self.pages.len() == target_page_index {
            self.mark_current_page_flow_content();
        }
    }

    /// Captures out-of-flow positioned paint fragments from every page touched by layout.
    ///
    /// CSS Positioned Layout takes absolutely positioned boxes out of normal
    /// flow, while CSS Fragmentation still allows their contents to split
    /// across page fragmentainers. Each produced page fragment must therefore
    /// be replayed in the positioned stacking level for that page, not left as
    /// normal-flow paint and not replayed as one page-local fragment:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn take_positioned_fragments_since(
        &mut self,
        paint_page_index: usize,
        paint_checkpoint: PaintCheckpoint,
    ) -> Vec<(usize, PaintFragment)> {
        if self.pages.len() == paint_page_index {
            return vec![(
                paint_page_index,
                self.current_page
                    .take_paint_fragment_since(paint_checkpoint),
            )];
        }

        let mut fragments = Vec::new();
        if let Some(page) = self.pages.get_mut(paint_page_index) {
            fragments.push((
                paint_page_index,
                page.take_paint_fragment_since(paint_checkpoint),
            ));
        }
        for page_index in paint_page_index + 1..self.pages.len() {
            let fragment = self.pages[page_index].take_paint_fragment();
            fragments.push((page_index, fragment));
        }
        fragments.push((self.pages.len(), self.current_page.take_paint_fragment()));
        fragments
    }
}
