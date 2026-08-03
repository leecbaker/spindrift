use super::*;
use std::rc::Rc;

/// The descendant-content clip carried by an atomic replaced element.
///
/// An empty contour is distinct from the absence of `border-shape`: it
/// suppresses content entirely, rather than falling back to the rectangular
/// content box.
#[derive(Debug, Clone)]
pub(in crate::layout) enum ReplacedBorderShapeContentClip {
    Path(RenderedPathClip),
    Empty,
}
/// A positioned box's source style and its normalized layout style.
///
/// Positioned layout must retain the source style for cascade-owned decisions
/// such as authored `auto` sizing, while all geometry consumes the used style
/// after viewport resolution and CSS `zoom`. Keeping both views together
/// prevents an out-of-flow entry point from applying the same effective zoom
/// twice while reconstructing a positioned subtree.
/// <https://drafts.csswg.org/css-position-3/#abspos-layout>
/// <https://drafts.csswg.org/css-viewport/#zoom-property>
#[derive(Debug, Clone)]
pub(in crate::layout) struct PositionedUsedStyle {
    source: ComputedStyle,
    used: css::ZoomedLayoutStyle,
}

impl PositionedUsedStyle {
    fn from_source_and_used(source: ComputedStyle, used: css::ZoomedLayoutStyle) -> Self {
        Self { source, used }
    }

    pub(in crate::layout) fn source(&self) -> &ComputedStyle {
        &self.source
    }

    pub(in crate::layout) fn used(&self) -> &ComputedStyle {
        &self.used
    }
}
impl<'a> LayoutBuilder<'a> {
    /// Prepare an out-of-flow box at the common positioned used-value boundary.
    ///
    /// Every absolute/fixed entry ultimately reaches `layout_positioned_block`.
    /// This makes its viewport and zoom normalization authoritative while the
    /// frozen box tree continues to own descendant cascade state.
    pub(in crate::layout) fn positioned_used_style(
        &mut self,
        source: &ComputedStyle,
    ) -> PositionedUsedStyle {
        // Viewport-length resolution and effective zoom are separate used
        // value steps. A frozen style may already carry its zoom transform
        // while retaining `vh`/`vw` descriptors for the positioned-layout
        // entry point; skipping normalization in that state turns a definite
        // `height: 300vh` into an auto-sized one-line absolute box.
        // <https://www.w3.org/TR/css-values-4/#viewport-relative-lengths>
        // <https://www.w3.org/TR/css-position-3/#abspos-layout>
        let mut used = css::LayoutStyle::from_computed(source);
        self.resolve_style_current_viewport_lengths(&mut used);
        // Positioned layout is an isolated used-value boundary just like
        // normal block layout.  Retaining a font-relative expression here
        // makes a definite `6em` width or height look like `auto` to the
        // absolute-position equations, incorrectly falling back to
        // shrink-to-fit content sizing.
        // <https://www.w3.org/TR/css-values-4/#font-relative-lengths>
        // <https://www.w3.org/TR/css-position-3/#abspos-layout>
        used.finalize_computed_font_relative_lengths();
        self.resolve_style_font_metric_lengths(&mut used);
        let used = used.into_zoomed();
        PositionedUsedStyle::from_source_and_used(source.clone(), used)
    }

