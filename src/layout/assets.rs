use super::*;

impl<'a> LayoutBuilder<'a> {
    pub(super) fn layout_canvas(&mut self, element: &Element, style: &ComputedStyle) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let (content_width, content_height) =
            used_canvas_size(element, &used_style, available_width);
        let used_edges = used_box_edges(&used_style, available_width);
        used_style.margin = used_edges.margin.to_css_edges();
        used_style.padding = used_edges.padding.to_css_edges();
        let style = &used_style;
        let border_widths = used_border_widths(style);
        let border_box_width = content_width
            + style.padding.left
            + style.padding.right
            + border_widths.left
            + border_widths.right;
        let border_box_height = content_height
            + style.padding.top
            + style.padding.bottom
            + border_widths.top
            + border_widths.bottom;

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
            PaintClip {
                x: border_x,
                y: border_y,
                width: border_box_width,
                height: border_box_height,
            },
            style,
            Vec::new(),
        );
        self.cursor_y -= border_box_height + style.margin.bottom;
    }

    pub(super) fn layout_image(&mut self, element: &Element, style: &ComputedStyle) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let used_edges = used_box_edges(&used_style, available_width);
        used_style.margin = used_edges.margin.to_css_edges();
        used_style.padding = used_edges.padding.to_css_edges();
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
        let border_widths = used_border_widths(style);

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
                self.push_rect(RenderedRect {
                    x: image_x,
                    y: image_y,
                    width: image.content_width,
                    height: image.content_height,
                    fill: Some(fill),
                    stroke: None,
                    stroke_width: 0.0,
                });
            } else {
                self.push_image(RenderedImage {
                    background: false,
                    x: image_x,
                    y: image_y,
                    width: image.content_width,
                    height: image.content_height,
                    pixel_width: image.decoded.pixel_width,
                    pixel_height: image.decoded.pixel_height,
                    source_rect: None,
                    interpolate: false,
                    rgb: image.decoded.rgb,
                    alpha: image.decoded.alpha,
                    alt_text: element.attrs.get("alt").cloned(),
                });
            }
        }
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip {
                x: border_x,
                y: border_y,
                width: image.border_box_width,
                height: image.border_box_height,
            },
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
        let used_edges = used_box_edges(&used_style, available_width);
        used_style.margin = used_edges.margin.to_css_edges();
        used_style.padding = used_edges.padding.to_css_edges();
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
        let border_widths = used_border_widths(style);
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
                self.push_image(RenderedImage {
                    background: false,
                    x: image_x,
                    y: image_y,
                    width: image.content_width,
                    height: image.content_height,
                    pixel_width: image.decoded.pixel_width,
                    pixel_height: image.decoded.pixel_height,
                    source_rect: None,
                    interpolate: false,
                    rgb: image.decoded.rgb,
                    alpha: image.decoded.alpha,
                    alt_text,
                });
            }
        }
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip {
                x: border_x,
                y: border_y,
                width: image.border_box_width,
                height: image.border_box_height,
            },
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
        let used_edges = used_box_edges(&used_style, available_width);
        used_style.margin = used_edges.margin.to_css_edges();
        used_style.padding = used_edges.padding.to_css_edges();
        let style = &used_style;
        let border_widths = used_border_widths(style);
        let border_box_width = content_width
            + style.padding.left
            + style.padding.right
            + border_widths.left
            + border_widths.right;
        let border_box_height = content_height
            + style.padding.top
            + style.padding.bottom
            + border_widths.top
            + border_widths.bottom;
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
            self.push_rect(RenderedRect {
                x: border_x + border_widths.left + style.padding.left,
                y: border_y + border_widths.bottom + style.padding.bottom,
                width: content_width,
                height: content_height,
                fill: Some(fill),
                stroke: None,
                stroke_width: 0.0,
            });
        }
        self.scope_current_page_atomic_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip {
                x: border_x,
                y: border_y,
                width: border_box_width,
                height: border_box_height,
            },
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
                    RenderedImageTileRect {
                        x: dest_x[col],
                        y: dest_y[row],
                        width: dest_width[col],
                        height: dest_height[row],
                    },
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
        let used_edges = used_box_edges(&used_style, containing_block.width);
        used_style.margin = used_edges.margin.to_css_edges();
        used_style.padding = used_edges.padding.to_css_edges();
        if used_style.display.is_inline_level() {
            // CSS Display blockifies the outer display type of absolutely
            // positioned boxes for layout while preserving the static-position
            // source separately:
            // https://www.w3.org/TR/css-display-3/#transformations
            used_style.display = used_style.display.blockified();
        }
        let style = &used_style;
        let positioned_available_width = (containing_block.width
            - style.margin.left
            - style.margin.right
            - style.padding.left
            - style.padding.right
            - horizontal_border_width(style))
        .max(style.font_size);
        let static_horizontal_start = (previous_left - containing_block.x).max(0.0);
        let inline_auto_static_y = style.abspos_static_source_was_inline_level
            && used_inset_top(style, containing_block).is_none()
            && used_inset_bottom(style, containing_block).is_none();
        let inline_static_baseline_y = inline_auto_static_y
            .then_some(self.inline_static_baseline_y)
            .flatten();
        let static_vertical_start = (containing_block.top_y - previous_cursor_y).max(0.0);
        let shrink_to_fit_width = self.estimate_shrink_to_fit_width(
            element,
            style,
            stylesheets,
            positioned_available_width,
            child_boxes,
            table_fragment,
        );
        let positioned_x = resolve_absolute_horizontal(
            style,
            containing_block,
            shrink_to_fit_width,
            static_horizontal_start,
        );
        let positioned_content_width = positioned_x.size;

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
        let vertical_border_width_for_positioning = if is_html_table_element(element) {
            self.collapsed_table_outer_vertical_insets(style, stylesheets, table_fragment)
                .unwrap_or_else(|| vertical_border_width(style))
        } else {
            vertical_border_width(style)
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
        self.content_left = containing_block.x + positioned_x.start + style.margin.left;
        self.content_right = self.content_left + positioned_border_box_width;
        // CSS Positioned Layout defines the auto inset static-position
        // rectangle from the box's hypothetical normal-flow position. Inline
        // abspos boxes are blockified for layout, but their static position is
        // still the inline margin box; avoid subtracting the start margin a
        // second time until box-tree placeholders carry explicit rectangles.
        let positioned_margin_top = if inline_auto_static_y {
            ((style.line_height - style.font_size) / 2.0).max(0.0)
        } else {
            style.margin.top
        };
        self.cursor_y = containing_block.top_y - positioned_y.start - positioned_margin_top;
        let positioned_border_box_height = positioned_content_height
            + style.padding.top
            + style.padding.bottom
            + vertical_border_width(style);
        let positioned_border_box = PaintClip {
            x: self.content_left,
            y: self.cursor_y - positioned_border_box_height,
            width: positioned_border_box_width,
            height: positioned_border_box_height,
        };

        let mut flow_style = style.clone();
        flow_style.position = Position::Static;
        flow_style.margin = css::Edges::ZERO;
        set_style_used_width(&mut flow_style, positioned_content_width);
        set_style_used_height(&mut flow_style, positioned_content_height);
        clear_position_insets(&mut flow_style);
        let border_widths = used_border_widths(&flow_style);
        self.containing_blocks.push(ContainingBlock {
            x: self.content_left + border_widths.left,
            top_y: self.cursor_y - border_widths.top,
            width: positioned_content_width + flow_style.padding.left + flow_style.padding.right,
            height: positioned_content_height + flow_style.padding.top + flow_style.padding.bottom,
        });
        self.push_page_name_scope_suppression();
        self.layout_element_inner(
            element,
            &flow_style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
        );
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
            if let (Some(static_baseline_y), Some(fragment_baseline_y)) =
                (inline_static_baseline_y, positioned_fragment.first_line_y())
            {
                *positioned_fragment = positioned_fragment
                    .clone()
                    .translated(0.0, static_baseline_y - fragment_baseline_y);
            }
        }
        if positioned_fragments
            .iter()
            .all(|(_, fragment)| fragment.is_empty())
            && child_positioned_layers.is_empty()
        {
            return;
        }
        let z_index = style.z_index.unwrap_or(0);
        let child_layers_by_page = child_positioned_layers;
        for (page_index, positioned_fragment) in positioned_fragments {
            let child_layers = child_layers_by_page
                .iter()
                .filter(|layer| layer.page_index == page_index)
                .cloned()
                .collect::<Vec<_>>();
            let mut links = positioned_fragment.links.clone();
            for layer in &child_layers {
                links.extend(layer.links.clone());
            }
            let child_contexts = child_layers
                .into_iter()
                .map(|layer| layer.context)
                .collect();
            let bounds = positioned_border_box;
            let effects = paint_effects_for_box(style, bounds);
            let context = PaintStackingContext::new(z_index, positioned_fragment, child_contexts)
                .with_source_order(self.next_paint_source_order())
                .with_effects(effects)
                .with_bounds(bounds);
            if style.position == Position::Fixed {
                self.fixed_layers.push(FixedPaintLayer {
                    z_index,
                    context,
                    links,
                });
                continue;
            }
            self.positioned_layers.push(PositionedPaintLayer {
                page_index,
                z_index,
                context,
                links,
            });
        }
    }

    pub(super) fn layout_positioned_block_with_inline_static_baseline(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
        static_baseline_y: f32,
    ) {
        let previous = self.inline_static_baseline_y;
        self.inline_static_baseline_y = Some(static_baseline_y);
        self.layout_positioned_block(element, style, stylesheets, child_boxes, table_fragment);
        self.inline_static_baseline_y = previous;
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
        let margin_box_top = containing_block.top_y - positioned_y.start;
        let margin_box_height = style.margin.top
            + positioned_y.size
            + style.padding.top
            + style.padding.bottom
            + vertical_border_width
            + style.margin.bottom;
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
        if style.display.is_flex() {
            return self
                .estimate_flex_shrink_to_fit_width(
                    element,
                    style,
                    stylesheets,
                    available_width,
                    child_boxes,
                )
                .max(0.0);
        }
        if style.display.is_table()
            && let Some(fragment) = table_fragment
        {
            return self.table_shrink_to_fit_width_from_fragment(
                element,
                style,
                stylesheets,
                fragment,
                available_width,
            );
        }

        let contribution = self.intrinsic_inline_contribution_for_element(
            element,
            style,
            stylesheets,
            child_boxes,
        );
        let mut preferred = intrinsic::guarded_max_content_width(contribution.max_content, style);
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
                let child_extras = child_style.margin.left
                    + child_style.margin.right
                    + child_style.padding.left
                    + child_style.padding.right
                    + horizontal_border_width(child_style);
                let (intrinsic_preferred_min, intrinsic_preferred) =
                    if let box_tree::FormattingBox::Table(table_box) = child_box {
                        self.table_intrinsic_widths_from_fragment(
                            table_box.element,
                            child_style,
                            stylesheets,
                            &table_box.fragment,
                            available_width,
                        )
                    } else if child_style.display.is_flex() {
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
                        (
                            contribution.min_content,
                            intrinsic::guarded_max_content_width(
                                contribution.max_content,
                                child_style,
                            ),
                        )
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
                let child_style = style_for_layout_element(
                    child_element,
                    signature,
                    stylesheets,
                    Some(style),
                    &self.ancestors,
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
                        (
                            contribution.min_content,
                            intrinsic::guarded_max_content_width(
                                contribution.max_content,
                                &child_style,
                            ),
                        )
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

        intrinsic::shrink_to_fit_width(preferred_min, preferred, available_width)
            .max(style.font_size)
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
        let snapshot = self.snapshot();
        self.content_left = 0.0;
        self.content_right = width.max(style.font_size);
        self.cursor_y = self.page_bottom() + 10_000.0;
        let start_y = self.cursor_y;
        self.containing_blocks.push(ContainingBlock {
            x: self.content_left,
            top_y: self.cursor_y,
            width: self.content_right - self.content_left,
            height: 10_000.0,
        });
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
        // `height` term and add padding/borders separately.
        (consumed - style.padding.top - style.padding.bottom - vertical_border_width(style))
            .max(0.0)
    }

    pub(super) fn page_containing_block(&self) -> ContainingBlock {
        ContainingBlock {
            x: self.page_left(),
            top_y: self.page_top(),
            width: self.page_area_width(),
            height: self.page_area_height(),
        }
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
    let Some(BackgroundImage::Url {
        src,
        base_url,
        root_url,
    }) = style.background_image.as_ref()
    else {
        return Vec::new();
    };
    let Some(decoded) = load_image_source(
        src,
        base_url.as_deref().or(fallback_base_url),
        root_url.as_deref().or(fallback_root_url),
        resource_cache,
    ) else {
        return Vec::new();
    };
    let (image_width, image_height) =
        used_background_size(&decoded, area.width, area.height, style.background_size);
    if image_width <= 0.0 || image_height <= 0.0 {
        return Vec::new();
    }
    let (offset_x, offset_y) = background_position(
        style.background_position,
        area.width,
        area.height,
        image_width,
        image_height,
    );
    let tile_xs = background_tile_positions(
        area.x + offset_x,
        area.x,
        area.width,
        image_width,
        style.background_repeat.repeats_x(),
    );
    let tile_ys = background_tile_positions(
        area.y + offset_y,
        area.y,
        area.height,
        image_height,
        style.background_repeat.repeats_y(),
    );
    let mut images = Vec::new();
    for tile_y in tile_ys {
        for tile_x in &tile_xs {
            images.push(RenderedImage {
                background: true,
                x: *tile_x,
                y: tile_y,
                width: image_width,
                height: image_height,
                pixel_width: decoded.pixel_width,
                pixel_height: decoded.pixel_height,
                source_rect: None,
                interpolate: true,
                rgb: decoded.rgb.clone(),
                alpha: decoded.alpha.clone(),
                alt_text: None,
            });
        }
    }
    images
}

fn clear_position_insets(style: &mut ComputedStyle) {
    clear_style_insets(style);
}

#[derive(Debug, Clone, Copy)]
struct RenderedImageTileRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
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
        border_image_tile_segments(repeat_x, destination.width, tile_width, source.width);
    let y_segments =
        border_image_tile_segments(repeat_y, destination.height, tile_height, source.height);
    for y_segment in &y_segments {
        for x_segment in &x_segments {
            if x_segment.destination_size <= 0.0
                || y_segment.destination_size <= 0.0
                || x_segment.source_size == 0
                || y_segment.source_size == 0
            {
                continue;
            }
            images.push(RenderedImage {
                background: true,
                x: destination.x + x_segment.destination_offset,
                y: destination.y + y_segment.destination_offset,
                width: x_segment.destination_size,
                height: y_segment.destination_size,
                pixel_width: decoded.pixel_width,
                pixel_height: decoded.pixel_height,
                source_rect: Some(RenderedImageSourceRect {
                    x: source.x + x_segment.source_offset,
                    y: source.y + y_segment.source_offset,
                    width: x_segment.source_size,
                    height: y_segment.source_size,
                }),
                interpolate: true,
                rgb: decoded.rgb.clone(),
                alpha: decoded.alpha.clone(),
                alt_text: None,
            });
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
        let scale = destination.height / source.height as f32;
        tile_width *= scale;
    }
    if repeat_y != css::BorderImageRepeatKeyword::Stretch
        && repeat_x == css::BorderImageRepeatKeyword::Stretch
        && source.width > 0
    {
        let scale = destination.width / source.width as f32;
        tile_height *= scale;
    }
    if repeat_x == css::BorderImageRepeatKeyword::Stretch {
        tile_width = destination.width;
    }
    if repeat_y == css::BorderImageRepeatKeyword::Stretch {
        tile_height = destination.height;
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
    shrink_to_fit_width: f32,
    static_start: f32,
) -> PositionedAxis {
    // CSS 2.1 10.3.7, non-replaced absolutely positioned elements. Static
    // position is approximated from the layout cursor/content edge at the
    // element's source position until layout carries explicit placeholders.
    let left = used_inset_left(style, containing_block);
    let right = used_inset_right(style, containing_block);
    let width = used_content_width_or_auto(
        style,
        containing_block.width,
        style.padding.left + style.padding.right + horizontal_border_width(style),
    );
    let static_start = static_start.clamp(0.0, containing_block.width);
    let margin_start = style.margin.left;
    let margin_end = style.margin.right;
    let non_content = style.padding.left + style.padding.right + horizontal_border_width(style);
    let fill_between = |start: f32, end: f32| {
        (containing_block.width - start - margin_start - non_content - margin_end - end)
            .max(style.font_size)
    };
    let border_box_size = |content_size: f32| content_size + non_content;
    let start_for_end = |content_size: f32, end: f32| {
        containing_block.width - end - margin_start - margin_end - border_box_size(content_size)
    };

    match (left, width, right) {
        (Some(start), Some(size), _) => PositionedAxis { start, size },
        (Some(start), None, Some(end)) => PositionedAxis {
            start,
            size: fill_between(start, end),
        },
        (Some(start), None, None) => PositionedAxis {
            start,
            size: shrink_to_fit_width,
        },
        (None, Some(size), Some(end)) => PositionedAxis {
            start: start_for_end(size, end),
            size,
        },
        (None, Some(size), None) => PositionedAxis {
            start: static_start,
            size,
        },
        (None, None, Some(end)) => PositionedAxis {
            start: start_for_end(shrink_to_fit_width, end),
            size: shrink_to_fit_width,
        },
        (None, None, None) => PositionedAxis {
            start: static_start,
            size: shrink_to_fit_width,
        },
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
        containing_block.height,
        style.padding.top + style.padding.bottom + vertical_border_width,
    );
    let static_start = static_start.clamp(0.0, containing_block.height);
    let margin_start = style.margin.top;
    let margin_end = style.margin.bottom;
    let non_content = style.padding.top + style.padding.bottom + vertical_border_width;
    let fill_between = |start: f32, end: f32| {
        (containing_block.height - start - margin_start - non_content - margin_end - end).max(0.0)
    };
    let border_box_size = |content_size: f32| content_size + non_content;
    let start_for_end = |content_size: f32, end: f32| {
        containing_block.height - end - margin_start - margin_end - border_box_size(content_size)
    };

    match (top, height, bottom) {
        (Some(start), Some(size), _) => PositionedAxis { start, size },
        (Some(start), None, Some(end)) => PositionedAxis {
            start,
            size: fill_between(start, end),
        },
        (Some(start), None, None) => PositionedAxis {
            start,
            size: auto_height,
        },
        (None, Some(size), Some(end)) => PositionedAxis {
            start: start_for_end(size, end),
            size,
        },
        (None, Some(size), None) => PositionedAxis {
            start: static_start,
            size,
        },
        (None, None, Some(end)) => PositionedAxis {
            start: start_for_end(auto_height, end),
            size: auto_height,
        },
        (None, None, None) => PositionedAxis {
            start: static_start,
            size: auto_height,
        },
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
        overflow_clip: style.overflow.clips_overflow().then_some(PaintClip {
            x: border_box.x + borders.left,
            y: border_box.y + borders.bottom,
            width: (border_box.width - borders.left - borders.right).max(0.0),
            height: (border_box.height - borders.top - borders.bottom).max(0.0),
        }),
        absolute_clip: None,
    }
}

fn paint_transform_for_box(style: &ComputedStyle, border_box: PaintClip) -> Option<PaintTransform> {
    if style.transform.is_empty() {
        return None;
    }
    let origin_x =
        border_box.x + used_length_percentage(style.transform_origin.x, border_box.width);
    let origin_y =
        border_box.y + used_length_percentage(style.transform_origin.y, border_box.height);
    let mut transform = PaintTransform::translate(origin_x, origin_y);
    for function in &style.transform {
        transform = transform.multiply(transform_function_matrix(
            *function,
            border_box.width,
            border_box.height,
        ));
    }
    transform = transform.multiply(PaintTransform::translate(-origin_x, -origin_y));
    Some(transform)
}

fn transform_function_matrix(
    function: css::TransformFunction,
    border_box_width: f32,
    border_box_height: f32,
) -> PaintTransform {
    match function {
        css::TransformFunction::Matrix(a, b, c, d, e, f) => PaintTransform { a, b, c, d, e, f },
        css::TransformFunction::Translate(x, y) => PaintTransform::translate(
            used_length_percentage(x, border_box_width),
            used_length_percentage(y, border_box_height),
        ),
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
