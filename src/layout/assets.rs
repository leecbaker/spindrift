use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(super) fn layout_canvas(&mut self, element: &Element, style: &ComputedStyle) {
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

    pub(super) fn layout_image(&mut self, element: &Element, style: &ComputedStyle) {
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

        let (border_x, border_y) =
            self.place_block_replaced_box(style, image.border_box_width, image.border_box_height);
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible {
            if style.background_color.is_some() || used_border_width(style) > 0.0 {
                let (rects, rounded_rects, paths, strokes) = block_paint_ops(
                    border_x,
                    border_y,
                    image.border_box_width,
                    image.border_box_height,
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
                    paint_space_rect(image_x, image_y, image.content_width, image.content_height),
                    Some(fill),
                ));
            } else {
                self.push_image(RenderedImage::from_paint_rect(
                    paint_space_rect(image_x, image_y, image.content_width, image.content_height),
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
                image.border_box_width,
                image.border_box_height,
            )),
            style,
            Vec::new(),
        );
        self.cursor_y -= image.border_box_height + style.margin.bottom;
    }

    pub(super) fn layout_generated_image(&mut self, element: &Element, style: &ComputedStyle) {
        let Content::Replacement {
            image:
                GeneratedContentPart::Image {
                    url,
                    base_url,
                    root_url,
                },
            ..
        } = &style.content
        else {
            return;
        };
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics(&mut used_style, available_width);
        let style = &used_style;
        let image = used_generated_image(
            url,
            style,
            available_width,
            base_url.as_deref(),
            root_url.as_deref(),
            self.resource_cache,
        )
        .unwrap_or_else(|| used_invalid_replacement_image(style, available_width));
        let alt_text = self.generated_alt_text(element, style);
        let border_widths = box_metrics.border;
        let (border_x, border_y) =
            self.place_block_replaced_box(style, image.border_box_width, image.border_box_height);
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible {
            if style.background_color.is_some() || used_border_width(style) > 0.0 {
                let (rects, rounded_rects, paths, strokes) = block_paint_ops(
                    border_x,
                    border_y,
                    image.border_box_width,
                    image.border_box_height,
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
            if image.content_width > 0.0 && image.content_height > 0.0 {
                self.push_image(RenderedImage::from_paint_rect(
                    paint_space_rect(image_x, image_y, image.content_width, image.content_height),
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
                image.border_box_width,
                image.border_box_height,
            )),
            style,
            Vec::new(),
        );
        self.cursor_y -= image.border_box_height + style.margin.bottom;
    }

    pub(super) fn layout_svg(&mut self, element: &Element, style: &ComputedStyle) {
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

    fn place_block_replaced_box(
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
    pub(super) fn background_images(
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
    pub(super) fn border_image_slices(
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

    pub(super) fn layout_positioned_block(
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
        let style = &used_style;
        let positioned_available_outer_width =
            (containing_block.width() - style.margin.left - style.margin.right)
                .max(style.font_size);
        let inline_auto_static_y = style.abspos_static_source_was_inline_level
            && used_inset_top(style, containing_block).is_none()
            && used_inset_bottom(style, containing_block).is_none();
        let inline_static_position = self.inline_static_position;
        let inline_static_baseline_position = inline_auto_static_y
            .then_some(inline_static_position)
            .flatten();
        let inline_auto_static_x = style.abspos_static_source_was_inline_level
            && used_inset_left(style, containing_block).is_none()
            && used_inset_right(style, containing_block).is_none();
        let static_horizontal_start = inline_auto_static_x
            .then_some(inline_static_position)
            .flatten()
            .map(|position| position.start_x - containing_block.x())
            .unwrap_or(previous_left - containing_block.x())
            .max(0.0);
        let static_vertical_base = containing_block.top_y() - previous_cursor_y;
        let mut static_vertical_start = static_vertical_base.max(0.0);
        if !inline_auto_static_y
            && used_inset_top(style, containing_block).is_none()
            && used_inset_bottom(style, containing_block).is_none()
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
            static_horizontal_start,
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
        // CSS Positioned Layout defines the auto inset static-position
        // rectangle from the box's hypothetical normal-flow position. Inline
        // abspos boxes are blockified for layout, but their static position is
        // still the inline margin box; avoid subtracting the start margin a
        // second time until box-tree placeholders carry explicit rectangles.
        let positioned_margin_top = if inline_auto_static_y {
            ((style.line_height - style.font_size) / 2.0).max(0.0)
        } else {
            positioned_y.margin_start
        };
        self.cursor_y = containing_block.top_y() - positioned_y.start - positioned_margin_top;
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
        self.out_of_flow_prebreak_suppression_depth += 1;
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
        self.out_of_flow_prebreak_suppression_depth -= 1;
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
            let policy = StackingContextPolicy::for_positioned(style, bounds);
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
                });
            }
            if !isolates_positioned_descendants {
                self.positioned_layers.extend(child_layers);
            }
        }
    }

    pub(super) fn layout_positioned_block_with_inline_static_position(
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

    pub(super) fn layout_positioned_block_with_block_static_y_offset(
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
    fn ensure_absolute_positioned_page_span(
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
    pub(super) fn take_positioned_fragments_since(
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

    pub(super) fn estimate_shrink_to_fit_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        let (preferred_min, preferred) = self.formatting_context_intrinsic_widths(
            element,
            style,
            stylesheets,
            available_width,
            child_boxes,
            table_fragment,
        );
        intrinsic::shrink_to_fit_width(preferred_min, preferred, available_width)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn used_intrinsic_or_shrink_to_fit_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        horizontal_non_content: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        let content_available_width = (available_width - horizontal_non_content).max(0.0);
        let (preferred_min, preferred) = self.formatting_context_intrinsic_widths(
            element,
            style,
            stylesheets,
            content_available_width,
            child_boxes,
            table_fragment,
        );
        intrinsic::intrinsic_width_keyword(
            style.box_values.width,
            preferred_min,
            preferred,
            available_width,
            horizontal_non_content,
        )
        .unwrap_or_else(|| {
            intrinsic::shrink_to_fit_width(preferred_min, preferred, content_available_width)
        })
    }

    fn formatting_context_intrinsic_widths(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        available_width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> (f32, f32) {
        if style.display.is_flex() {
            let (preferred_min, preferred) = self.estimate_flex_intrinsic_widths(
                element,
                style,
                stylesheets,
                available_width,
                child_boxes,
            );
            return (
                preferred_min.max(0.0),
                preferred.max(preferred_min).max(0.0),
            );
        }
        if style.display.is_table()
            && let Some(fragment) = table_fragment
        {
            let (preferred_min, preferred) = self.table_intrinsic_widths_from_fragment(
                element,
                style,
                stylesheets,
                fragment,
                available_width,
            );
            return (
                preferred_min.max(0.0),
                preferred.max(preferred_min).max(0.0),
            );
        }

        let contribution = self.intrinsic_inline_contribution_for_element(
            element,
            style,
            stylesheets,
            child_boxes,
        );
        let mut preferred = contribution.max_content;
        let mut preferred_min = contribution.min_content;

        if let Some(child_boxes) = child_boxes {
            let mut float_run_width = 0.0f32;
            let mut max_float_run_width = 0.0f32;
            let mut max_float_width = 0.0f32;
            for child_box in child_boxes {
                let Some((child_element, _, child_style, child_children)) =
                    child_box.element_parts()
                else {
                    continue;
                };
                if child_style.float != Float::None {
                    let child_width = self.float_margin_box_width(
                        child_element,
                        child_style,
                        stylesheets,
                        available_width,
                        Some(child_children),
                    );
                    float_run_width += child_width;
                    max_float_run_width = max_float_run_width.max(float_run_width);
                    max_float_width = max_float_width.max(child_width);
                    continue;
                }
                float_run_width = 0.0;
                if child_style.display.is_inline_level() {
                    continue;
                }
                if let box_tree::FormattingBox::Table(table_box) = child_box {
                    let (child_preferred_min, child_preferred) = self
                        .table_outer_intrinsic_widths_from_fragment(
                            table_box.element,
                            child_style,
                            stylesheets,
                            &table_box.fragment,
                            available_width,
                        );
                    preferred = preferred.max(child_preferred);
                    preferred_min = preferred_min.max(child_preferred_min);
                    continue;
                }
                let child_extras = child_style.margin.left
                    + child_style.margin.right
                    + child_style.padding.left
                    + child_style.padding.right
                    + horizontal_border_width(child_style);
                let (intrinsic_preferred_min, intrinsic_preferred) =
                    if child_style.display.is_flex() {
                        self.estimate_flex_intrinsic_widths(
                            child_element,
                            child_style,
                            stylesheets,
                            available_width,
                            Some(child_children),
                        )
                    } else {
                        let contribution = self.intrinsic_inline_contribution_for_element(
                            child_element,
                            child_style,
                            stylesheets,
                            Some(child_children),
                        );
                        (contribution.min_content, contribution.max_content)
                    };
                let child_content_width =
                    used_content_width_or_auto(child_style, available_width, child_extras)
                        .map(|width| constrain_width(child_style, width, available_width));
                let child_preferred = child_content_width.unwrap_or(intrinsic_preferred);
                let child_preferred_min = child_content_width.unwrap_or(intrinsic_preferred_min);
                preferred = preferred.max(child_preferred + child_extras);
                preferred_min = preferred_min.max(child_preferred_min + child_extras);
            }
            preferred = preferred.max(max_float_run_width);
            preferred_min = preferred_min.max(max_float_width);
        } else if preferred <= 0.0 {
            let sibling_tags = element_sibling_tags(element);
            let mut element_index = 0usize;
            let mut float_run_width = 0.0f32;
            let mut max_float_run_width = 0.0f32;
            let mut max_float_width = 0.0f32;
            for child in &element.children {
                let NodeKind::Element(child_element) = &child.kind else {
                    continue;
                };
                let signature = ElementSignature::with_siblings(
                    child_element.tag.clone(),
                    child_element.attrs.clone(),
                    element_index,
                    sibling_tags.clone(),
                );
                element_index += 1;
                let child_style = self.style_for_layout_element_with_parent_font_metrics(
                    child_element,
                    signature,
                    stylesheets,
                    Some(style),
                );
                if child_style.float != Float::None {
                    let child_width = self.float_margin_box_width(
                        child_element,
                        &child_style,
                        stylesheets,
                        available_width,
                        None,
                    );
                    float_run_width += child_width;
                    max_float_run_width = max_float_run_width.max(float_run_width);
                    max_float_width = max_float_width.max(child_width);
                    continue;
                }
                float_run_width = 0.0;
                if child_style.display.is_inline_level() {
                    continue;
                }
                let child_extras = child_style.margin.left
                    + child_style.margin.right
                    + child_style.padding.left
                    + child_style.padding.right
                    + horizontal_border_width(&child_style);
                let (intrinsic_preferred_min, intrinsic_preferred) =
                    if child_style.display.is_flex() {
                        self.estimate_flex_intrinsic_widths(
                            child_element,
                            &child_style,
                            stylesheets,
                            available_width,
                            None,
                        )
                    } else {
                        let contribution = self.intrinsic_inline_contribution_for_element(
                            child_element,
                            &child_style,
                            stylesheets,
                            None,
                        );
                        (contribution.min_content, contribution.max_content)
                    };
                let child_content_width =
                    used_content_width_or_auto(&child_style, available_width, child_extras)
                        .map(|width| constrain_width(&child_style, width, available_width));
                let child_preferred = child_content_width.unwrap_or(intrinsic_preferred);
                let child_preferred_min = child_content_width.unwrap_or(intrinsic_preferred_min);
                preferred = preferred.max(child_preferred + child_extras);
                preferred_min = preferred_min.max(child_preferred_min + child_extras);
            }
            preferred = preferred.max(max_float_run_width);
            preferred_min = preferred_min.max(max_float_width);
        }

        (
            preferred_min.max(0.0),
            preferred.max(preferred_min).max(0.0),
        )
    }

    pub(super) fn measure_auto_positioned_block_height(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        width: f32,
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        let vertical_border_width_for_positioning =
            self.positioned_vertical_border_width(element, style, stylesheets, table_fragment);
        let snapshot = self.snapshot();
        self.content_left = 0.0;
        self.content_right = width.max(style.font_size);
        self.cursor_y = self.page_bottom() + 10_000.0;
        let start_y = self.cursor_y;
        self.containing_blocks
            .push(ContainingBlock::from_page_top_rect(PageTopRect::new(
                self.content_left,
                self.cursor_y,
                self.content_right - self.content_left,
                10_000.0,
            )));
        self.layout_element_inner(
            element,
            style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
        );
        self.containing_blocks.pop();
        let consumed = (start_y - self.cursor_y).max(0.0);
        self.restore(snapshot);
        // CSS 2.2 absolute positioning equations use content height as the
        // `height` term and add padding/borders separately. Collapsed table
        // borders contribute resolved outer grid insets rather than authored
        // full border widths, so use the same vertical non-content size that
        // will be used by the absolute-position equation.
        (consumed
            - style.padding.top
            - style.padding.bottom
            - vertical_border_width_for_positioning)
            .max(0.0)
    }

    fn positioned_vertical_border_width(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) -> f32 {
        if is_html_table_element(element) {
            self.collapsed_table_outer_vertical_insets(style, stylesheets, table_fragment)
                .unwrap_or_else(|| vertical_border_width(style))
        } else {
            vertical_border_width(style)
        }
    }

    pub(super) fn page_containing_block(&self) -> ContainingBlock {
        ContainingBlock::from_page_top_rect(PageTopRect::new(
            self.page_left(),
            self.page_top(),
            self.page_area_width(),
            self.page_area_height(),
        ))
    }

    pub(super) fn current_containing_block(&self) -> ContainingBlock {
        self.containing_blocks
            .last()
            .copied()
            .unwrap_or_else(|| self.page_containing_block())
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BackgroundPaintArea {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

/// Resolves and tiles a CSS background image layer for any box-like area.
///
/// CSS Backgrounds and Borders defines background image sizing, positioning,
/// and repetition independently of the formatting context that produced the
/// box. This shared helper is used by document boxes, page boxes, and
/// page-margin boxes so generated page content paints backgrounds with the
/// same semantics as normal elements:
/// <https://www.w3.org/TR/css-backgrounds-3/#backgrounds>.
pub(super) fn background_images_for_style(
    area: BackgroundPaintArea,
    style: &ComputedStyle,
    fallback_base_url: Option<&Path>,
    fallback_root_url: Option<&Path>,
    resource_cache: &ResourceCache,
) -> Vec<RenderedImage> {
    let mut images = Vec::new();
    for layer in background_layers_for_paint(style).iter().rev() {
        let positioning_area = background_paint_area_for_box(area, style, layer.origin);
        let clip_area = background_paint_area_for_box(area, style, layer.clip);
        let Some(BackgroundImage::Url {
            src,
            base_url,
            root_url,
        }) = layer.image.as_ref()
        else {
            continue;
        };
        let Some(decoded) = load_image_source(
            src.as_str(),
            base_url.as_deref().or(fallback_base_url),
            root_url.as_deref().or(fallback_root_url),
            resource_cache,
        ) else {
            continue;
        };
        let (image_width, image_height) = used_background_size(
            &decoded,
            positioning_area.width,
            positioning_area.height,
            layer.size,
        );
        if image_width <= 0.0 || image_height <= 0.0 {
            continue;
        }
        let (offset_x, offset_y) = background_position(
            layer.position,
            positioning_area.width,
            positioning_area.height,
            image_width,
            image_height,
        );
        let tile_xs = background_tile_positions(
            positioning_area.x + offset_x,
            positioning_area.x,
            positioning_area.width,
            image_width,
            layer.repeat.repeats_x(),
        );
        let tile_ys = background_tile_positions(
            positioning_area.y + offset_y,
            positioning_area.y,
            positioning_area.height,
            image_height,
            layer.repeat.repeats_y(),
        );
        for tile_y in tile_ys {
            for tile_x in &tile_xs {
                let image = RenderedImage::from_paint_rect(
                    paint_space_rect(*tile_x, tile_y, image_width, image_height),
                    true,
                    decoded.pixel_width,
                    decoded.pixel_height,
                    None,
                    true,
                    decoded.rgb.clone(),
                    decoded.alpha.clone(),
                    None,
                );
                if let Some(image) = clip_background_image_to_area(image, clip_area) {
                    images.push(image);
                }
            }
        }
    }
    images
}

fn background_layers_for_paint(style: &ComputedStyle) -> Vec<css::BackgroundLayer> {
    if !style.background_layers.is_empty() {
        return style.background_layers.clone();
    }
    vec![css::BackgroundLayer {
        image: style.background_image.clone(),
        position: style.background_position,
        size: style.background_size,
        repeat: style.background_repeat,
        origin: style.background_origin,
        clip: style.background_clip,
    }]
}

fn background_paint_area_for_box(
    area: BackgroundPaintArea,
    style: &ComputedStyle,
    box_: css::BackgroundBox,
) -> BackgroundPaintArea {
    let border = used_border_widths(style);
    match box_ {
        css::BackgroundBox::Border => area,
        css::BackgroundBox::Padding => area.inset(border),
        css::BackgroundBox::Content => area.inset(border).inset(style.padding),
    }
}

fn clip_background_image_to_area(
    mut image: RenderedImage,
    clip: BackgroundPaintArea,
) -> Option<RenderedImage> {
    let image_x = image.x();
    let image_y = image.y();
    let image_width = image.width();
    let image_height = image.height();
    let x1 = image_x.max(clip.x);
    let y1 = image_y.max(clip.y);
    let x2 = (image_x + image_width).min(clip.x + clip.width);
    let y2 = (image_y + image_height).min(clip.y + clip.height);
    if x2 <= x1 || y2 <= y1 || image_width <= 0.0 || image_height <= 0.0 {
        return None;
    }
    let source = image.source_rect.unwrap_or(RenderedImageSourceRect {
        x: 0,
        y: 0,
        width: image.pixel_width,
        height: image.pixel_height,
    });
    let source_x = source.x as f32 + ((x1 - image_x) / image_width) * source.width as f32;
    let source_y = source.y as f32 + ((y1 - image_y) / image_height) * source.height as f32;
    let source_width = ((x2 - x1) / image_width) * source.width as f32;
    let source_height = ((y2 - y1) / image_height) * source.height as f32;
    image.set_paint_rect(paint_space_rect(x1, y1, x2 - x1, y2 - y1));
    image.source_rect = Some(RenderedImageSourceRect {
        x: source_x.floor().max(0.0) as u32,
        y: source_y.floor().max(0.0) as u32,
        width: source_width.ceil().max(1.0) as u32,
        height: source_height.ceil().max(1.0) as u32,
    });
    Some(image)
}

impl BackgroundPaintArea {
    fn inset(self, edges: css::Edges) -> Self {
        Self {
            x: self.x + edges.left,
            y: self.y + edges.bottom,
            width: (self.width - edges.left - edges.right).max(0.0),
            height: (self.height - edges.top - edges.bottom).max(0.0),
        }
    }
}

fn clear_position_insets(style: &mut ComputedStyle) {
    clear_style_insets(style);
}

#[derive(Debug, Clone, Copy)]
struct RenderedImageTileRect {
    /// Destination tile region in page-local CSS paint coordinates.
    ///
    /// CSS Backgrounds and Borders slices `border-image` into destination
    /// regions that are painted into the border-image area. At this stage the
    /// layout box has already been projected into paint space, so the rectangle
    /// uses the same bottom-left-origin coordinate system as rendered images:
    /// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
    rect: PaintRect,
}

impl RenderedImageTileRect {
    fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            rect: paint_space_rect(x, y, width, height),
        }
    }

    fn x(self) -> f32 {
        self.rect.origin.x
    }

    fn y(self) -> f32 {
        self.rect.origin.y
    }

    fn width(self) -> f32 {
        self.rect.size.width
    }

    fn height(self) -> f32 {
        self.rect.size.height
    }
}

#[derive(Debug, Clone, Copy)]
struct BorderImageTileSegment {
    destination_offset: f32,
    destination_size: f32,
    source_offset: u32,
    source_size: u32,
}

/// Emits the repeated image tiles for one border-image slice region.
///
/// CSS Backgrounds and Borders Level 3 applies `border-image-repeat` after the
/// source image has been sliced into a 3x3 grid. Corners are stretched, edge
/// regions repeat only along their long axis, and the optional center region
/// repeats on both axes:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-process>.
fn push_border_image_tiles(
    images: &mut Vec<RenderedImage>,
    decoded: &DecodedPngImage,
    destination: RenderedImageTileRect,
    source: RenderedImageSourceRect,
    repeat_x: css::BorderImageRepeatKeyword,
    repeat_y: css::BorderImageRepeatKeyword,
) {
    let (tile_width, tile_height) =
        border_image_base_tile_size(destination, source, repeat_x, repeat_y);
    let x_segments =
        border_image_tile_segments(repeat_x, destination.width(), tile_width, source.width);
    let y_segments =
        border_image_tile_segments(repeat_y, destination.height(), tile_height, source.height);
    for y_segment in &y_segments {
        for x_segment in &x_segments {
            if x_segment.destination_size <= 0.0
                || y_segment.destination_size <= 0.0
                || x_segment.source_size == 0
                || y_segment.source_size == 0
            {
                continue;
            }
            images.push(RenderedImage::from_paint_rect(
                paint_space_rect(
                    destination.x() + x_segment.destination_offset,
                    destination.y() + y_segment.destination_offset,
                    x_segment.destination_size,
                    y_segment.destination_size,
                ),
                true,
                decoded.pixel_width,
                decoded.pixel_height,
                Some(RenderedImageSourceRect {
                    x: source.x + x_segment.source_offset,
                    y: source.y + y_segment.source_offset,
                    width: x_segment.source_size,
                    height: y_segment.source_size,
                }),
                true,
                decoded.rgb.clone(),
                decoded.alpha.clone(),
                None,
            ));
        }
    }
}

fn border_image_base_tile_size(
    destination: RenderedImageTileRect,
    source: RenderedImageSourceRect,
    repeat_x: css::BorderImageRepeatKeyword,
    repeat_y: css::BorderImageRepeatKeyword,
) -> (f32, f32) {
    let mut tile_width = source.width as f32;
    let mut tile_height = source.height as f32;
    if repeat_x != css::BorderImageRepeatKeyword::Stretch
        && repeat_y == css::BorderImageRepeatKeyword::Stretch
        && source.height > 0
    {
        let scale = destination.height() / source.height as f32;
        tile_width *= scale;
    }
    if repeat_y != css::BorderImageRepeatKeyword::Stretch
        && repeat_x == css::BorderImageRepeatKeyword::Stretch
        && source.width > 0
    {
        let scale = destination.width() / source.width as f32;
        tile_height *= scale;
    }
    if repeat_x == css::BorderImageRepeatKeyword::Stretch {
        tile_width = destination.width();
    }
    if repeat_y == css::BorderImageRepeatKeyword::Stretch {
        tile_height = destination.height();
    }
    (tile_width.max(0.0), tile_height.max(0.0))
}

/// Computes destination/source segments for one `border-image-repeat` axis.
///
/// The CSS border-image process defines four repeat modes: `stretch` scales one
/// image to the region, `repeat` clips repeated tiles at the ends, `round`
/// adjusts the tile size to fit an integer number of tiles, and `space`
/// distributes whole tiles with gaps:
/// <https://www.w3.org/TR/css-backgrounds-3/#border-image-repeat>.
fn border_image_tile_segments(
    repeat: css::BorderImageRepeatKeyword,
    destination_size: f32,
    base_tile_size: f32,
    source_size: u32,
) -> Vec<BorderImageTileSegment> {
    if destination_size <= 0.0 || source_size == 0 {
        return Vec::new();
    }
    if repeat == css::BorderImageRepeatKeyword::Stretch || base_tile_size <= 0.0 {
        return vec![BorderImageTileSegment {
            destination_offset: 0.0,
            destination_size,
            source_offset: 0,
            source_size,
        }];
    }
    match repeat {
        css::BorderImageRepeatKeyword::Repeat => {
            repeat_border_image_tile_segments(destination_size, base_tile_size, source_size)
        }
        css::BorderImageRepeatKeyword::Round => {
            let count = (destination_size / base_tile_size).round().max(1.0) as usize;
            let tile_size = destination_size / count as f32;
            (0..count)
                .map(|index| BorderImageTileSegment {
                    destination_offset: index as f32 * tile_size,
                    destination_size: tile_size,
                    source_offset: 0,
                    source_size,
                })
                .collect()
        }
        css::BorderImageRepeatKeyword::Space => {
            let count = (destination_size / base_tile_size).floor() as usize;
            if count <= 1 {
                let tile_size = base_tile_size.min(destination_size);
                return vec![BorderImageTileSegment {
                    destination_offset: (destination_size - tile_size) / 2.0,
                    destination_size: tile_size,
                    source_offset: 0,
                    source_size,
                }];
            }
            let spacing = (destination_size - base_tile_size * count as f32) / (count - 1) as f32;
            (0..count)
                .map(|index| BorderImageTileSegment {
                    destination_offset: index as f32 * (base_tile_size + spacing),
                    destination_size: base_tile_size,
                    source_offset: 0,
                    source_size,
                })
                .collect()
        }
        css::BorderImageRepeatKeyword::Stretch => unreachable!(),
    }
}

fn repeat_border_image_tile_segments(
    destination_size: f32,
    tile_size: f32,
    source_size: u32,
) -> Vec<BorderImageTileSegment> {
    let mut segments = Vec::new();
    let mut offset = 0.0;
    while offset < destination_size - f32::EPSILON {
        let visible_size = tile_size.min(destination_size - offset);
        let source_visible = ((source_size as f32) * (visible_size / tile_size))
            .round()
            .clamp(1.0, source_size as f32) as u32;
        segments.push(BorderImageTileSegment {
            destination_offset: offset,
            destination_size: visible_size,
            source_offset: 0,
            source_size: source_visible,
        });
        offset += tile_size;
    }
    segments
}

#[derive(Debug, Clone, Copy)]
struct PositionedAxis {
    start: f32,
    size: f32,
    margin_start: f32,
    margin_end: f32,
}

impl PositionedAxis {
    fn new(start: f32, size: f32, margin_start: f32, margin_end: f32) -> Self {
        Self {
            start,
            size,
            margin_start,
            margin_end,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AbsoluteAxisDirection {
    HorizontalLtr,
    HorizontalRtl,
    Vertical,
}

#[derive(Debug, Clone, Copy)]
struct AbsoluteDefiniteAxis {
    start: f32,
    size: f32,
    end: f32,
    margin_start: f32,
    margin_end: f32,
    non_content: f32,
    containing_size: f32,
}

/// Resolve auto margins for a fully definite absolutely positioned axis.
///
/// CSS 2.2 defines absolute-position sizing by a constraint equation over
/// start inset, margins, padding, borders, content size, and end inset. Auto
/// margins remain zero for the other non-replaced absolute-position cases, but
/// when both insets and the used size are definite, auto margins absorb the
/// equation's remaining space before overconstraint handling:
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-width> and
/// <https://www.w3.org/TR/CSS22/visudet.html#abs-non-replaced-height>.
fn resolve_absolute_definite_axis_auto_margins(
    start_auto: bool,
    end_auto: bool,
    axis: AbsoluteDefiniteAxis,
    direction: AbsoluteAxisDirection,
) -> PositionedAxis {
    let remaining = axis.containing_size
        - axis.start
        - axis.margin_start
        - axis.non_content
        - axis.size
        - axis.margin_end
        - axis.end;

    match (start_auto, end_auto) {
        (true, true) => {
            if matches!(direction, AbsoluteAxisDirection::HorizontalLtr) && remaining < 0.0 {
                return PositionedAxis::new(axis.start, axis.size, 0.0, remaining);
            }
            if matches!(direction, AbsoluteAxisDirection::HorizontalRtl) && remaining < 0.0 {
                return PositionedAxis::new(axis.start, axis.size, remaining, 0.0);
            }
            PositionedAxis::new(
                axis.start,
                axis.size,
                axis.margin_start + remaining / 2.0,
                axis.margin_end + remaining / 2.0,
            )
        }
        (true, false) => PositionedAxis::new(
            axis.start,
            axis.size,
            axis.margin_start + remaining,
            axis.margin_end,
        ),
        (false, true) => PositionedAxis::new(
            axis.start,
            axis.size,
            axis.margin_start,
            axis.margin_end + remaining,
        ),
        (false, false) => match direction {
            AbsoluteAxisDirection::HorizontalRtl => PositionedAxis::new(
                axis.containing_size
                    - axis.end
                    - axis.margin_start
                    - axis.margin_end
                    - axis.non_content
                    - axis.size,
                axis.size,
                axis.margin_start,
                axis.margin_end,
            ),
            AbsoluteAxisDirection::HorizontalLtr | AbsoluteAxisDirection::Vertical => {
                PositionedAxis::new(axis.start, axis.size, axis.margin_start, axis.margin_end)
            }
        },
    }
}

/// Returns tile origins that intersect a background positioning area.
///
/// CSS Backgrounds and Borders repeats from the positioned first tile in both
/// directions as needed, but PDF emission needs a finite set of image
/// placements for the current painted area:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-repeat>.
fn background_tile_positions(
    positioned_start: f32,
    area_start: f32,
    area_size: f32,
    tile_size: f32,
    repeats: bool,
) -> Vec<f32> {
    if area_size <= 0.0 || tile_size <= 0.0 {
        return Vec::new();
    }
    if !repeats {
        return vec![positioned_start];
    }

    let area_end = area_start + area_size;
    let mut first = positioned_start;
    while first > area_start {
        first -= tile_size;
    }
    while first + tile_size <= area_start {
        first += tile_size;
    }

    let mut positions = Vec::new();
    let mut current = first;
    while current < area_end {
        positions.push(current);
        current += tile_size;
    }
    positions
}

fn resolve_absolute_horizontal(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_or_intrinsic_width: f32,
    static_start: f32,
    containing_direction: Direction,
) -> PositionedAxis {
    // CSS 2.1 10.3.7, non-replaced absolutely positioned elements. Static
    // position comes from the hypothetical normal-flow rectangle for
    // placeholder-backed inline sources, and otherwise falls back to the
    // layout cursor/content edge at the element's source position.
    let left = used_inset_left(style, containing_block);
    let right = used_inset_right(style, containing_block);
    let width = used_content_width_or_auto(
        style,
        containing_block.width(),
        style.padding.left + style.padding.right + horizontal_border_width(style),
    )
    .or_else(|| {
        matches!(
            style.box_values.width,
            css::ComputedLengthPercentageOrAuto::MinContent
                | css::ComputedLengthPercentageOrAuto::MaxContent
                | css::ComputedLengthPercentageOrAuto::FitContent(_)
        )
        .then_some(auto_or_intrinsic_width)
    })
    .map(|width| constrain_width(style, width, containing_block.width()));
    let shrink_to_fit_width =
        constrain_width(style, auto_or_intrinsic_width, containing_block.width());
    let static_start = static_start.clamp(0.0, containing_block.width());
    let margin_start = style.margin.left;
    let margin_end = style.margin.right;
    let non_content = style.padding.left + style.padding.right + horizontal_border_width(style);
    let fill_between = |start: f32, end: f32| {
        (containing_block.width() - start - margin_start - non_content - margin_end - end).max(0.0)
    };
    let border_box_size = |content_size: f32| content_size + non_content;
    let start_for_end = |content_size: f32, end: f32| {
        containing_block.width() - end - margin_start - margin_end - border_box_size(content_size)
    };

    match (left, width, right) {
        (Some(start), Some(size), Some(end)) => match containing_direction {
            Direction::Ltr => resolve_absolute_definite_axis_auto_margins(
                style.box_values.margin.left.is_auto(),
                style.box_values.margin.right.is_auto(),
                AbsoluteDefiniteAxis {
                    start,
                    size,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: containing_block.width(),
                },
                AbsoluteAxisDirection::HorizontalLtr,
            ),
            Direction::Rtl => resolve_absolute_definite_axis_auto_margins(
                style.box_values.margin.left.is_auto(),
                style.box_values.margin.right.is_auto(),
                AbsoluteDefiniteAxis {
                    start,
                    size,
                    end,
                    margin_start,
                    margin_end,
                    non_content,
                    containing_size: containing_block.width(),
                },
                AbsoluteAxisDirection::HorizontalRtl,
            ),
        },
        (Some(start), Some(size), None) => {
            PositionedAxis::new(start, size, margin_start, margin_end)
        }
        (Some(start), None, Some(end)) => PositionedAxis::new(
            start,
            constrain_width(style, fill_between(start, end), containing_block.width()),
            margin_start,
            margin_end,
        ),
        (Some(start), None, None) => {
            PositionedAxis::new(start, shrink_to_fit_width, margin_start, margin_end)
        }
        (None, Some(size), Some(end)) => {
            PositionedAxis::new(start_for_end(size, end), size, margin_start, margin_end)
        }
        (None, Some(size), None) => {
            PositionedAxis::new(static_start, size, margin_start, margin_end)
        }
        (None, None, Some(end)) => PositionedAxis::new(
            start_for_end(shrink_to_fit_width, end),
            shrink_to_fit_width,
            margin_start,
            margin_end,
        ),
        (None, None, None) => {
            PositionedAxis::new(static_start, shrink_to_fit_width, margin_start, margin_end)
        }
    }
}

fn resolve_absolute_vertical(
    style: &ComputedStyle,
    containing_block: ContainingBlock,
    auto_height: f32,
    static_start: f32,
    vertical_border_width: f32,
) -> PositionedAxis {
    // CSS 2.1 10.6.4, non-replaced absolutely positioned elements. Static
    // position is approximated from the layout cursor at the element's source
    // position until layout carries explicit placeholders.
    let top = used_inset_top(style, containing_block);
    let bottom = used_inset_bottom(style, containing_block);
    let height = used_content_height_or_auto(
        style,
        containing_block.height(),
        style.padding.top + style.padding.bottom + vertical_border_width,
    )
    .map(|height| constrain_height(style, height, containing_block.height()));
    let auto_height = constrain_height(style, auto_height, containing_block.height());
    // CSS 2.2 defines the static position as the hypothetical normal-flow
    // position. It can fall outside the containing block, especially while a
    // nested formatting context is measured in temporary coordinates.
    let margin_start = style.margin.top;
    let margin_end = style.margin.bottom;
    let non_content = style.padding.top + style.padding.bottom + vertical_border_width;
    let fill_between = |start: f32, end: f32| {
        (containing_block.height() - start - margin_start - non_content - margin_end - end).max(0.0)
    };
    let border_box_size = |content_size: f32| content_size + non_content;
    let start_for_end = |content_size: f32, end: f32| {
        containing_block.height() - end - margin_start - margin_end - border_box_size(content_size)
    };

    match (top, height, bottom) {
        (Some(start), Some(size), Some(end)) => resolve_absolute_definite_axis_auto_margins(
            style.box_values.margin.top.is_auto(),
            style.box_values.margin.bottom.is_auto(),
            AbsoluteDefiniteAxis {
                start,
                size,
                end,
                margin_start,
                margin_end,
                non_content,
                containing_size: containing_block.height(),
            },
            AbsoluteAxisDirection::Vertical,
        ),
        (Some(start), Some(size), None) => {
            PositionedAxis::new(start, size, margin_start, margin_end)
        }
        (Some(start), None, Some(end)) => PositionedAxis::new(
            start,
            constrain_height(style, fill_between(start, end), containing_block.height()),
            margin_start,
            margin_end,
        ),
        (Some(start), None, None) => {
            PositionedAxis::new(start, auto_height, margin_start, margin_end)
        }
        (None, Some(size), Some(end)) => {
            PositionedAxis::new(start_for_end(size, end), size, margin_start, margin_end)
        }
        (None, Some(size), None) => {
            PositionedAxis::new(static_start, size, margin_start, margin_end)
        }
        (None, None, Some(end)) => PositionedAxis::new(
            start_for_end(auto_height, end),
            auto_height,
            margin_start,
            margin_end,
        ),
        (None, None, None) => {
            PositionedAxis::new(static_start, auto_height, margin_start, margin_end)
        }
    }
}

/// Returns the fill color for a decoded image that is exactly one opaque color.
///
/// CSS Images paints replaced raster content into the element's concrete object
/// size. When every source pixel is the same opaque color, a filled PDF
/// rectangle is visually equivalent and avoids raster-image boundary
/// antialiasing seams at adjacent same-color edges:
/// <https://www.w3.org/TR/css-images-3/#concrete-object-size> and
/// ISO 32000-1:2008 section 8.9.
fn solid_opaque_image_fill(image: &DecodedPngImage) -> Option<Color> {
    if image.pixel_width <= 1 && image.pixel_height <= 1 {
        return None;
    }
    if image.alpha.is_some() || image.rgb.len() < 3 {
        return None;
    }
    let first = &image.rgb[..3];
    image
        .rgb
        .chunks_exact(3)
        .all(|pixel| pixel == first)
        .then(|| Color::new(first[0], first[1], first[2]))
}

pub(super) fn paint_effects_for_box(style: &ComputedStyle, border_box: PaintClip) -> PaintEffects {
    let borders = used_border_widths(style);
    PaintEffects {
        opacity: style.opacity,
        transform: paint_transform_for_box(style, border_box),
        overflow_clip: style
            .overflow
            .clips_overflow()
            .then_some(PaintClip::from_paint_rect(paint_space_rect(
                border_box.x() + borders.left,
                border_box.y() + borders.bottom,
                border_box.width() - borders.left - borders.right,
                border_box.height() - borders.top - borders.bottom,
            ))),
        absolute_clip: None,
        clip_path: paint_clip_path_effect(style),
        mask: paint_mask_effect(style),
        filter: paint_filter_effect(style),
        blend_mode: paint_blend_mode(style.mix_blend_mode),
        isolation: style.isolation == Isolation::Isolate || style.will_change.isolation,
    }
}

fn paint_clip_path_effect(style: &ComputedStyle) -> PaintClipPathEffect {
    match style.clip_path {
        ClipPath::None if style.will_change.clip_path => PaintClipPathEffect::WillChange,
        ClipPath::None => PaintClipPathEffect::None,
        ClipPath::Inset => PaintClipPathEffect::Inset,
        ClipPath::Shape => PaintClipPathEffect::Shape,
        ClipPath::Url => PaintClipPathEffect::Url,
    }
}

fn paint_mask_effect(style: &ComputedStyle) -> PaintMaskEffect {
    if !matches!(style.mask, MaskValue::None) {
        PaintMaskEffect::MaskImage
    } else if style.will_change.mask {
        PaintMaskEffect::WillChange
    } else {
        PaintMaskEffect::None
    }
}

fn paint_filter_effect(style: &ComputedStyle) -> PaintFilterEffect {
    if !matches!(style.filter, FilterValue::None) {
        PaintFilterEffect::FilterList
    } else if style.will_change.filter {
        PaintFilterEffect::WillChange
    } else {
        PaintFilterEffect::None
    }
}

fn paint_blend_mode(mode: MixBlendMode) -> PaintBlendMode {
    match mode {
        MixBlendMode::Normal => PaintBlendMode::Normal,
        MixBlendMode::Multiply => PaintBlendMode::Multiply,
        MixBlendMode::Screen => PaintBlendMode::Screen,
        MixBlendMode::Overlay => PaintBlendMode::Overlay,
        MixBlendMode::Darken => PaintBlendMode::Darken,
        MixBlendMode::Lighten => PaintBlendMode::Lighten,
        MixBlendMode::ColorDodge => PaintBlendMode::ColorDodge,
        MixBlendMode::ColorBurn => PaintBlendMode::ColorBurn,
        MixBlendMode::HardLight => PaintBlendMode::HardLight,
        MixBlendMode::SoftLight => PaintBlendMode::SoftLight,
        MixBlendMode::Difference => PaintBlendMode::Difference,
        MixBlendMode::Exclusion => PaintBlendMode::Exclusion,
        MixBlendMode::Hue => PaintBlendMode::Hue,
        MixBlendMode::Saturation => PaintBlendMode::Saturation,
        MixBlendMode::Color => PaintBlendMode::Color,
        MixBlendMode::Luminosity => PaintBlendMode::Luminosity,
    }
}

fn positioned_applicable_overflow_clips(
    clips: &[OverflowClip],
    containing_block: ContainingBlock,
) -> Vec<OverflowClip> {
    let containing_block_rect = PageTopRect::new(
        containing_block.x(),
        containing_block.top_y(),
        containing_block.width(),
        containing_block.height(),
    )
    .paint_rect();
    clips
        .iter()
        .copied()
        .filter(|clip| paint_rect_contains(clip.paint_rect(), containing_block_rect))
        .collect()
}

fn paint_rect_contains(outer: PaintRect, inner: PaintRect) -> bool {
    const EPSILON: f32 = 0.01;
    let outer_left = outer.origin.x;
    let outer_right = outer.origin.x + outer.size.width;
    let outer_bottom = outer.origin.y;
    let outer_top = outer.origin.y + outer.size.height;
    let inner_left = inner.origin.x;
    let inner_right = inner.origin.x + inner.size.width;
    let inner_bottom = inner.origin.y;
    let inner_top = inner.origin.y + inner.size.height;
    outer_left <= inner_left + EPSILON
        && outer_right + EPSILON >= inner_right
        && outer_bottom <= inner_bottom + EPSILON
        && outer_top + EPSILON >= inner_top
}

fn paint_transform_for_box(style: &ComputedStyle, border_box: PaintClip) -> Option<PaintTransform> {
    if style.transform.is_empty() {
        return None;
    }
    let origin_x =
        border_box.x() + used_length_percentage(style.transform_origin.x, border_box.width());
    let origin_y =
        border_box.y() + used_length_percentage(style.transform_origin.y, border_box.height());
    let mut transform = PaintTransform::translate(PaintVector::new(origin_x, origin_y));
    for function in &style.transform {
        transform = transform.multiply(transform_function_matrix(
            *function,
            border_box.width(),
            border_box.height(),
        ));
    }
    transform = transform.multiply(PaintTransform::translate(PaintVector::new(
        -origin_x, -origin_y,
    )));
    Some(transform)
}

fn transform_function_matrix(
    function: css::TransformFunction,
    border_box_width: f32,
    border_box_height: f32,
) -> PaintTransform {
    match function {
        css::TransformFunction::Matrix(a, b, c, d, e, f) => PaintTransform { a, b, c, d, e, f },
        css::TransformFunction::Translate(x, y) => PaintTransform::translate(PaintVector::new(
            used_length_percentage(x, border_box_width),
            used_length_percentage(y, border_box_height),
        )),
        css::TransformFunction::Scale(x, y) => PaintTransform {
            a: x,
            b: 0.0,
            c: 0.0,
            d: y,
            e: 0.0,
            f: 0.0,
        },
        css::TransformFunction::Rotate(angle) => {
            let sin = angle.sin();
            let cos = angle.cos();
            PaintTransform {
                a: cos,
                b: sin,
                c: -sin,
                d: cos,
                e: 0.0,
                f: 0.0,
            }
        }
        css::TransformFunction::Skew(x, y) => PaintTransform {
            a: 1.0,
            b: y.tan(),
            c: x.tan(),
            d: 1.0,
            e: 0.0,
            f: 0.0,
        },
    }
}