    pub(in crate::layout) fn layout_canvas(&mut self, element: &Element, style: &ComputedStyle) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let containing_block_height = self
            .definite_block_size_stack
            .last()
            .cloned()
            .unwrap_or_else(PercentageBasis::indefinite);
        apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            self.current_content_logical_inline_percentage_basis(),
        );
        let canvas = used_canvas(
            element,
            &used_style,
            available_width,
            containing_block_height,
        );
        let style = &used_style;
        let border_box_width = canvas.border_box_size.width;
        let border_box_height = canvas.border_box_size.height;
        if element.tag.eq_ignore_ascii_case("iframe") {
            self.resource_cache.record_iframe_viewport(
                element.id,
                canvas.content_size.width,
                canvas.content_size.height,
            );
        }

        let border_origin =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let border_x = border_origin.x;
        let border_y = border_origin.y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible
            && (style.background_color.is_potentially_visible()
                || used_border_width(style) > layout_pt(0.0))
        {
            let (rects, rounded_rects, paths, strokes) = block_paint_ops(
                paint_space_rect(border_x, border_y, border_box_width, border_box_height),
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
        if element.tag.eq_ignore_ascii_case("iframe")
            && let Some(document) = self.iframe_documents.get(&element.id)
            && let Some(page) = document.pages.first()
        {
            let borders = used_border_widths(style);
            let content_x = border_x + borders.left + style.padding.left;
            let content_y = border_y + borders.bottom + style.padding.bottom;
            let content_rect = paint_space_rect(
                content_x,
                content_y,
                canvas.content_size.width,
                canvas.content_size.height,
            );
            // The child page is produced with the iframe content-box as its
            // viewport. Re-home its primitive copy into the parent paint tree
            // and clip the entire isolated browsing context to that viewport.
            // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
            let mut fragment = page.paint_fragment().translated(PaintTranslation::new(
                content_x,
                content_y + canvas.content_size.height - page.height(),
            ));
            fragment.promote_page_background_to_in_flow_block();
            fragment.promote_background_border_to_in_flow_block();
            fragment = fragment.with_contents_effect_scoped_to_rect_if_needed(
                page,
                PaintClip::from_paint_rect(content_rect),
            );
            self.current_page
                .append_paint_fragment_owned(fragment, PaintTranslation::identity());
        }
        self.scope_current_page_replaced_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip::from_paint_rect(paint_space_rect(
                border_x,
                border_y,
                border_box_width,
                border_box_height,
            )),
            style,
        );
        self.cursor_y -= border_box_height + style.margin.bottom;
    }

    pub(in crate::layout) fn layout_image(&mut self, element: &Element, style: &ComputedStyle) {
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            self.current_content_logical_inline_percentage_basis(),
        );
        let style = &used_style;
        let Some(image) = used_image(
            element,
            style,
            available_width,
            self.definite_block_size_stack
                .last()
                .cloned()
                .unwrap_or_else(PercentageBasis::indefinite),
            self.base_url,
            self.root_url,
            self.resource_cache,
        ) else {
            return;
        };
        let border_widths = box_metrics.border.to_css_edges();
        let content_width = image.content_size.width;
        let content_height = image.content_size.height;
        let border_box_width = image.border_box_size.width;
        let border_box_height = image.border_box_size.height;

        let border_origin =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let border_x = border_origin.x;
        let border_y = border_origin.y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let decoration_rect =
            paint_space_rect(border_x, border_y, border_box_width, border_box_height);
        let border_shape_content_clip =
            single_border_shape_content_clip(decoration_rect, style, border_widths);
        if style.visibility == Visibility::Visible {
            if style.background_color.is_potentially_visible()
                || used_border_width(style) > layout_pt(0.0)
            {
                // Replaced content occupies the content layer: its box
                // background belongs below it, while inset shadows and the
                // border must paint above it. A monolithic decoration pass
                // reverses that order and lets the image erase its border.
                let (rects, rounded_rects, paths, strokes) = block_paint_ops_with_phases(
                    decoration_rect,
                    style,
                    border_widths,
                    true,
                    true,
                    false,
                    false,
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
            // A size-contained replaced box may retain its padding and
            // background while its content box is empty. SVG's viewport
            // painter treats a zero destination as an omitted viewport, so
            // do not let it fall back to its natural dimensions here.
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            if content_width > 0.0
                && content_height > 0.0
                && !matches!(
                    border_shape_content_clip,
                    Some(ReplacedBorderShapeContentClip::Empty)
                )
            {
                if let Some(asset) = image.svg {
                    let mut group = svg_replaced_group(
                        &asset,
                        paint_space_rect(image_x, image_y, content_width, content_height),
                        style.object_fit,
                        style.object_position.clone(),
                        style.object_view_box.clone(),
                    );
                    if let Some(ReplacedBorderShapeContentClip::Path(clip)) =
                        border_shape_content_clip
                    {
                        group = group.with_clip(clip);
                    }
                    self.push_svg_group_in_band(PaintBand::InFlowBlock, group);
                } else {
                    let mut rendered = RenderedImage::from_paint_rect(
                        paint_space_rect(image_x, image_y, content_width, content_height),
                        false,
                        image.decoded.pixel_width,
                        image.decoded.pixel_height,
                        None,
                        raster_image_interpolation(style),
                        image.decoded.rgb.shared(),
                        image.decoded.alpha,
                        element.attrs.get("alt").cloned().map(Rc::from),
                    )
                    .with_raster_color_space(image.decoded.color_space.clone())
                    .with_image_id(image.decoded.image_id);
                    if let Some(ReplacedBorderShapeContentClip::Path(clip)) =
                        border_shape_content_clip
                    {
                        rendered = rendered.with_clip(clip);
                    }
                    if apply_object_fit(
                        &mut rendered,
                        style.object_fit,
                        style.object_position.clone(),
                        style.object_view_box.clone(),
                    ) {
                        self.push_image(rendered);
                    }
                }
            }
            if style.background_color.is_potentially_visible()
                || used_border_width(style) > layout_pt(0.0)
            {
                let (rects, rounded_rects, paths, strokes) = block_paint_ops_with_phases(
                    decoration_rect,
                    style,
                    border_widths,
                    false,
                    false,
                    true,
                    true,
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
        }
        self.scope_current_page_replaced_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip::from_paint_rect(paint_space_rect(
                border_x,
                border_y,
                border_box_width,
                border_box_height,
            )),
            style,
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
        let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            self.current_content_logical_inline_percentage_basis(),
        );
        let style = &used_style;
        let image = image
            .as_image()
            .and_then(|image| {
                used_generated_image_value(
                    image,
                    style,
                    available_width,
                    self.base_url,
                    self.root_url,
                    self.resource_cache,
                )
            })
            .unwrap_or_else(|| used_invalid_replacement_image(style, available_width));
        let alt_text = self.generated_alt_text(element, style);
        let border_widths = box_metrics.border.to_css_edges();
        let content_width = image.content_size.width;
        let content_height = image.content_size.height;
        let border_box_width = image.border_box_size.width;
        let border_box_height = image.border_box_size.height;
        let border_origin =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let border_x = border_origin.x;
        let border_y = border_origin.y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible {
            if style.background_color.is_potentially_visible()
                || used_border_width(style) > layout_pt(0.0)
            {
                let (rects, rounded_rects, paths, strokes) = block_paint_ops(
                    paint_space_rect(border_x, border_y, border_box_width, border_box_height),
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
                if let UsedReplacementImageSource::Gradient { image: gradient } = &image.source
                    && style.object_fit == css::ObjectFit::Fill
                    && matches!(style.object_view_box, css::ObjectViewBox::None)
                    && let Some(primitive) = native_generated_gradient_primitive(
                        gradient,
                        paint_space_rect(image_x, image_y, content_width, content_height),
                        style.color,
                        None,
                    )
                {
                    self.push_primitive_in_band(PaintBand::InFlowBlock, primitive);
                } else if let Some(asset) = image.svg {
                    self.push_svg_group_in_band(
                        PaintBand::InFlowBlock,
                        svg_replaced_group(
                            &asset,
                            paint_space_rect(image_x, image_y, content_width, content_height),
                            style.object_fit,
                            style.object_position.clone(),
                            style.object_view_box.clone(),
                        ),
                    );
                } else {
                    let mut rendered = RenderedImage::from_paint_rect(
                        paint_space_rect(image_x, image_y, content_width, content_height),
                        false,
                        image.decoded.pixel_width,
                        image.decoded.pixel_height,
                        None,
                        raster_image_interpolation(style),
                        image.decoded.rgb.shared(),
                        image.decoded.alpha,
                        alt_text.map(Rc::from),
                    )
                    .with_raster_color_space(image.decoded.color_space.clone())
                    .with_image_id(image.decoded.image_id);
                    if apply_object_fit(
                        &mut rendered,
                        style.object_fit,
                        style.object_position.clone(),
                        style.object_view_box.clone(),
                    ) {
                        self.push_image(rendered);
                    }
                }
            }
        }
        self.scope_current_page_replaced_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip::from_paint_rect(paint_space_rect(
                border_x,
                border_y,
                border_box_width,
                border_box_height,
            )),
            style,
        );
        self.cursor_y -= border_box_height + style.margin.bottom;
    }

    pub(in crate::layout) fn layout_svg(&mut self, element: &Element, style: &ComputedStyle) {
        let Some(asset) = self.resource_cache.inline_svg_asset(element) else {
            return;
        };
        let available_width = (self.content_right - self.content_left).max(1.0);
        let mut used_style = self.style_with_current_viewport_lengths(style);
        let box_metrics = apply_used_box_metrics_for_logical_inline_basis(
            &mut used_style,
            self.current_content_logical_inline_percentage_basis(),
        );
        let style = &used_style;
        let Some(geometry) = used_svg(
            element,
            style,
            available_width,
            self.definite_block_size_stack
                .last()
                .cloned()
                .unwrap_or_else(PercentageBasis::indefinite),
        ) else {
            return;
        };
        let border_widths = box_metrics.border.to_css_edges();
        let content_width = geometry.content_size.width;
        let content_height = geometry.content_size.height;
        let border_box_width = geometry.border_box_size.width;
        let border_box_height = geometry.border_box_size.height;
        let border_origin =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let border_x = border_origin.x;
        let border_y = border_origin.y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let border_paint_rect =
            paint_space_rect(border_x, border_y, border_box_width, border_box_height);
        let border_shape_content_clip = style_clips_overflow(style)
            .then(|| single_border_shape_content_clip(border_paint_rect, style, border_widths))
            .flatten();
        if style.visibility == Visibility::Visible
            && (style.background_color.is_potentially_visible()
                || used_border_width(style) > layout_pt(0.0))
        {
            let (rects, rounded_rects, paths, strokes) = block_paint_ops(border_paint_rect, style);
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
        if style.visibility == Visibility::Visible
            && content_width > 0.0
            && content_height > 0.0
            && !matches!(
                border_shape_content_clip,
                Some(ReplacedBorderShapeContentClip::Empty)
            )
        {
            let content_x = border_x + border_widths.left + style.padding.left;
            let content_y = border_y + border_widths.bottom + style.padding.bottom;
            // An inline SVG with omitted root dimensions is parsed with a
            // provisional SVG viewport. Reparse it for this concrete CSS
            // replaced-object size before resolving percentage geometry such
            // as `<rect width="100%">`; otherwise a 1 by 1 viewBox leaks
            // into paint even when Flexbox has assigned a 100px item.
            // <https://www.w3.org/TR/SVG2/coords.html#ViewportSpace>
            let viewport_asset =
                asset.with_replaced_viewport(content_box_size_pt(content_width, content_height));
            let mut group = viewport_asset.paint_inline_group(
                paint_space_rect(content_x, content_y, content_width, content_height),
                style_clips_overflow(style),
            );
            if let Some(ReplacedBorderShapeContentClip::Path(clip)) = border_shape_content_clip {
                group = group.with_clip(clip);
            }
            self.push_svg_group_in_band(PaintBand::InFlowBlock, group);
        }
        self.scope_current_page_replaced_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip::from_paint_rect(border_paint_rect),
            style,
        );
        self.cursor_y -= border_box_height + style.margin.bottom;
    }

    pub(in crate::layout) fn place_block_replaced_box(
        &mut self,
        style: &ComputedStyle,
        border_box_width: f32,
        border_box_height: f32,
    ) -> PaintPoint {
        let margin_box_width = style.margin.left + border_box_width + style.margin.right;
        let margin_box_height = style.margin.top + border_box_height + style.margin.bottom;
        self.cursor_y -= style.margin.top;
        self.prebreak_bfc_margin_box_if_needed(margin_box_pt(margin_box_height), style.margin.top);
        let placement = self.place_float_avoiding_margin_box(
            PageTopBlockPosition::new(self.cursor_y),
            margin_box_size_pt(margin_box_width, margin_box_height),
            style.clear,
            style.writing_mode,
            style.direction,
            self.containing_block_direction,
        );
        self.cursor_y = placement.origin.top_y();
        // Replaced elements participate in normal flow just like ordinary
        // block boxes. Record a used positive block fragment before its
        // painting is emitted so named-page boundaries and fragmentation do
        // not treat a replaced box as transparent.
        // <https://www.w3.org/TR/CSS2/visuren.html#block-boxes>
        if border_box_height > 0.0 {
            self.mark_current_page_flow_content();
        }
        PaintPoint::new(
            placement.origin.x() + style.margin.left,
            self.cursor_y - border_box_height,
        )
    }

    pub(in crate::layout) fn background_image_primitives(
        &self,
        border_rect: PaintRect,
        style: &ComputedStyle,
    ) -> Vec<PaintPrimitive> {
        self.background_image_primitives_with_paint_areas(border_rect, border_rect, style)
    }

    /// Resolve a box background with independent positioning and clipping
    /// border areas.
    ///
    /// Fragmented boxes keep one background positioning area while each
    /// fragment contributes its own clip area.  This is the CSS Backgrounds
    /// `box-decoration-break: slice` model: a continuation must clip the
    /// source box's image rather than re-position a new layer in itself.
    /// Keeping this at the asset boundary also preserves `fixed` attachment's
    /// containing block while float paint capture re-parents the result.
    ///
    /// <https://www.w3.org/TR/css-backgrounds-3/#background-position>
    /// <https://www.w3.org/TR/css-backgrounds-3/#box-decoration-break>
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>
    pub(in crate::layout) fn background_image_primitives_with_paint_areas(
        &self,
        positioning_border_rect: PaintRect,
        clip_border_rect: PaintRect,
        style: &ComputedStyle,
    ) -> Vec<PaintPrimitive> {
        let fixed_positioning_area = self
            .fixed_containing_blocks
            .last()
            .cloned()
            .map(|containing_block| {
                PaintBackgroundArea::new(
                    PaintPoint::new(
                        containing_block.x(),
                        containing_block.top_y() - containing_block.height(),
                    ),
                    PaintSize::new(containing_block.width(), containing_block.height()),
                )
            })
            .unwrap_or_else(|| {
                PaintBackgroundArea::new(
                    PaintPoint::new(
                        self.current_page_context.left(),
                        self.current_page_context.bottom(),
                    ),
                    PaintSize::new(
                        self.current_page_context.area_width(),
                        self.current_page_context.area_height(),
                    ),
                )
            });
        background_image_primitives_for_style_with_paint_areas_and_fixed_positioning_area(
            PaintBackgroundArea::from_paint_rect(positioning_border_rect),
            PaintBackgroundArea::from_paint_rect(clip_border_rect),
            Some(fixed_positioning_area),
            // A fixed-position containing-block stack records ancestors that
            // localize viewport-fixed painting. A fixed background in that
            // subtree must likewise use element-local positioning; the box's
            // own transform is included before its containing-block scope is
            // necessarily pushed.
            style.has_transform() || !self.fixed_containing_blocks.is_empty(),
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
    pub(in crate::layout) fn border_image_paint(
        &self,
        border_rect: PaintRect,
        style: &ComputedStyle,
    ) -> BorderPaint {
        let Some(src) = style.border_image.source.as_image() else {
            return BorderPaint::UseNormalBorder;
        };
        let resolved = match src.selected_image() {
            BackgroundImage::Url {
                src,
                base_url,
                root_url,
                request_modifiers,
            } => load_resolved_image_source_with_request(
                src,
                base_url
                    .as_ref()
                    .or(style.border_image.source_base_url.as_ref())
                    .or(self.base_url),
                root_url
                    .as_ref()
                    .or(style.border_image.source_root_url.as_ref()),
                self.resource_cache,
                style.image_orientation == css::ImageOrientation::FromImage,
                request_modifiers,
            ),
            image => rasterize_generated_css_image(
                image,
                PaintSize::new(
                    border_rect.size.width / css::CSS_PX_TO_PT,
                    border_rect.size.height / css::CSS_PX_TO_PT,
                ),
                style.color,
                style
                    .border_image
                    .source_base_url
                    .as_ref()
                    .or(self.base_url),
                style.border_image.source_root_url.as_ref(),
                self.resource_cache,
            )
            .map(ResolvedImageAsset::Raster),
        };
        let asset = match resolved {
            Some(asset) => asset,
            None => return BorderPaint::UseNormalBorder,
        };
        let (source_width, source_height) = match &asset {
            ResolvedImageAsset::Raster(decoded) => (decoded.pixel_width, decoded.pixel_height),
            ResolvedImageAsset::Svg(asset) => {
                let size = asset.source_viewport_size();
                (
                    size.width.ceil().max(1.0) as u32,
                    size.height.ceil().max(1.0) as u32,
                )
            }
        };
        let slices = used_border_image_slices(
            style.border_image.slice.offsets,
            source_width,
            source_height,
        );
        let borders = used_border_widths(style);
        let outsets = used_border_image_outsets(style, borders);
        let area_width = border_rect.size.width + outsets.left + outsets.right;
        let area_height = border_rect.size.height + outsets.top + outsets.bottom;
        let image_widths = fit_border_image_widths_to_area(
            used_border_image_widths(
                style,
                borders,
                border_box_pt(border_rect.size.width),
                border_box_pt(border_rect.size.height),
                slices,
            ),
            area_width,
            area_height,
        );
        let area_x = border_rect.origin.x - outsets.left;
        let area_y = border_rect.origin.y - outsets.bottom;

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
        let source_x = [0, slices.left, source_width.saturating_sub(slices.right)];
        let source_width = [
            slices.left,
            source_width
                .saturating_sub(slices.left)
                .saturating_sub(slices.right),
            slices.right,
        ];
        let source_y = [source_height.saturating_sub(slices.bottom), slices.top, 0];
        let source_height = [
            slices.bottom,
            source_height
                .saturating_sub(slices.top)
                .saturating_sub(slices.bottom),
            slices.top,
        ];

        let mut primitives = Vec::new();
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
                let destination = RenderedImageTileRect::from_paint_rect(paint_space_rect(
                    dest_x[col],
                    dest_y[row],
                    dest_width[col],
                    dest_height[row],
                ));
                let source = RenderedImageSourceRect {
                    x: source_x[col],
                    y: source_y[row],
                    width: source_width[col],
                    height: source_height[row],
                };
                match &asset {
                    ResolvedImageAsset::Raster(decoded) => {
                        let mut images = Vec::new();
                        push_border_image_tiles(
                            &mut images,
                            decoded,
                            destination,
                            source,
                            repeat_x,
                            repeat_y,
                            raster_image_interpolation(style),
                        );
                        primitives.extend(images.into_iter().map(PaintPrimitive::Image));
                    }
                    ResolvedImageAsset::Svg(asset) => {
                        push_svg_border_image_tiles(
                            &mut primitives,
                            asset,
                            destination,
                            source,
                            repeat_x,
                            repeat_y,
                        );
                    }
                }
            }
        }
        BorderPaint::ReplaceNormalBorder { primitives }
    }
}
