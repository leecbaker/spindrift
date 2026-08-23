use std::rc::Rc;

use super::*;
use crate::layout::asset_helpers::CssImageNaturalDimensions;

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
                IframeEmbeddingContext {
                    viewport: PageSize::from_points(
                        canvas.content_size.width,
                        canvas.content_size.height,
                    ),
                    effective_zoom: style.effective_zoom,
                },
            );
        }

        let border_origin =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let border_x = border_origin.x;
        let border_y = border_origin.y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        self.paint_replaced_box_decoration(
            paint_space_rect(border_x, border_y, border_box_width, border_box_height),
            style,
            PaintBand::BackgroundBorder,
        );
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
            fragment.promote_outline_to_in_flow_outline();
            // The embedded page canvas is descendant paint of the iframe,
            // rather than the embedding element's own decoration.  Its
            // background must therefore stay inside the same viewport clip
            // as the child document's translated scroll contents.
            // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
            fragment = fragment
                .with_effect_scoped_to_rect_all_bands(PaintClip::from_paint_rect(content_rect));
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
        let overflow = ReplacedObjectOverflow::from_style(style);
        let content_contour = replaced_content_contour(decoration_rect, style, border_widths);
        if style.visibility == Visibility::Visible {
            // A principal box's complete decoration paints in its parent
            // block phase before its contents. Replaced content is an atomic
            // content layer, not a reason to split the element's background
            // and border around an image draw. Keeping this on the ordinary
            // box-decoration path also gives replaced elements the complete
            // `border-image` and background-image algorithms.
            // <https://www.w3.org/TR/CSS22/zindex.html>
            // <https://drafts.csswg.org/css-backgrounds-3/#border-images>
            self.paint_replaced_box_decoration(decoration_rect, style, PaintBand::BackgroundBorder);
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
                    content_contour.as_ref().map(|clip| &clip.contour),
                    Some(BoxContentContour::Empty)
                )
            {
                if let Some(asset) = image.svg {
                    let mut group = svg_replaced_group(
                        &asset,
                        paint_space_rect(image_x, image_y, content_width, content_height),
                        style.object_fit,
                        style.object_position.clone(),
                        style.object_view_box.clone(),
                        overflow,
                    );
                    if let Some(clip) = content_contour
                        .as_ref()
                        .and_then(ResolvedBoxContentClip::path_clip)
                    {
                        group = group.with_clip(clip);
                    }
                    self.push_svg_group_in_band(PaintBand::InFlowBlock, group);
                } else {
                    let natural_size = image.decoded.natural_layout_size();
                    let mut rendered = RenderedImage::from_paint_rect(
                        paint_space_rect(image_x, image_y, content_width, content_height),
                        false,
                        image.decoded.pixel_size.width,
                        image.decoded.pixel_size.height,
                        None,
                        raster_image_sampling(style),
                        image.decoded.rgb.shared(),
                        image.decoded.alpha,
                        element.attrs.get("alt").cloned().map(Rc::from),
                    )
                    .with_raster_color_space(image.decoded.color_space.clone())
                    .with_image_id(image.decoded.image_id);
                    if let Some(clip) = content_contour
                        .as_ref()
                        .and_then(ResolvedBoxContentClip::path_clip)
                    {
                        rendered = rendered.with_clip(clip);
                    }
                    if apply_object_fit(
                        &mut rendered,
                        natural_size,
                        style.object_fit,
                        style.object_position.clone(),
                        style.object_view_box.clone(),
                        overflow,
                        style.effective_zoom,
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
        let overflow = ReplacedObjectOverflow::from_style(style);
        if style.visibility == Visibility::Visible {
            self.paint_replaced_box_decoration(
                paint_space_rect(border_x, border_y, border_box_width, border_box_height),
                style,
                PaintBand::BackgroundBorder,
            );
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
                            overflow,
                        ),
                    );
                } else {
                    let natural_size = image.decoded.natural_layout_size();
                    let mut rendered = RenderedImage::from_paint_rect(
                        paint_space_rect(image_x, image_y, content_width, content_height),
                        false,
                        image.decoded.pixel_size.width,
                        image.decoded.pixel_size.height,
                        image.decoded.source_rect,
                        raster_image_sampling(style),
                        image.decoded.rgb.shared(),
                        image.decoded.alpha,
                        alt_text.map(Rc::from),
                    )
                    .with_raster_color_space(image.decoded.color_space.clone())
                    .with_image_id(image.decoded.image_id);
                    if apply_object_fit(
                        &mut rendered,
                        natural_size,
                        style.object_fit,
                        style.object_position.clone(),
                        style.object_view_box.clone(),
                        overflow,
                        style.effective_zoom,
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
        let content_contour = replaced_content_contour(border_paint_rect, style, border_widths);
        let overflow_edge = resolve_overflow_clip_edge(
            border_paint_rect,
            style,
            border_widths,
            UsedOverflowAxes::from_svg_viewport_style(style),
            style.contain.paint,
            None,
        );
        self.paint_replaced_box_decoration(border_paint_rect, style, PaintBand::BackgroundBorder);
        if style.visibility == Visibility::Visible
            && content_width > 0.0
            && content_height > 0.0
            && !matches!(
                content_contour.as_ref().map(|clip| &clip.contour),
                Some(BoxContentContour::Empty)
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
                false,
            );
            if let Some(clip) = overflow_edge
                .as_ref()
                .and_then(|edge| edge.clip.path_clip())
                .or_else(|| {
                    content_contour
                        .as_ref()
                        .and_then(ResolvedBoxContentClip::path_clip)
                })
            {
                group = group.with_clip(clip);
            }
            self.push_svg_group_in_band(PaintBand::InFlowBlock, group);
        }
        self.scope_current_page_inline_svg_root_paint_since(
            &paint_checkpoint,
            PaintBand::InFlowBlock,
            PaintClip::from_paint_rect(border_paint_rect),
            overflow_edge.as_ref().map_or_else(
                || PaintClip::from_paint_rect(border_paint_rect),
                |edge| {
                    let edge = edge.clip.bounds;
                    let x = border_paint_rect.min_x().min(edge.x());
                    let y = border_paint_rect.min_y().min(edge.y());
                    let right = border_paint_rect.max_x().max(edge.x() + edge.width());
                    let top = border_paint_rect.max_y().max(edge.y() + edge.height());
                    PaintClip::new(x, y, right - x, top - y)
                },
            ),
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

    /// Resolve image primitives together with the finite-tile geometry used
    /// to choose the normal-box PDF decoration phase.
    pub(in crate::layout) fn resolved_background_image_paint_with_paint_areas(
        &self,
        positioning_border_rect: PaintRect,
        clip_border_rect: PaintRect,
        style: &ComputedStyle,
    ) -> ResolvedBackgroundImagePaint {
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
                fixed_background_page_margin_box(
                    PaintPoint::new(0.0, 0.0),
                    self.current_page_context.size,
                )
            });
        background_image_paint_for_style_with_paint_areas_and_fixed_positioning_area(
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

    /// Paint the complete principal-box decoration of a replaced element.
    ///
    /// Replaced elements participate in the same CSS Backgrounds painting
    /// order as ordinary boxes. In particular, `border-image` replaces the
    /// visible normal-border paint without changing the box used for layout;
    /// bypassing the shared decoration path drops that replacement entirely.
    /// <https://drafts.csswg.org/css-backgrounds-3/#the-background>
    /// <https://drafts.csswg.org/css-backgrounds-3/#border-images>
    fn paint_replaced_box_decoration(
        &mut self,
        border_rect: PaintRect,
        style: &ComputedStyle,
        band: PaintBand,
    ) {
        if style.visibility != Visibility::Visible {
            return;
        }
        for primitive in self.box_background_primitives(border_rect, style) {
            self.push_primitive_in_band(band, primitive);
        }
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
        // A generated image's source coordinates address its concrete CSS
        // object size, while an external raster image's coordinates address
        // its intrinsic sample grid. The generated image may internally be
        // supersampled, so retain the distinction through the slice/tiling
        // stages and map to samples only when emitting PDF image crops.
        let source_is_generated = !matches!(
            src.selected_image(),
            BackgroundImage::Url(_)
                | BackgroundImage::ImageFunction(css::ImageFunction {
                    source: Some(_),
                    ..
                })
        );
        // Numeric `border-image-outset` values use the computed border width,
        // not the used width that `border-style: none` suppresses for layout.
        // The resulting area is also the percentage basis for image widths and
        // the default object size of dimensionless sources.
        // <https://drafts.csswg.org/css-backgrounds-3/#border-image-outset>
        // <https://drafts.csswg.org/css-backgrounds-3/#border-image-width>
        let computed_borders = computed_border_widths(style);
        let outsets = used_border_image_outsets(style, computed_borders);
        let area_width = border_rect.size.width + outsets.left + outsets.right;
        let area_height = border_rect.size.height + outsets.top + outsets.bottom;
        let resolved = match src.selected_image() {
            // Preserve the existing direct-URL loading path exactly. Besides
            // avoiding a second selection boundary, it retains border-image's
            // established SVG fragment and source-root behavior.
            BackgroundImage::Url(url) => load_resolved_image_source_with_request(
                &url.href,
                url.base_url
                    .as_ref()
                    .or(style.border_image.source_base_url.as_ref())
                    .or(self.base_url),
                url.root_url
                    .as_ref()
                    .or(style.border_image.source_root_url.as_ref()),
                self.resource_cache,
                crate::layout::asset_helpers::raster_orientation_policy(style.image_orientation),
                crate::svg::SvgImageContext::from_used_color_scheme(style.used_color_scheme),
                &url.request_modifiers,
            ),
            BackgroundImage::ImageFunction(_) => {
                match resolve_css_image_source(
                    src.selected_image(),
                    ImageResolutionContext {
                        base_url: style
                            .border_image
                            .source_base_url
                            .as_ref()
                            .or(self.base_url),
                        root_url: style.border_image.source_root_url.as_ref(),
                        current_color: style.color,
                        orientation: crate::layout::asset_helpers::raster_orientation_policy(
                            style.image_orientation,
                        ),
                        svg_context: crate::svg::SvgImageContext::from_used_color_scheme(
                            style.used_color_scheme,
                        ),
                        resource_cache: self.resource_cache,
                    },
                ) {
                    ResolvedCssImage::External(asset) => Some(asset),
                    // Border-image's color fallback is a dimensionless image;
                    // rasterizing it at the border-image area's concrete size
                    // gives its slices the same source-space semantics as
                    // existing generated CSS images.
                    ResolvedCssImage::SolidColor(_) => rasterize_generated_css_image(
                        src.selected_image(),
                        PaintSize::new(
                            area_width / css::CSS_PX_TO_PT,
                            area_height / css::CSS_PX_TO_PT,
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
                    ResolvedCssImage::Invalid => None,
                }
            }
            image => rasterize_generated_css_image(
                image,
                PaintSize::new(
                    area_width / css::CSS_PX_TO_PT,
                    area_height / css::CSS_PX_TO_PT,
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
        let asset = match asset {
            ResolvedImageAsset::Raster(asset) => ResolvedImageAsset::Raster(asset),
            // Resolve the vector source's default concrete object size before
            // `border-image-slice` resolves its vector-coordinate offsets.
            // This preserves a supplied intrinsic axis or aspect ratio while
            // using the border-image area as the default object size.
            // <https://www.w3.org/TR/css-backgrounds-3/#border-image-slice>
            // <https://www.w3.org/TR/css-images-3/#default-sizing>
            ResolvedImageAsset::Svg(asset) => {
                let dimensions = asset.intrinsic_dimensions();
                if dimensions.width.is_some() && dimensions.height.is_some() {
                    ResolvedImageAsset::Svg(asset)
                } else {
                    let concrete_size = CssImageNaturalDimensions::from_layout_axes(
                        dimensions.width,
                        dimensions.height,
                        dimensions.aspect_ratio,
                    )
                    .default_size(PaintSize::new(area_width, area_height));
                    ResolvedImageAsset::Svg(Rc::new(asset.with_css_image_viewport(concrete_size)))
                }
            }
        };
        let (source_width, source_height) = match &asset {
            ResolvedImageAsset::Raster(decoded) => {
                let (source_width, source_height) = if source_is_generated {
                    (
                        (area_width / css::CSS_PX_TO_PT).max(0.0),
                        (area_height / css::CSS_PX_TO_PT).max(0.0),
                    )
                } else {
                    (
                        decoded.pixel_size.width as f32,
                        decoded.pixel_size.height as f32,
                    )
                };
                (source_width, source_height)
            }
            ResolvedImageAsset::Svg(asset) => {
                let size = asset.source_viewport_size();
                (size.width.max(0.0), size.height.max(0.0))
            }
        };
        let source_image_bounds = BorderImageSourceRect::new(0.0, 0.0, source_width, source_height);
        let slices = used_border_image_slices(
            style.border_image.slice.offsets,
            source_width,
            source_height,
        );
        let image_widths = fit_border_image_widths_to_area(
            used_border_image_widths(
                style,
                computed_borders,
                border_box_pt(area_width),
                border_box_pt(area_height),
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
        let source_x = [0.0, slices.left, (source_width - slices.right).max(0.0)];
        let source_width = [
            slices.left,
            (source_width - slices.left - slices.right).max(0.0),
            slices.right,
        ];
        let source_y = [(source_height - slices.bottom).max(0.0), slices.top, 0.0];
        let source_height = [
            slices.bottom,
            (source_height - slices.top - slices.bottom).max(0.0),
            slices.top,
        ];

        // The border-image process first scales the top/bottom edge images in
        // their cross axis and the left/right edge images in their cross axis.
        // The center image inherits those horizontal and vertical factors.
        // Repeat keywords are applied only afterwards.
        // <https://drafts.csswg.org/css-backgrounds-3/#border-image-process>
        let source_to_paint = |value: f32| value * css::CSS_PX_TO_PT;
        let axis_scale = |destination: f32, source: f32| {
            (source > 0.0).then_some(destination / source_to_paint(source))
        };
        let horizontal_scale = border_image_center_axis_scale(
            dest_height[2],
            source_to_paint(source_height[2]),
            dest_height[0],
            source_to_paint(source_height[0]),
        );
        let vertical_scale = border_image_center_axis_scale(
            dest_width[0],
            source_to_paint(source_width[0]),
            dest_width[2],
            source_to_paint(source_width[2]),
        );

        let mut primitives = Vec::new();
        for row in 0..3 {
            for col in 0..3 {
                if row == 1 && col == 1 && !style.border_image.slice.fill {
                    continue;
                }
                if dest_width[col] <= 0.0
                    || dest_height[row] <= 0.0
                    || source_width[col] <= 0.0
                    || source_height[row] <= 0.0
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
                let source = BorderImageSourceRect::new(
                    source_x[col],
                    source_y[row],
                    source_width[col],
                    source_height[row],
                );
                let tile_size = match (row, col) {
                    // Corners are stretched in both axes.
                    (0 | 2, 0 | 2) => PaintSize::new(dest_width[col], dest_height[row]),
                    // Top/bottom regions retain their aspect ratio after the
                    // cross-axis scale to their destination height.
                    (0 | 2, 1) => PaintSize::new(
                        source_to_paint(source.width)
                            * axis_scale(dest_height[row], source.height).unwrap_or(0.0),
                        dest_height[row],
                    ),
                    // Left/right regions retain their aspect ratio after the
                    // cross-axis scale to their destination width.
                    (1, 0 | 2) => PaintSize::new(
                        dest_width[col],
                        source_to_paint(source.height)
                            * axis_scale(dest_width[col], source.width).unwrap_or(0.0),
                    ),
                    // The center takes the edge scale factors established in
                    // the first stage, independently per axis.
                    (1, 1) => PaintSize::new(
                        source_to_paint(source.width) * horizontal_scale,
                        source_to_paint(source.height) * vertical_scale,
                    ),
                    _ => unreachable!(),
                };
                match &asset {
                    ResolvedImageAsset::Raster(decoded) => {
                        let mut images = Vec::new();
                        push_border_image_tiles(
                            &mut images,
                            self.resource_cache,
                            RasterBorderImageTilePaint {
                                decoded,
                                destination,
                                source_image_bounds,
                                source,
                                tile_size,
                                repeat_x,
                                repeat_y,
                                sampling: raster_image_sampling(style),
                            },
                        );
                        primitives.extend(images.into_iter().map(PaintPrimitive::Image));
                    }
                    ResolvedImageAsset::Svg(asset) => {
                        push_svg_border_image_tiles(
                            &mut primitives,
                            asset,
                            destination,
                            source,
                            tile_size,
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
