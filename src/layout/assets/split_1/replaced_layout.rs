use super::*;

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
    used: ComputedStyle,
}

impl PositionedUsedStyle {
    fn from_source_and_used(source: ComputedStyle, used: ComputedStyle) -> Self {
        debug_assert!(used.zoom_applied);
        Self { source, used }
    }

    pub(in crate::layout) fn source(&self) -> &ComputedStyle {
        &self.source
    }

    pub(in crate::layout) fn used(&self) -> &ComputedStyle {
        &self.used
    }
}
use crate::css::ObjectFit;
use crate::layout::block::suppress_fragmented_box_edges;
use crate::layout::builder::page_for_context;
use crate::svg::{SharedSvgAsset, SvgSourcePoint, SvgSourceRect, SvgSourceSize};
use crate::units::LayoutSize;
use std::rc::Rc;

pub(in crate::layout) struct PositionedPaginationState {
    pages: Vec<Page>,
    page_names: Vec<Option<String>>,
    page_blanks: Vec<bool>,
    page_named_strings: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    page_running_elements: Vec<HashMap<String, Vec<NamedStringAssignment>>>,
    current_page: Page,
    current_page_has_flow_content: bool,
    current_page_has_named_page_flow_content: bool,
    current_page_context: PageContext,
    current_page_named_strings: HashMap<String, Vec<NamedStringAssignment>>,
    current_page_running_elements: HashMap<String, Vec<NamedStringAssignment>>,
    cursor_y: f32,
    content_left: f32,
    content_right: f32,
    fragment_top_offsets: Vec<f32>,
    truncate_page_start_margins: bool,
    pending_paint_fragments: Vec<PendingPaintFragment>,
    pending_page_side_effects: Vec<PendingPageSideEffects>,
    pending_positioned_page_span_target: Option<usize>,
}

/// Retain page fragments established by nested out-of-flow layout when its
/// scratch pagination state is restored to the enclosing formatting context.
///
/// Each nested positioned subtree can extend the final document independently,
/// so neither requirement may replace the other.
/// <https://www.w3.org/TR/css-position-3/#fragmenting-absolutely-positioned-elements>
fn merged_positioned_page_span_target(
    enclosing: Option<usize>,
    nested: Option<usize>,
) -> Option<usize> {
    enclosing.into_iter().chain(nested).max()
}

impl<'a> LayoutBuilder<'a> {
    /// Prepare an out-of-flow box at the common positioned used-value boundary.
    ///
    /// Every absolute/fixed entry ultimately reaches `layout_positioned_block`.
    /// This makes its viewport and zoom normalization authoritative while the
    /// frozen box tree continues to own descendant cascade state.
    pub(in crate::layout) fn positioned_used_style(
        &self,
        source: &ComputedStyle,
    ) -> PositionedUsedStyle {
        let used = if source.zoom_applied {
            source.clone()
        } else {
            self.style_with_current_viewport_lengths(source)
        };
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
        apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_width)),
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
            && (style.background_color.is_some() || used_border_width(style) > layout_pt(0.0))
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
            fragment.promote_background_border_to_in_flow_block();
            fragment = fragment
                .with_contents_effect_scoped_to_rect(PaintClip::from_paint_rect(content_rect));
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
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_width)),
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
        let border_widths = box_metrics.border;
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
            if style.background_color.is_some() || used_border_width(style) > layout_pt(0.0) {
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
            // A size-contained replaced box may retain its padding and
            // background while its content box is empty. SVG's viewport
            // painter treats a zero destination as an omitted viewport, so
            // do not let it fall back to its natural dimensions here.
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            if content_width > 0.0 && content_height > 0.0 {
                if let Some(asset) = image.svg {
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
                        style.image_rendering != css::ImageRendering::Pixelated,
                        image.decoded.rgb,
                        image.decoded.alpha,
                        element.attrs.get("alt").cloned().map(Rc::from),
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
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_width)),
        );
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
        let border_origin =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let border_x = border_origin.x;
        let border_y = border_origin.y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible {
            if style.background_color.is_some() || used_border_width(style) > layout_pt(0.0) {
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
                if let Some(asset) = image.svg {
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
                        style.image_rendering != css::ImageRendering::Pixelated,
                        image.decoded.rgb,
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
        let box_metrics = apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(available_width)),
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
        let border_widths = box_metrics.border;
        let content_width = geometry.content_size.width;
        let content_height = geometry.content_size.height;
        let border_box_width = geometry.border_box_size.width;
        let border_box_height = geometry.border_box_size.height;
        let border_origin =
            self.place_block_replaced_box(style, border_box_width, border_box_height);
        let border_x = border_origin.x;
        let border_y = border_origin.y;
        let paint_checkpoint = self.current_page.paint_checkpoint();
        if style.visibility == Visibility::Visible
            && (style.background_color.is_some() || used_border_width(style) > layout_pt(0.0))
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
        if style.visibility == Visibility::Visible && content_width > 0.0 && content_height > 0.0 {
            let content_x = border_x + border_widths.left + style.padding.left;
            let content_y = border_y + border_widths.bottom + style.padding.bottom;
            // An inline SVG with omitted root dimensions is parsed with a
            // provisional SVG viewport. Reparse it for this concrete CSS
            // replaced-object size before resolving percentage geometry such
            // as `<rect width="100%">`; otherwise a 1 by 1 viewBox leaks
            // into paint even when Flexbox has assigned a 100px item.
            // <https://www.w3.org/TR/SVG2/coords.html#ViewportSpace>
            let viewport_asset = asset.with_replaced_viewport(content_width, content_height);
            self.push_svg_group_in_band(
                PaintBand::InFlowBlock,
                viewport_asset.paint_group(paint_space_rect(
                    content_x,
                    content_y,
                    content_width,
                    content_height,
                )),
            );
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

    pub(in crate::layout) fn place_block_replaced_box(
        &mut self,
        style: &ComputedStyle,
        border_box_width: f32,
        border_box_height: f32,
    ) -> PaintPoint {
        let margin_box_width = style.margin.left + border_box_width + style.margin.right;
        let margin_box_height = style.margin.top + border_box_height + style.margin.bottom;
        self.cursor_y -= style.margin.top;
        self.prebreak_bfc_margin_box_if_needed(margin_box_height, style.margin.top);
        let (margin_box_left, avoided_top, _) = self.place_float_avoiding_margin_box(
            self.cursor_y,
            PageTopSize::new(margin_box_width, margin_box_height),
            style.clear,
            style.writing_mode,
            style.direction,
            self.containing_block_direction,
        );
        self.cursor_y = avoided_top;
        // Replaced elements participate in normal flow just like ordinary
        // block boxes. Record a used positive block fragment before its
        // painting is emitted so named-page boundaries and fragmentation do
        // not treat a replaced box as transparent.
        // <https://www.w3.org/TR/CSS2/visuren.html#block-boxes>
        if border_box_height > 0.0 {
            self.mark_current_page_flow_content();
        }
        PaintPoint::new(
            margin_box_left + style.margin.left,
            self.cursor_y - border_box_height,
        )
    }

    pub(in crate::layout) fn background_image_primitives(
        &self,
        border_rect: PaintRect,
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
            PaintBackgroundArea::from_paint_rect(border_rect),
            PaintBackgroundArea::from_paint_rect(border_rect),
            Some(fixed_positioning_area),
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
        border_rect: PaintRect,
        style: &ComputedStyle,
    ) -> Vec<PaintPrimitive> {
        let Some(src) = style.border_image.source.as_ref() else {
            return Vec::new();
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
                true,
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
            None => return Vec::new(),
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
                border_rect.size.width,
                border_rect.size.height,
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
                            background_raster_interpolation(style),
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
        primitives
    }

    pub(in crate::layout) fn layout_positioned_block(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        stylesheets: &[Stylesheet],
        child_boxes: Option<&[box_tree::FormattingBox<'_>]>,
        table_fragment: Option<&box_tree::TableFragment<'_>>,
    ) {
        let positioned_style = self.positioned_used_style(style);
        let source_style = positioned_style.source();
        let style = positioned_style.used();
        let paint_checkpoint = self.current_page.paint_checkpoint();
        let paint_page_index = self.pages.len();
        let static_scroll_snap_scope = self.begin_static_scroll_snap_scope(style, false);
        let positioned_layer_start = self.positioned_layers.len();
        let pagination_state = self.positioned_pagination_state();
        let previous_left = self.content_left;
        let previous_right = self.content_right;
        let previous_cursor_y = self.cursor_y;
        let previous_inline_static_position = self.inline_static_position;
        let previous_block_static_position_y_offset = self.block_static_position_y_offset;
        let previous_absolute_static_position = self.absolute_static_position;

        let locally_contained_fixed_block = (style.position == Position::Fixed)
            .then(|| self.fixed_containing_blocks.last().cloned())
            .flatten();
        if locally_contained_fixed_block.is_some() {
            // Intrinsic/table planning can previously have materialized this
            // same fixed element against the initial containing block. Once a
            // committed transformed containing block is available, that
            // viewport-fixed layer is stale and must not survive alongside
            // the locally captured positioned layer.
            self.fixed_layers
                .retain(|layer| layer.source_element != element.id);
        }
        let source_containing_block = if style.position == Position::Fixed {
            locally_contained_fixed_block.unwrap_or_else(|| self.page_containing_block())
        } else {
            self.containing_blocks
                .last()
                .cloned()
                .unwrap_or_else(|| self.page_containing_block())
        };
        let uses_initial_page_containing_block =
            matches!(style.position, Position::Absolute | Position::Fixed)
                && self.containing_blocks.is_empty()
                && locally_contained_fixed_block.is_none();
        let grid_positioning_context = (style.position == Position::Absolute)
            .then(|| {
                self.grid_positioning_scopes.iter().rev().find_map(|scope| {
                    scope.descendant_positioning_context(style, source_containing_block)
                })
            })
            .flatten();
        // A qualifying positioned descendant receives Grid's actual area
        // containing block and a distinct grid-derived static rectangle.
        // <https://www.w3.org/TR/css-grid-1/#abspos>
        let containing_block = grid_positioning_context
            .and_then(|context| context.grid_area_containing_block)
            .unwrap_or(source_containing_block);
        let containing_block_fragment_origin_page_index = containing_block
            .origin_page_index
            .unwrap_or(paint_page_index);
        // Preserve whether the authored axis size is automatic before used
        // values normalize replaced-element intrinsic dimensions. Grid's
        // abspos self-alignment must run only after that intrinsic size is
        // known for an automatic axis.
        // <https://www.w3.org/TR/css-grid-1/#abspos-items> and
        // <https://drafts.csswg.org/css-align-3/#abspos-align>.
        let horizontal_size_was_auto = source_style.box_values.width.is_auto();
        let vertical_size_was_auto = source_style.box_values.height.is_auto();
        let mut used_style = style.clone();
        // Grid self-alignment of an automatically sized positioned item must
        // wait for intrinsic sizing below. Replaced elements receive used
        // intrinsic dimensions into `used_style`, so retain the authored auto
        // state before that normalization.
        apply_used_box_metrics(
            &mut used_style,
            PercentageBasis::definite(layout_pt(containing_block.width())),
        );
        if used_style.display.is_inline_level() {
            // CSS Display blockifies the outer display type of absolutely
            // positioned boxes for layout while preserving the static-position
            // source separately:
            // https://www.w3.org/TR/css-display-3/#transformations
            used_style.display = used_style.display.blockified();
        }
        // Resolve a non-replaced positioned box's auto axis from a definite
        // authored opposite axis before solving the absolute inset equations.
        // Inset-derived fill sizes remain part of those equations and are not
        // treated as an authored preferred size here.
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
        // <https://www.w3.org/TR/css-position-3/#abspos-layout>
        if !is_replaced_element(element) {
            let horizontal_non_content = used_style.padding.left
                + used_style.padding.right
                + horizontal_border_width(&used_style);
            let vertical_non_content = used_style.padding.top
                + used_style.padding.bottom
                + vertical_border_width(&used_style);
            let definite_width = used_content_box_width_or_auto(
                &used_style,
                layout_pt(containing_block.width()),
                non_content_pt(horizontal_non_content),
            )
            .map(|width| {
                constrain_content_width(
                    &used_style,
                    width,
                    PercentageBasis::definite(layout_pt(containing_block.width())),
                )
                .points()
            });
            let definite_height = used_content_box_height_or_auto(
                &used_style,
                layout_pt(containing_block.height()),
                non_content_pt(vertical_non_content),
            )
            .map(|height| {
                constrain_content_height(
                    &used_style,
                    height,
                    PercentageBasis::definite(layout_pt(containing_block.height())),
                )
                .points()
            });
            match (horizontal_size_was_auto, vertical_size_was_auto) {
                (true, false) => {
                    if let Some(height) = definite_height
                        && let Some(width) = non_replaced_aspect_ratio_content_width(
                            &used_style,
                            height,
                            horizontal_non_content,
                            vertical_non_content,
                        )
                    {
                        set_style_used_width(&mut used_style, width);
                        set_style_used_height(&mut used_style, height);
                        used_style.box_sizing = BoxSizing::ContentBox;
                    }
                }
                (false, true) => {
                    if let Some(width) = definite_width
                        && let Some(height) = non_replaced_aspect_ratio_content_height(
                            &used_style,
                            width,
                            horizontal_non_content,
                            vertical_non_content,
                        )
                    {
                        set_style_used_width(&mut used_style, width);
                        set_style_used_height(&mut used_style, height);
                        used_style.box_sizing = BoxSizing::ContentBox;
                    }
                }
                (true, true) => {
                    let horizontal_fill = used_inset_left(&used_style, containing_block)
                        .zip(used_inset_right(&used_style, containing_block))
                        .map(|(left, right)| {
                            constrain_content_width(
                                &used_style,
                                content_box_pt(
                                    (containing_block.width()
                                        - left
                                        - used_style.margin.left
                                        - horizontal_non_content
                                        - used_style.margin.right
                                        - right)
                                        .max(0.0),
                                ),
                                PercentageBasis::definite(layout_pt(containing_block.width())),
                            )
                            .points()
                        });
                    let vertical_fill = used_inset_top(&used_style, containing_block)
                        .zip(used_inset_bottom(&used_style, containing_block))
                        .map(|(top, bottom)| {
                            constrain_content_height(
                                &used_style,
                                content_box_pt(
                                    (containing_block.height()
                                        - top
                                        - used_style.margin.top
                                        - vertical_non_content
                                        - used_style.margin.bottom
                                        - bottom)
                                        .max(0.0),
                                ),
                                PercentageBasis::definite(layout_pt(containing_block.height())),
                            )
                            .points()
                        });
                    // When both dimensions are auto, a definite inline fill
                    // size is the ratio's primary axis. This is also the
                    // tie-breaker when both inset pairs are definite.
                    if let Some(mut width) = horizontal_fill {
                        if let Some(mut height) = non_replaced_aspect_ratio_content_height(
                            &used_style,
                            width,
                            horizontal_non_content,
                            vertical_non_content,
                        ) {
                            height = constrain_content_height(
                                &used_style,
                                content_box_pt(height),
                                PercentageBasis::definite(layout_pt(containing_block.height())),
                            )
                            .points();
                            if let Some(constrained_width) = non_replaced_aspect_ratio_content_width(
                                &used_style,
                                height,
                                horizontal_non_content,
                                vertical_non_content,
                            ) {
                                width = constrain_content_width(
                                    &used_style,
                                    content_box_pt(constrained_width),
                                    PercentageBasis::definite(layout_pt(containing_block.width())),
                                )
                                .points();
                            }
                            set_style_used_width(&mut used_style, width);
                            set_style_used_height(&mut used_style, height);
                            used_style.box_sizing = BoxSizing::ContentBox;
                        }
                    } else if let Some(mut height) = vertical_fill
                        && let Some(mut width) = non_replaced_aspect_ratio_content_width(
                            &used_style,
                            height,
                            horizontal_non_content,
                            vertical_non_content,
                        )
                    {
                        width = constrain_content_width(
                            &used_style,
                            content_box_pt(width),
                            PercentageBasis::definite(layout_pt(containing_block.width())),
                        )
                        .points();
                        if let Some(constrained_height) = non_replaced_aspect_ratio_content_height(
                            &used_style,
                            width,
                            horizontal_non_content,
                            vertical_non_content,
                        ) {
                            height = constrain_content_height(
                                &used_style,
                                content_box_pt(constrained_height),
                                PercentageBasis::definite(layout_pt(containing_block.height())),
                            )
                            .points();
                        }
                        set_style_used_width(&mut used_style, width);
                        set_style_used_height(&mut used_style, height);
                        used_style.box_sizing = BoxSizing::ContentBox;
                    }
                }
                _ => {}
            }
        }
        let positioned_available_outer_width =
            (containing_block.width() - used_style.margin.left - used_style.margin.right)
                .max(used_style.font_size);
        let replaced_content_size = if is_replaced_element(element) {
            used_image(
                element,
                &used_style,
                positioned_available_outer_width,
                PercentageBasis::definite_from(
                    content_box_pt(containing_block.height()),
                    BlockSizeBasisSource::AbsolutePositioned,
                ),
                self.base_url,
                self.root_url,
                self.resource_cache,
            )
            .map(|image| {
                // CSS 2.2 gives absolutely positioned replaced elements their
                // own auto-size rules: intrinsic dimensions and aspect ratio
                // resolve the content size before the absolute inset equation
                // is solved.
                // <https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-width>
                // and <https://www.w3.org/TR/CSS22/visudet.html#abs-replaced-height>.
                set_style_used_width(&mut used_style, image.content_size.width);
                set_style_used_height(&mut used_style, image.content_size.height);
                (image.content_size.width, image.content_size.height)
            })
        } else {
            None
        };
        let style = &used_style;
        let left_inset = used_inset_left(style, containing_block);
        let right_inset = used_inset_right(style, containing_block);
        let top_inset = used_inset_top(style, containing_block);
        let bottom_inset = used_inset_bottom(style, containing_block);
        let horizontal_insets_are_auto = left_inset.is_none() && right_inset.is_none();
        let vertical_insets_are_auto = top_inset.is_none() && bottom_inset.is_none();
        // With both block-axis insets auto, the static-position rectangle is
        // generated on the source fragment's page even when the containing
        // block is the initial page area. Explicit insets remain anchored to
        // the containing block's block-start page.
        // <https://drafts.csswg.org/css-position-3/#static-position-rectangle>
        let containing_block_origin_page_index =
            if style.position == Position::Absolute && vertical_insets_are_auto {
                paint_page_index
            } else {
                containing_block_fragment_origin_page_index
            };
        // Direct grid children install their already-resolved static rectangle
        // explicitly. For positioned descendants, a live grid scope supplies
        // the same rectangle only while that grid still provides the actual
        // containing block.
        // <https://www.w3.org/TR/css-grid-1/#abspos>
        let absolute_static_position = self
            .absolute_static_position
            .or_else(|| grid_positioning_context.map(|context| context.static_position));
        // A hypothetical normal-flow position can lie outside the containing
        // block, for example through a negative margin on an intervening block
        // ancestor. CSS Positioned Layout uses that position directly when
        // resolving two automatic inline insets; it must not be clamped back
        // into the containing block.
        // <https://www.w3.org/TR/css-position-3/#static-position-rectangle>
        let source_static_position = AbsoluteStaticPosition::from_page_rect_with_horizontal_outside(
            previous_left,
            previous_right,
            previous_cursor_y,
            true,
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
        let mut static_horizontal_position =
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
            && let Some(position) =
                absolute_static_position.filter(|position| position.has_vertical_position())
        {
            position.vertical_start(containing_block)
        } else if inline_static_uses_margin_box_top && let Some(position) = inline_static_position {
            containing_block.top_y() - position.top_y
        } else {
            static_vertical_base.max(0.0)
        };
        if absolute_static_position.is_none_or(|position| !position.has_vertical_position())
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
        // Collapsed table borders are grid-edge borders, not ordinary
        // content-to-border-box insets. Treating them as generic absolute
        // positioning insets shrinks an explicitly sized table before its
        // column algorithm sees the requested grid width.
        // <https://www.w3.org/TR/css-tables-3/#table-wrapper-box>
        let horizontal_non_content = if is_html_table_element(element)
            && style.border_collapse == css::BorderCollapse::Collapse
        {
            0.0
        } else {
            style.padding.left + style.padding.right + horizontal_border_width(style)
        };
        let vertical_border_width_for_positioning =
            self.positioned_vertical_border_width(element, style, stylesheets, table_fragment);
        let positioned_height_percentage_basis =
            absolute_positioned_content_height_percentage_basis(
                style,
                containing_block,
                vertical_border_width_for_positioning,
            );
        if positioned_height_percentage_basis.is_definite() {
            self.definite_block_size_stack
                .push(positioned_height_percentage_basis);
        }
        let auto_or_intrinsic_width = replaced_content_size.map_or_else(
            || {
                self.used_intrinsic_or_shrink_to_fit_width(
                    element,
                    style,
                    stylesheets,
                    layout_pt(positioned_available_outer_width),
                    non_content_pt(horizontal_non_content),
                    child_boxes,
                    table_fragment,
                )
                .points()
            },
            |(width, _)| width,
        );
        if positioned_height_percentage_basis.is_definite() {
            self.definite_block_size_stack.pop();
        }
        if horizontal_insets_are_auto
            && horizontal_size_was_auto
            && let Some(grid_alignment) =
                absolute_static_position.and_then(AbsoluteStaticPosition::grid_alignment)
        {
            static_horizontal_position = grid::grid_abspos_late_horizontal_static_position(
                grid_alignment,
                style,
                containing_block,
                auto_or_intrinsic_width + horizontal_non_content,
            );
        }
        let mut positioned_x = resolve_absolute_horizontal_with_non_content(
            style,
            containing_block,
            auto_or_intrinsic_width,
            // An auto preferred size resolved from the opposite axis through
            // a preferred aspect ratio still has its normal content-based
            // automatic minimum. This must be applied after intrinsic width
            // measurement, rather than treating the ratio transfer as an
            // authored definite width.
            // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
            (horizontal_size_was_auto
                && style.box_values.min_width.is_auto()
                && !style.overflow_x.is_scrollable()
                && style
                    .aspect_ratio
                    .preferred_ratio_for_non_replaced(false)
                    .is_some())
            .then_some(auto_or_intrinsic_width),
            static_horizontal_position,
            self.containing_block_direction,
            horizontal_non_content,
        );
        // An absolutely positioned table is laid out against the containing
        // block's available size, even when the generic absolute-position
        // equation has two opposing insets that would span a larger range.
        // Keep the table layout algorithm's auto width instead of replacing it
        // with that range; the insets still determine the table's position.
        // <https://drafts.csswg.org/css-tables-3/#abspos>
        if table_fragment.is_some() && horizontal_size_was_auto {
            positioned_x.size = auto_or_intrinsic_width;
        }
        let mut positioned_content_width = positioned_x.size;

        // Measure under the same out-of-flow named-page suppression as the
        // final positioned subtree, so page-name descendants cannot inflate
        // the measured fragment span:
        // <https://www.w3.org/TR/css-page-3/#using-named-pages>
        self.push_page_name_scope_suppression();
        let positioned_is_orthogonal = positioned_box_is_orthogonal_to_containing_block(
            self.containing_block_writing_mode,
            style.writing_mode,
        );
        let auto_height = if vertical_size_was_auto && let Some((_, height)) = replaced_content_size
        {
            height
        } else {
            let mut estimate_style = style.clone();
            estimate_style.position = Position::Static;
            estimate_style.margin = css::Edges::ZERO;
            estimate_style.box_values.margin =
                css::CssEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
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
            // Size-contained positioned boxes use the measured size of their
            // empty principal formatting context. A font-sized floor would
            // incorrectly reintroduce descendant/font intrinsic size.
            // <https://www.w3.org/TR/css-contain-1/#containment-size>
            .max(if style.contain.size {
                0.0
            } else {
                style.line_height
            })
        };
        self.pop_page_name_scope_suppression();
        if vertical_insets_are_auto
            && vertical_size_was_auto
            && let Some(grid_alignment) =
                absolute_static_position.and_then(AbsoluteStaticPosition::grid_alignment)
        {
            static_vertical_start = grid::grid_abspos_late_vertical_static_start(
                grid_alignment,
                style,
                containing_block,
                auto_height
                    + style.padding.top
                    + style.padding.bottom
                    + vertical_border_width_for_positioning,
            );
        }
        let positioned_y = resolve_absolute_vertical(
            style,
            containing_block,
            auto_height,
            // See the corresponding inline-axis minimum above. The block
            // automatic minimum is the measured in-flow content height.
            // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
            (vertical_size_was_auto
                && style.box_values.min_height.is_auto()
                && !style.overflow_y.is_scrollable()
                && style
                    .aspect_ratio
                    .preferred_ratio_for_non_replaced(false)
                    .is_some())
            .then_some(auto_height),
            static_vertical_start,
            vertical_border_width_for_positioning,
        );
        let positioned_content_height = positioned_y.size;
        // A min/max constraint on an automatic block size transfers through
        // the preferred aspect ratio to constrain the automatic inline size.
        // The absolute-position equations are solved independently, so feed
        // the final constrained block result back into the inline equation
        // once both authored preferred sizes were automatic.
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio-size-transfers>
        if horizontal_size_was_auto
            && vertical_size_was_auto
            && let Some(unconstrained_height) = non_replaced_aspect_ratio_content_height(
                style,
                positioned_content_width,
                horizontal_non_content,
                style.padding.top + style.padding.bottom + vertical_border_width_for_positioning,
            )
            && (positioned_content_height - unconstrained_height).abs() > 0.01
            && let Some(transferred_width) = non_replaced_aspect_ratio_content_width(
                style,
                positioned_content_height,
                horizontal_non_content,
                style.padding.top + style.padding.bottom + vertical_border_width_for_positioning,
            )
        {
            let constrained_width = if positioned_content_height < unconstrained_height {
                positioned_content_width.min(transferred_width)
            } else {
                positioned_content_width.max(transferred_width)
            };
            if (constrained_width - positioned_content_width).abs() > 0.01 {
                let mut ratio_constrained_style = style.clone();
                set_style_used_width(&mut ratio_constrained_style, constrained_width);
                ratio_constrained_style.box_sizing = BoxSizing::ContentBox;
                positioned_x = resolve_absolute_horizontal_with_non_content(
                    &ratio_constrained_style,
                    containing_block,
                    auto_or_intrinsic_width,
                    None,
                    static_horizontal_position,
                    self.containing_block_direction,
                    horizontal_non_content,
                );
                positioned_content_width = positioned_x.size;
            }
        }
        let positioned_page_offset = if style.position == Position::Absolute {
            let (page_offset, remainder) =
                self.absolute_positioned_page_start_offset(containing_block, positioned_y);
            // A static-position rectangle exactly on a fragmentainer end
            // belongs to that current fragmentainer. Its contents can then
            // fragment forward normally; eagerly shifting the principal box
            // first manufactures an empty intervening page.
            // <https://www.w3.org/TR/css-position-3/#static-position>
            if vertical_insets_are_auto && page_offset > 0 && remainder <= 0.01 {
                page_offset - 1
            } else {
                page_offset
            }
        } else {
            0
        };
        let positioned_origin_page_index =
            containing_block_origin_page_index + positioned_page_offset;
        let positioned_border_box_width = positioned_content_width + horizontal_non_content;
        self.content_left = containing_block.x() + positioned_x.start + positioned_x.margin_start;
        self.content_right = self.content_left + positioned_border_box_width;
        let positioned_margin_top = if inline_auto_static_y && !inline_static_uses_margin_box_top {
            ((style.line_height - style.font_size) / 2.0).max(0.0)
        } else {
            positioned_y.margin_start
        };
        self.cursor_y = containing_block.top_y() - positioned_y.start - positioned_margin_top;
        if !inline_auto_static_y
            && vertical_insets_are_auto
            && !(left_inset.is_some() && right_inset.is_some())
            && absolute_static_position.is_none()
            && positioned_is_orthogonal
        {
            self.cursor_y += positioned_content_height;
        }
        // Enter the destination fragmentainer's local coordinate space before
        // laying out the positioned subtree. Keeping a continuous coordinate
        // below the source page and translating captured paint afterward loses
        // exact-boundary placement and gives descendants the wrong containing
        // block geometry.
        // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
        if positioned_page_offset > 0 {
            self.cursor_y += positioned_page_offset as f32 * self.page_area_height().max(1.0);
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
        // A positioned descendant is not emitted through normal-flow block
        // layout, so record its final border-box geometry here while the
        // ancestor scroll-container capture is active.  The later stacking
        // context bounds are paint bounds (and may include a different
        // coordinate remapping), whereas CSS Scroll Snap defines an area from
        // the box's border box before its scroll-margin outsets.
        // <https://www.w3.org/TR/css-scroll-snap-1/#scroll-snap-model>
        self.record_static_scroll_snap_area(element, style, positioned_border_box.paint_rect());
        self.record_static_scroll_target_area(
            element.is_target,
            positioned_border_box.paint_rect(),
            style,
        );
        // CSS Positioned Layout and CSS 2.2 Appendix E order positioned
        // boxes in tree order in their containing stacking context. Reserve
        // this box's order before laying out descendants so child positioned
        // contexts, including fixed descendants, sort after their parent.
        let positioned_source_order = self.next_paint_source_order();

        let mut flow_style = style.clone();
        flow_style.position = Position::Static;
        // This positioned principal box owns the scroll-capture boundary;
        // its blockified flow surrogate must not open a duplicate scope.
        flow_style.scroll_snap_type = css::ScrollSnapType::None;
        // The surrogate is laid out at its static-flow origin solely to
        // capture the positioned principal's contents.  It is not a second
        // CSS box, so it must not contribute a duplicate snap area at that
        // temporary origin.
        flow_style.scroll_snap_align = css::ScrollSnapAlign::default();
        flow_style.margin = css::Edges::ZERO;
        flow_style.box_values.margin =
            css::CssEdges::all(css::ComputedLengthPercentageOrAuto::ZERO);
        set_style_used_width(&mut flow_style, positioned_content_width);
        set_style_used_height(&mut flow_style, positioned_content_height);
        clear_position_insets(&mut flow_style);
        let border_widths = used_border_widths(&flow_style);
        let positioned_containing_block_top = self.cursor_y - border_widths.top;
        self.containing_blocks.push(
            ContainingBlock::from_page_top_rect(PageTopRect::new(
                self.content_left + border_widths.left,
                positioned_containing_block_top,
                positioned_content_width + flow_style.padding.left + flow_style.padding.right,
                positioned_content_height + flow_style.padding.top + flow_style.padding.bottom,
            ))
            .on_page(positioned_origin_page_index),
        );
        // A statically positioned absolute box can fragment its contents
        // through later page contexts. An explicitly block-start-pinned
        // absolute box, and every fixed box, instead belongs to one resolved
        // out-of-flow placement and must not let descendant `page` values
        // manufacture normal-flow page transitions.
        // <https://drafts.csswg.org/css-position-3/#fragmentation>
        let suppress_descendant_page_name_transitions = style.position == Position::Fixed
            || (style.position == Position::Absolute && !vertical_insets_are_auto);
        if suppress_descendant_page_name_transitions {
            self.push_page_name_scope_suppression();
        }
        let previous_overflow_clips = self.overflow_clips.clone();
        self.overflow_clips =
            positioned_applicable_overflow_clips(&previous_overflow_clips, containing_block);
        let previous_defer_block_decoration_promotion = self.defer_next_block_decoration_promotion;
        // The positioned stacking context owns its principal decoration. Keep
        // that decoration in the background/border band so overflow and paint
        // containment can clip only the captured contents below.
        // <https://www.w3.org/TR/CSS22/zindex.html>
        // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
        self.defer_next_block_decoration_promotion = true;
        // Absolutely positioned block containers establish a new block
        // formatting context. In particular, float exclusions from the source
        // formatting context cannot constrain either this box's line widths or
        // its auto block-size; floats created inside remain local to it.
        // <https://www.w3.org/TR/CSS22/visuren.html#dis-pos-flo>
        self.push_float_context();
        // The positioned stacking context assembled below owns the principal
        // transform, opacity, and clipping effects for this box.  Re-entering
        // the generic element dispatcher with its normal effect capture would
        // wrap the same principal fragment a second time, applying (for
        // example) a CSS transform twice.  Descendants still capture their
        // own effects normally.
        //
        // CSS Transforms § 3 makes the transformed element one stacking
        // context; it does not create a nested second context for its own
        // principal box.
        self.layout_element_inner_with_principal_effect_context(
            element,
            &flow_style,
            stylesheets,
            &[],
            child_boxes,
            table_fragment,
            false,
        );
        self.pop_float_context();
        self.defer_next_block_decoration_promotion = previous_defer_block_decoration_promotion;
        self.overflow_clips = previous_overflow_clips;
        if suppress_descendant_page_name_transitions {
            self.pop_page_name_scope_suppression();
        }
        self.containing_blocks.pop();
        let mut child_positioned_layers = if positioned_layer_start < self.positioned_layers.len() {
            self.positioned_layers.split_off(positioned_layer_start)
        } else {
            Vec::new()
        };
        let has_principal_decoration = style.visibility == Visibility::Visible
            && (style.background_color.is_some()
                || style.background_image.is_some()
                || style.border_image.source.is_some()
                || used_border_width(style) > layout_pt(0.0)
                || (style.outline_width > 0.0 && !style.outline_style.suppresses_used_width()));
        let principal_decoration_target_page_index = has_principal_decoration
            .then(|| {
                self.absolute_positioned_decoration_span_target(
                    style,
                    containing_block,
                    positioned_y,
                    vertical_border_width_for_positioning,
                    containing_block_origin_page_index,
                )
            })
            .flatten();
        let mut positioned_fragments =
            self.take_positioned_fragments_since(paint_page_index, paint_checkpoint);
        if let Some(target_page_index) = principal_decoration_target_page_index {
            let captured_last_page_index = positioned_fragments
                .iter()
                .map(|(page_index, _)| *page_index)
                .max()
                .unwrap_or(paint_page_index);
            self.extend_positioned_principal_decoration_fragments(
                &mut positioned_fragments,
                style,
                positioned_border_box,
                paint_page_index,
                captured_last_page_index,
                target_page_index,
                pagination_state.current_page_context,
            );
        }
        if style.position == Position::Absolute {
            // Preserve every captured slice when moving scratch pagination to
            // the absolute box's destination sequence. A slice can contain
            // only background, border, or another non-text paint primitive;
            // using the first text baseline to choose a remapping origin
            // drops those observable fragments.
            // <https://www.w3.org/TR/css-break-3/#fragmentation-model>
            // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
            self.remap_absolute_positioned_fragments(
                &mut positioned_fragments,
                paint_page_index,
                positioned_origin_page_index,
            );
            // A nested absolute containing block is itself established in a
            // destination fragmentainer. When an explicitly positioned child
            // starts in a later fragmentainer of that block, its captured
            // principal fragments begin after the containing block's current
            // destination fragment. The scratch sequence has no page for
            // that already-established containing-block fragment, so account
            // for it while assigning final page ownership.
            // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
            if containing_block_origin_page_index > 0 && positioned_page_offset > 0 {
                for (page_index, _) in &mut positioned_fragments {
                    *page_index += 1;
                }
                for layer in &mut child_positioned_layers {
                    layer.page_index += 1;
                }
            }
            // A descendant positioned box resolves its containing block from
            // this box's destination-page origin. Its layer page index is
            // therefore already final, even though the parent principal
            // fragments above were captured in scratch fragmentainers.
            // Remapping it again would shift a nested absolute box by the
            // parent page offset a second time.
            // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
        }
        let border_widths = used_border_widths(style);
        let scroll_padding_box = paint_space_rect(
            positioned_border_box.x() + border_widths.left,
            positioned_border_box.y() + border_widths.bottom,
            (positioned_border_box.width() - border_widths.left - border_widths.right).max(0.0),
            (positioned_border_box.height() - border_widths.top - border_widths.bottom).max(0.0),
        );
        let static_scroll_offset = self.finish_static_scroll_snap_scope(
            static_scroll_snap_scope,
            scroll_padding_box,
            scroll_padding_box,
        );
        if static_scroll_offset.x != 0.0 || static_scroll_offset.y != 0.0 {
            let translation =
                crate::layout::scroll_snap::static_scroll_translation(static_scroll_offset, style);
            for (_, fragment) in &mut positioned_fragments {
                *fragment = fragment.clone().translated(translation);
            }
            for layer in &mut child_positioned_layers {
                *layer = layer.clone().translated(translation);
            }
        }
        for layer in &child_positioned_layers {
            if !positioned_fragments
                .iter()
                .any(|(page_index, _)| *page_index == layer.page_index)
            {
                positioned_fragments.push((
                    layer.page_index,
                    PaintFragment::from_primitives(Vec::new(), Vec::new()),
                ));
            }
        }
        let target_page_index = if style.position == Position::Absolute {
            positioned_fragments
                .iter()
                .filter(|(_, fragment)| !fragment.is_empty())
                .map(|(page_index, _)| *page_index)
                .chain(child_positioned_layers.iter().map(|layer| layer.page_index))
                .chain(principal_decoration_target_page_index)
                .max()
        } else {
            None
        };
        let nested_positioned_page_span_target = self.pending_positioned_page_span_target;
        self.restore_positioned_pagination_state(pagination_state);
        self.pending_positioned_page_span_target = merged_positioned_page_span_target(
            self.pending_positioned_page_span_target,
            nested_positioned_page_span_target,
        );
        self.content_left = previous_left;
        self.content_right = previous_right;
        self.cursor_y = previous_cursor_y;
        self.inline_static_position = previous_inline_static_position;
        self.block_static_position_y_offset = previous_block_static_position_y_offset;
        self.absolute_static_position = previous_absolute_static_position;
        self.ensure_positioned_page_span(target_page_index);

        // A positioned box owns its principal decoration even when its
        // contained formatting context contributes no fragment. This occurs
        // for example when size containment suppresses the only in-flow
        // descendant. Do not let that empty content result suppress a visible
        // background, border, or outline.
        // <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        if positioned_fragments.is_empty()
            && child_positioned_layers.is_empty()
            && style.visibility == Visibility::Visible
            && positioned_border_box.width() > 0.0
            && positioned_border_box.height() > 0.0
        {
            let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
            fragment.prepend_primitives_in_band(
                PaintBand::BackgroundBorder,
                self.box_background_primitives(
                    paint_space_rect(
                        positioned_border_box.x(),
                        positioned_border_box.y(),
                        positioned_border_box.width(),
                        positioned_border_box.height(),
                    ),
                    style,
                ),
            );
            fragment.append_primitives_in_band(
                PaintBand::Outline,
                self.box_outline_primitives(
                    paint_space_rect(
                        positioned_border_box.x(),
                        positioned_border_box.y(),
                        positioned_border_box.width(),
                        positioned_border_box.height(),
                    ),
                    style,
                ),
            );
            if !fragment.is_empty() {
                positioned_fragments.push((positioned_origin_page_index, fragment));
            }
        }

        for (_, positioned_fragment) in &mut positioned_fragments {
            if let (Some(static_position), Some(fragment_baseline_y)) = (
                inline_static_baseline_position,
                positioned_fragment.first_line_y(),
            ) {
                *positioned_fragment =
                    positioned_fragment
                        .clone()
                        .translated(PaintTranslation::new(
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
        for (page_index, mut positioned_fragment) in positioned_fragments {
            let child_layers = child_layers_by_page
                .iter()
                .filter(|layer| layer.page_index == page_index)
                .cloned()
                .collect::<Vec<_>>();
            let mut links = positioned_fragment.links.clone();
            let bounds = positioned_border_box;
            if style.contain.size {
                positioned_fragment =
                    positioned_fragment.with_monolithic_fragmentation_scope(bounds);
            }
            let mut policy = StackingContextPolicy::for_positioned(element, style, bounds);
            if uses_initial_page_containing_block && style.position != Position::Fixed {
                // Paged media clips document-content painting to the page area.
                // Fixed descendants instead replay over the final page
                // sequence, whose media boxes provide the page-local clip.
                // Retaining this initial-page clip would hide a fixed layer
                // in the extra area of a later differently sized named page.
                // Add the absolute-positioned clip only if this box reaches
                // outside the page. Applying an identical clip to a fully
                // contained opaque rectangle introduces a second antialiased
                // edge in PDF rasterizers, despite no overflow needing
                // containment.
                // <https://www.w3.org/TR/css-page-3/#page-model>
                let page_clip = PageTopRect::new(
                    containing_block.x(),
                    containing_block.top_y(),
                    containing_block.width(),
                    containing_block.height(),
                )
                .paint_clip();
                let box_overflows_page = bounds.x() < page_clip.x()
                    || bounds.y() < page_clip.y()
                    || bounds.x() + bounds.width() > page_clip.x() + page_clip.width()
                    || bounds.y() + bounds.height() > page_clip.y() + page_clip.height();
                if box_overflows_page {
                    policy.effects.overflow_clip = Some(page_clip);
                }
            }
            if positioned_fragment.contains_overflow_clip()
                && policy
                    .effects
                    .overflow_clip
                    .is_some_and(|clip| clip.width() <= 0.0 || clip.height() <= 0.0)
            {
                // The measured formatting context already owns this empty
                // used padding-box clip. Applying the reconstructed empty clip
                // around that nested context would also erase the principal
                // decoration, which lies outside the contents clip.
                policy.effects.overflow_clip = None;
            }
            let escaped_atom_translation = if self.escaped_atom_positioning_depth > 0 {
                let containing_block_is_atom_local =
                    self.escaped_atom_containing_block == Some(containing_block);
                EscapedAtomTranslation::from_positioned_static_axes(
                    containing_block,
                    containing_block_is_atom_local
                        || (horizontal_insets_are_auto && absolute_static_position.is_some()),
                    containing_block_is_atom_local
                        || (vertical_insets_are_auto
                            && absolute_static_position
                                .is_some_and(AbsoluteStaticPosition::has_vertical_position)),
                    !containing_block_is_atom_local,
                )
            } else {
                EscapedAtomTranslation::none()
            };
            let captures_positioned_descendants =
                policy.is_real_stacking_context && policy.captures_positioned_descendants;
            let mut child_contexts = if captures_positioned_descendants {
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
            if style.contain.paint
                && let Some(overflow_clip) = policy.effects.overflow_clip.take()
            {
                // Overflow and paint containment clip the positioned box's
                // contents and captured descendants, not its own background,
                // border, or outline. Keep that clip inside the positioned
                // stacking context instead of applying it to the context as a
                // whole.
                // <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
                // <https://www.w3.org/TR/css-contain-1/#containment-paint>
                positioned_fragment = positioned_fragment.with_contents_clipped_to_rect(
                    overflow_clip,
                    std::mem::take(&mut child_contexts),
                );
            }
            if !positioned_fragment.is_empty() || !child_contexts.is_empty() {
                let context = PaintStackingContext::from_banded_fragment_with_stack_level(
                    policy.stack_level,
                    positioned_fragment,
                    child_contexts,
                )
                .with_source_order(positioned_source_order)
                .with_effects(policy.effects)
                .with_bounds(bounds);
                if style.position == Position::Fixed && locally_contained_fixed_block.is_none() {
                    self.fixed_layers.push(FixedPaintLayer {
                        source_element: element.id,
                        stack_level: policy.stack_level,
                        context,
                        links,
                    });
                    continue;
                }
                self.positioned_layers.push(PositionedPaintLayer {
                    page_index,
                    source_style: style.clone(),
                    source_is_target: element.is_target,
                    stack_level: policy.stack_level,
                    context,
                    links,
                    escaped_atom_translation,
                });
            }
            if !captures_positioned_descendants {
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

    /// Complete the principal decoration of a positioned box through every
    /// fragmentainer crossed by its used border-box block size.
    ///
    /// Positioned descendants are out of normal flow, so their containing
    /// normal-flow box can stop generating pages before the positioned
    /// principal box ends. CSS Fragmentation nevertheless gives the positioned
    /// box a fragment on each crossed fragmentainer. Descendant layers already
    /// carry their own page assignments; this phase adds only the principal
    /// background, border, and outline that cannot be inferred from descendant
    /// ink:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning>,
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>, and
    /// <https://www.w3.org/TR/css-break-3/#break-decoration>.
    #[allow(clippy::too_many_arguments)]
    fn extend_positioned_principal_decoration_fragments(
        &mut self,
        fragments: &mut Vec<(usize, PaintFragment)>,
        style: &ComputedStyle,
        border_box: PaintClip,
        first_page_index: usize,
        captured_last_page_index: usize,
        target_page_index: usize,
        first_page_context: PageContext,
    ) {
        if target_page_index <= captured_last_page_index || style.visibility != Visibility::Visible
        {
            return;
        }
        let fragmentainer_height = first_page_context.area_height().max(1.0);
        let box_top = border_box.y() + border_box.height();
        let box_start_distance = (first_page_context.top() - box_top).max(0.0);
        let box_end_distance = box_start_distance + border_box.height();

        for page_index in captured_last_page_index + 1..=target_page_index {
            let page_distance =
                page_index.saturating_sub(first_page_index) as f32 * fragmentainer_height;
            let slice_start = box_start_distance.max(page_distance);
            let slice_end = box_end_distance.min(page_distance + fragmentainer_height);
            if slice_end <= slice_start + 0.01 {
                continue;
            }
            let slice_top = first_page_context.top() - (slice_start - page_distance);
            let slice_height = slice_end - slice_start;
            let owns_block_start = slice_start <= box_start_distance + 0.01;
            let owns_block_end = slice_end >= box_end_distance - 0.01;
            let mut fragment_style = style.clone();
            suppress_fragmented_box_edges(&mut fragment_style, owns_block_start, owns_block_end);
            let background = self.box_background_primitives(
                paint_space_rect(
                    border_box.x(),
                    slice_top - slice_height,
                    border_box.width(),
                    slice_height,
                ),
                &fragment_style,
            );
            let outline = self.box_outline_primitives(
                paint_space_rect(
                    border_box.x(),
                    slice_top - slice_height,
                    border_box.width(),
                    slice_height,
                ),
                &fragment_style,
            );
            if background.is_empty() && outline.is_empty() {
                continue;
            }
            if let Some((_, fragment)) = fragments
                .iter_mut()
                .find(|(fragment_page_index, _)| *fragment_page_index == page_index)
            {
                fragment.prepend_primitives_in_band(PaintBand::BackgroundBorder, background);
                fragment.append_primitives_in_band(PaintBand::Outline, outline);
            } else {
                let mut fragment = PaintFragment::from_primitives(Vec::new(), Vec::new());
                fragment.prepend_primitives_in_band(PaintBand::BackgroundBorder, background);
                fragment.append_primitives_in_band(PaintBand::Outline, outline);
                fragments.push((page_index, fragment));
            }
        }
        fragments.sort_by_key(|(page_index, _)| *page_index);
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

    /// Returns the last page index whose principal decoration an absolute box may paint.
    ///
    /// The margin-box span determines which page fragments an absolute box
    /// occupies. Its decoration may be transparent, but the fragment span
    /// still contributes to the generated page sequence so fixed-position
    /// descendants and page-associated state replay on those pages.
    ///
    /// CSS Positioned Layout makes absolutely positioned boxes out-of-flow;
    /// CSS Fragmentation permits their rendered fragments to cross
    /// fragmentainer boundaries:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn absolute_positioned_decoration_span_target(
        &self,
        style: &ComputedStyle,
        containing_block: ContainingBlock,
        positioned_y: PositionedAxis,
        vertical_border_width: f32,
        containing_block_origin_page_index: usize,
    ) -> Option<usize> {
        if style.position != Position::Absolute {
            return None;
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
            return None;
        }
        // Size containment makes the principal box monolithic, but it does
        // not confine an oversized box's graphical representation to its
        // start fragmentainer. Its continuous margin-box extent therefore
        // still bounds every potential decoration slice.
        // <https://www.w3.org/TR/css-contain-1/#containment-size>
        // <https://www.w3.org/TR/css-break-3/#monolithic>
        let margin_box_bottom = margin_box_top - margin_box_height.max(0.0);
        let distance_from_page_top = (self.page_top() - margin_box_bottom).max(0.0);
        if distance_from_page_top <= 0.0 {
            return None;
        }
        Some(
            containing_block_origin_page_index
                + ((distance_from_page_top - 0.01).max(0.0) / page_height).floor() as usize,
        )
    }

    pub(in crate::layout) fn absolute_positioned_page_start_offset(
        &self,
        containing_block: ContainingBlock,
        positioned_y: PositionedAxis,
    ) -> (usize, f32) {
        let page_height = self.page_area_height().max(1.0);
        let margin_box_top = containing_block.top_y() - positioned_y.start;
        let start_distance = (self.page_top() - margin_box_top).max(0.0);
        let page_offset = (start_distance / page_height).floor() as usize;
        (
            page_offset,
            (start_distance - page_offset as f32 * page_height).max(0.0),
        )
    }

    /// Records final document pages required by positioned paint or descendant layers.
    ///
    /// The positioned subtree is first laid out against scratch page state so
    /// descendant fragmentation can be harvested without advancing normal flow.
    /// Only non-empty paint fragments and positioned descendant layers extend
    /// the real page sequence; an empty absolute margin-box span does not:
    /// <https://www.w3.org/TR/css-position-3/#absolute-positioning> and
    /// <https://www.w3.org/TR/css-break-3/#fragmentation-model>.
    pub(in crate::layout) fn ensure_positioned_page_span(
        &mut self,
        target_page_index: Option<usize>,
    ) {
        let Some(target_page_index) = target_page_index else {
            return;
        };
        self.pending_positioned_page_span_target = Some(
            self.pending_positioned_page_span_target
                .map_or(target_page_index, |existing| {
                    existing.max(target_page_index)
                }),
        );
    }

    pub(in crate::layout) fn materialize_pending_positioned_page_span(&mut self) {
        let target_page_index = self
            .pending_positioned_page_span_target
            .take()
            .into_iter()
            .chain(self.positioned_layers.iter().map(|layer| layer.page_index))
            .max();
        let Some(target_page_index) = target_page_index else {
            return;
        };
        while self.pages.len() < target_page_index {
            if !self.current_page_has_content() {
                self.mark_current_page_flow_content();
            }
            self.push_page_without_flushing_positioned_layers();
        }
        if self.pages.len() == target_page_index {
            self.mark_current_page_flow_content();
        }
    }

    pub(in crate::layout) fn push_page_without_flushing_positioned_layers(&mut self) {
        if !self.current_page_has_content() {
            self.mark_current_page_flow_content();
        }
        let offsets = self.current_fragment_offsets_for_page_break();
        // Positioned overflow must advance through the active fragmentainer
        // sequence without flushing layers that still belong to its containing
        // stacking context. In a multicol probe the next fragment is another
        // anonymous column box, not a document page.
        // <https://www.w3.org/TR/css-break-3/#fragmentation-model> and
        // <https://www.w3.org/TR/css-multicol-1/#pagination-and-overflow-outside-multicol>.
        let next_context = self
            .fragmentainer_override
            .map(|override_| override_.context_for_fragmentainer(self.pages.len() + 1))
            .unwrap_or_else(|| self.resolved_page_context(self.pages.len() + 2, false));
        let next_page = page_for_context(next_context);
        let page = std::mem::replace(&mut self.current_page, next_page);
        self.current_page_has_flow_content = false;
        self.current_page_has_named_page_flow_content = false;
        self.pages.push(page);
        self.page_names.push(self.current_page_name.clone());
        self.page_blanks.push(false);
        self.page_named_strings
            .push(std::mem::take(&mut self.current_page_named_strings));
        self.page_running_elements
            .push(std::mem::take(&mut self.current_page_running_elements));
        self.apply_page_context(next_context, offsets);
        self.truncate_page_start_margins = true;
        self.apply_pending_fragments_for_current_page();
    }

    pub(in crate::layout) fn remap_absolute_positioned_fragments(
        &self,
        fragments: &mut [(usize, PaintFragment)],
        scratch_start_page_index: usize,
        destination_start_page_index: usize,
    ) {
        // Positioned layout has already entered destination-page-local
        // coordinates before painting. Remapping therefore changes ownership
        // only, retaining each fragment's local geometry.
        // <https://drafts.csswg.org/css-position-3/#fragmenting-absolutely-positioned-elements>
        for (page_index, _) in fragments {
            let relative_page_index = page_index.saturating_sub(scratch_start_page_index);
            *page_index = destination_start_page_index + relative_page_index;
        }
    }

    pub(in crate::layout) fn positioned_pagination_state(&self) -> PositionedPaginationState {
        PositionedPaginationState {
            pages: self.pages.clone(),
            page_names: self.page_names.clone(),
            page_blanks: self.page_blanks.clone(),
            page_named_strings: self.page_named_strings.clone(),
            page_running_elements: self.page_running_elements.clone(),
            current_page: self.current_page.clone(),
            current_page_has_flow_content: self.current_page_has_flow_content,
            current_page_has_named_page_flow_content: self.current_page_has_named_page_flow_content,
            current_page_context: self.current_page_context,
            current_page_named_strings: self.current_page_named_strings.clone(),
            current_page_running_elements: self.current_page_running_elements.clone(),
            cursor_y: self.cursor_y,
            content_left: self.content_left,
            content_right: self.content_right,
            fragment_top_offsets: self.fragment_top_offsets.clone(),
            truncate_page_start_margins: self.truncate_page_start_margins,
            pending_paint_fragments: self.pending_paint_fragments.clone(),
            pending_page_side_effects: self.pending_page_side_effects.clone(),
            pending_positioned_page_span_target: self.pending_positioned_page_span_target,
        }
    }

    pub(in crate::layout) fn restore_positioned_pagination_state(
        &mut self,
        state: PositionedPaginationState,
    ) {
        self.pages = state.pages;
        self.page_names = state.page_names;
        self.page_blanks = state.page_blanks;
        self.page_named_strings = state.page_named_strings;
        self.page_running_elements = state.page_running_elements;
        self.current_page = state.current_page;
        self.current_page_has_flow_content = state.current_page_has_flow_content;
        self.current_page_has_named_page_flow_content =
            state.current_page_has_named_page_flow_content;
        self.current_page_context = state.current_page_context;
        self.current_page_named_strings = state.current_page_named_strings;
        self.current_page_running_elements = state.current_page_running_elements;
        self.cursor_y = state.cursor_y;
        self.content_left = state.content_left;
        self.content_right = state.content_right;
        self.fragment_top_offsets = state.fragment_top_offsets;
        self.truncate_page_start_margins = state.truncate_page_start_margins;
        self.pending_paint_fragments = state.pending_paint_fragments;
        self.pending_page_side_effects = state.pending_page_side_effects;
        self.pending_positioned_page_span_target = state.pending_positioned_page_span_target;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectViewBoxSourceSpace {}

type NormalizedObjectSourcePoint = euclid::Point2D<f32, ObjectViewBoxSourceSpace>;
type NormalizedObjectSourceSize = euclid::Size2D<f32, ObjectViewBoxSourceSpace>;
type NormalizedObjectSourceRect = euclid::Rect<f32, ObjectViewBoxSourceSpace>;

fn resolved_object_view_box(
    view_box: css::ObjectViewBox,
    natural_size: LayoutSize,
) -> Option<NormalizedObjectSourceRect> {
    if natural_size.width <= 0.0 || natural_size.height <= 0.0 {
        return None;
    }
    let resolve_x = |value| {
        used_length_percentage(
            value,
            PercentageBasis::definite(layout_pt(natural_size.width)),
        )
        .points()
            / natural_size.width
    };
    let resolve_y = |value| {
        used_length_percentage(
            value,
            PercentageBasis::definite(layout_pt(natural_size.height)),
        )
        .points()
            / natural_size.height
    };
    let rect = match view_box {
        css::ObjectViewBox::None => NormalizedObjectSourceRect::new(
            NormalizedObjectSourcePoint::new(0.0, 0.0),
            NormalizedObjectSourceSize::new(1.0, 1.0),
        ),
        css::ObjectViewBox::Inset {
            top,
            right,
            bottom,
            left,
            ..
        } => {
            let left = resolve_x(left);
            let right = resolve_x(right);
            let top = resolve_y(top);
            let bottom = resolve_y(bottom);
            NormalizedObjectSourceRect::new(
                NormalizedObjectSourcePoint::new(left, top),
                NormalizedObjectSourceSize::new(1.0 - left - right, 1.0 - top - bottom),
            )
        }
        css::ObjectViewBox::Xywh {
            x,
            y,
            width,
            height,
            ..
        } => NormalizedObjectSourceRect::new(
            NormalizedObjectSourcePoint::new(resolve_x(x), resolve_y(y)),
            NormalizedObjectSourceSize::new(resolve_x(width), resolve_y(height)),
        ),
        css::ObjectViewBox::Rect {
            top,
            right,
            bottom,
            left,
        } => {
            let left = resolve_x(left);
            let right = resolve_x(right);
            let top = resolve_y(top);
            let bottom = resolve_y(bottom);
            NormalizedObjectSourceRect::new(
                NormalizedObjectSourcePoint::new(left, top),
                NormalizedObjectSourceSize::new(right - left, bottom - top),
            )
        }
    };
    (rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.width.is_finite()
        && rect.size.height.is_finite()
        && rect.size.width > 0.0
        && rect.size.height > 0.0)
        .then_some(rect)
}

fn rectangular_object_view_box_clip(rect: PaintRect) -> RenderedPathClip {
    RenderedPathClip::new(
        vec![
            RenderedPathCommand::move_to(rect.origin),
            RenderedPathCommand::line_to(PaintPoint::new(rect.max_x(), rect.min_y())),
            RenderedPathCommand::line_to(PaintPoint::new(rect.max_x(), rect.max_y())),
            RenderedPathCommand::line_to(PaintPoint::new(rect.min_x(), rect.max_y())),
            RenderedPathCommand::Close,
        ],
        RenderedPathFillRule::NonZero,
        Vec::new(),
    )
}

fn object_view_box_clip(
    view_box: css::ObjectViewBox,
    natural_size: LayoutSize,
    source: NormalizedObjectSourceRect,
    geometry: ConcreteObjectGeometry,
) -> RenderedPathClip {
    let radii = match view_box {
        css::ObjectViewBox::Inset { radii, .. } | css::ObjectViewBox::Xywh { radii, .. } => radii,
        css::ObjectViewBox::None | css::ObjectViewBox::Rect { .. } => None,
    };
    let Some(radii) = radii.filter(|radii| !radii.clone().is_zero()) else {
        return rectangular_object_view_box_clip(geometry.visible);
    };
    let source_width = natural_size.width * source.size.width;
    let source_height = natural_size.height * source.size.height;
    let source_radii = used_rounded_rect_radii(radii, LayoutSize::new(source_width, source_height));
    let scale_x = geometry.concrete.size.width / source_width;
    let scale_y = geometry.concrete.size.height / source_height;
    let scale_corner = |corner: RenderedCornerRadius| {
        RenderedCornerRadius::new(corner.x() * scale_x, corner.y() * scale_y)
    };
    let destination_radii = RenderedRoundedRectRadii {
        top_left: scale_corner(source_radii.top_left),
        top_right: scale_corner(source_radii.top_right),
        bottom_right: scale_corner(source_radii.bottom_right),
        bottom_left: scale_corner(source_radii.bottom_left),
    };
    let mut clip = RenderedPathClip::new(
        shaped_rect_path_commands(
            geometry.concrete,
            destination_radii,
            css::CornerShapes::ROUND,
        ),
        RenderedPathFillRule::NonZero,
        Vec::new(),
    );
    let rectangular = rectangular_object_view_box_clip(geometry.visible);
    clip.additional_clips.push(RenderedPathClipPath::new(
        rectangular.commands,
        rectangular.fill_rule,
    ));
    clip
}

/// Resolve the concrete object size and position for a raster replaced element.
///
/// The concrete object is positioned in the element's content box and, when it
/// overflows, cropped through the shared raster paint-area operation. This
/// keeps `object-fit` and `background-size` on one source-to-destination
/// mapping model, including the PDF image resource's pixel coordinate system.
/// <https://www.w3.org/TR/css-images-3/#the-object-fit>
fn apply_object_fit(
    image: &mut RenderedImage,
    object_fit: ObjectFit,
    object_position: css::BackgroundPosition,
    object_view_box: css::ObjectViewBox,
) -> bool {
    if image.width() <= 0.0 || image.height() <= 0.0 {
        return false;
    }
    let source_width = image.pixel_width();
    let source_height = image.pixel_height();
    if source_width == 0 || source_height == 0 {
        return false;
    }
    let natural_size = LayoutSize::new(
        source_width as f32 * css::CSS_PX_TO_PT,
        source_height as f32 * css::CSS_PX_TO_PT,
    );
    let Some(source) = resolved_object_view_box(object_view_box.clone(), natural_size) else {
        return false;
    };
    let Some(geometry) = concrete_object_geometry(
        image.paint_rect(),
        natural_size.width * source.size.width,
        natural_size.height * source.size.height,
        object_fit,
        object_position,
    ) else {
        return false;
    };
    let full_width = geometry.concrete.size.width / source.size.width;
    let full_height = geometry.concrete.size.height / source.size.height;
    image.set_paint_rect(paint_space_rect(
        geometry.concrete.origin.x - source.origin.x * full_width,
        geometry.concrete.origin.y - (1.0 - source.max_y()) * full_height,
        full_width,
        full_height,
    ));
    *image = image.clone().with_clip(object_view_box_clip(
        object_view_box,
        natural_size,
        source,
        geometry,
    ));
    true
}

/// The concrete object and visible intersection for a replaced image.
///
/// CSS Images defines `object-fit` as concrete-object sizing followed by
/// `object-position` alignment in the element's content box. Keeping this
/// source-independent geometry lets raster and vector image emitters apply
/// the same sizing and clipping semantics.
/// <https://www.w3.org/TR/css-images-3/#the-object-fit>
#[derive(Clone, Copy)]
struct ConcreteObjectGeometry {
    concrete: crate::document::PaintRect,
    visible: crate::document::PaintRect,
}

fn concrete_object_geometry(
    destination: crate::document::PaintRect,
    natural_width: f32,
    natural_height: f32,
    object_fit: ObjectFit,
    object_position: css::BackgroundPosition,
) -> Option<ConcreteObjectGeometry> {
    if natural_width <= 0.0
        || natural_height <= 0.0
        || destination.size.width <= 0.0
        || destination.size.height <= 0.0
    {
        return None;
    }
    let contain_scale =
        (destination.size.width / natural_width).min(destination.size.height / natural_height);
    let cover_scale =
        (destination.size.width / natural_width).max(destination.size.height / natural_height);
    let (concrete_width, concrete_height) = match object_fit {
        ObjectFit::Fill => (destination.size.width, destination.size.height),
        ObjectFit::Contain => (
            natural_width * contain_scale,
            natural_height * contain_scale,
        ),
        ObjectFit::Cover => (natural_width * cover_scale, natural_height * cover_scale),
        ObjectFit::None => (natural_width, natural_height),
        ObjectFit::ScaleDown => {
            let scale = contain_scale.min(1.0);
            (natural_width * scale, natural_height * scale)
        }
    };
    let offset_x = used_background_position_axis(
        object_position.x,
        destination.size.width - concrete_width,
        false,
    );
    let offset_y = used_background_position_axis(
        object_position.y,
        destination.size.height - concrete_height,
        true,
    );
    let concrete = paint_space_rect(
        destination.origin.x + offset_x,
        destination.origin.y + offset_y,
        concrete_width,
        concrete_height,
    );
    let visible = concrete.intersection(&destination)?;
    Some(ConcreteObjectGeometry { concrete, visible })
}

/// Translate concrete-object geometry into an SVG viewport source rectangle.
///
/// SVG source coordinates start at the top, while paint rectangles start at
/// the bottom. The source Y conversion therefore inverts the visible
/// intersection within the concrete object.
fn svg_replaced_group(
    asset: &SharedSvgAsset,
    destination: PaintRect,
    object_fit: ObjectFit,
    object_position: css::BackgroundPosition,
    object_view_box: css::ObjectViewBox,
) -> crate::svg::SvgPaintGroup {
    let natural_size = asset.replaced_intrinsic_size();
    let Some(view_box) = resolved_object_view_box(object_view_box.clone(), natural_size) else {
        return crate::svg::SvgPaintGroup::empty();
    };
    let Some(geometry) = concrete_object_geometry(
        destination,
        natural_size.width * view_box.size.width,
        natural_size.height * view_box.size.height,
        object_fit,
        object_position,
    ) else {
        return crate::svg::SvgPaintGroup::empty();
    };
    // The SVG root still has its complete source viewport.  The view box only
    // changes CSS Images' effective natural size, so scale that full viewport
    // to the concrete object before selecting the requested source rectangle.
    let viewport_asset = asset.with_replaced_viewport(
        geometry.concrete.size.width / view_box.size.width,
        geometry.concrete.size.height / view_box.size.height,
    );
    let source_size = viewport_asset.source_viewport_size();
    let left =
        (geometry.visible.min_x() - geometry.concrete.min_x()) / geometry.concrete.size.width;
    let bottom =
        (geometry.visible.min_y() - geometry.concrete.min_y()) / geometry.concrete.size.height;
    let visible_width = geometry.visible.size.width / geometry.concrete.size.width;
    let visible_height = geometry.visible.size.height / geometry.concrete.size.height;
    let source = SvgSourceRect::new(
        SvgSourcePoint::new(
            source_size.width * (view_box.origin.x + view_box.size.width * left),
            source_size.height
                * (view_box.origin.y + view_box.size.height * (1.0 - bottom - visible_height)),
        ),
        SvgSourceSize::new(
            source_size.width * view_box.size.width * visible_width,
            source_size.height * view_box.size.height * visible_height,
        ),
    );
    let group = viewport_asset.paint_group_for_source_rect(geometry.visible, source);
    group.with_clip(object_view_box_clip(
        object_view_box,
        natural_size,
        view_box,
        geometry,
    ))
}

/// Emit tiled vector paths for one CSS border-image slice.
///
/// This shares the same segment resolution as raster `border-image`, but maps
/// each selected SVG root-viewport rectangle directly to the tile's
/// destination rectangle.
fn push_svg_border_image_tiles(
    primitives: &mut Vec<PaintPrimitive>,
    asset: &SharedSvgAsset,
    destination: RenderedImageTileRect,
    source: RenderedImageSourceRect,
    repeat_x: css::BorderImageRepeatKeyword,
    repeat_y: css::BorderImageRepeatKeyword,
) {
    let tile_size = border_image_base_tile_size(destination, source, repeat_x, repeat_y);
    let x_segments =
        border_image_tile_segments(repeat_x, destination.width(), tile_size.width, source.width);
    let y_segments = border_image_tile_segments(
        repeat_y,
        destination.height(),
        tile_size.height,
        source.height,
    );
    for y_segment in &y_segments {
        for x_segment in &x_segments {
            if x_segment.destination_size <= 0.0
                || y_segment.destination_size <= 0.0
                || x_segment.source_size == 0
                || y_segment.source_size == 0
            {
                continue;
            }
            let source = SvgSourceRect::new(
                SvgSourcePoint::new(
                    (source.x + x_segment.source_offset) as f32,
                    (source.y + y_segment.source_offset) as f32,
                ),
                SvgSourceSize::new(x_segment.source_size as f32, y_segment.source_size as f32),
            );
            primitives.extend(
                asset
                    .paint_paths_for_source_rect(
                        paint_space_rect(
                            destination.x() + x_segment.destination_offset,
                            destination.y() + y_segment.destination_offset,
                            x_segment.destination_size,
                            y_segment.destination_size,
                        ),
                        source,
                    )
                    .into_iter()
                    .map(PaintPrimitive::Path),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::ComputedLengthPercentage;
    use std::rc::Rc;

    fn first_svg_path(group: &crate::svg::SvgPaintGroup) -> Option<&RenderedPath> {
        group.items.iter().find_map(|item| match item {
            crate::svg::SvgPaintItem::Path(path) => Some(path.as_ref()),
            crate::svg::SvgPaintItem::Group(group) => first_svg_path(group),
        })
    }

    #[test]
    fn cover_geometry_preserves_the_typed_destination_paint_rect() {
        let destination: PaintRect = paint_space_rect(10.0, 20.0, 100.0, 100.0);
        let geometry = concrete_object_geometry(
            destination,
            200.0,
            100.0,
            ObjectFit::Cover,
            css::BackgroundPosition::INITIAL,
        )
        .expect("positive replaced geometry should be paintable");

        assert_eq!(
            geometry.concrete,
            paint_space_rect(10.0, 20.0, 200.0, 100.0)
        );
        assert_eq!(geometry.visible, destination);
    }

    #[test]
    fn object_view_box_uses_its_own_top_left_source_space() {
        let natural = LayoutSize::new(200.0, 100.0);
        let view_box = css::ObjectViewBox::Xywh {
            x: ComputedLengthPercentage::from_points(20.0),
            y: ComputedLengthPercentage::from_points(10.0),
            width: ComputedLengthPercentage::from_points(100.0),
            height: ComputedLengthPercentage::from_points(50.0),
            radii: None,
        };

        let resolved = resolved_object_view_box(view_box, natural)
            .expect("positive object-view-box source geometry");

        assert_eq!(resolved.origin.x, 0.1);
        assert_eq!(resolved.origin.y, 0.1);
        assert_eq!(resolved.size.width, 0.5);
        assert_eq!(resolved.size.height, 0.5);
    }

    #[test]
    fn object_view_box_rejects_empty_source_geometry() {
        let view_box = css::ObjectViewBox::Inset {
            top: ComputedLengthPercentage::from_points(50.0),
            right: ComputedLengthPercentage::ZERO,
            bottom: ComputedLengthPercentage::from_points(50.0),
            left: ComputedLengthPercentage::ZERO,
            radii: None,
        };

        assert!(resolved_object_view_box(view_box, LayoutSize::new(100.0, 100.0)).is_none());
    }

    #[test]
    fn raster_object_view_box_maps_the_full_source_then_clips_the_crop() {
        let destination = paint_space_rect(0.0, 0.0, 100.0, 100.0);
        let mut image = RenderedImage::from_paint_rect(
            destination,
            false,
            200,
            100,
            None,
            true,
            vec![0; 200 * 100 * 3].into(),
            None,
            None,
        );
        // CSS dimensions are points, while this raster's 200×100 source is
        // 150×75pt at 96dpi. Select its central quarter in source space.
        let view_box = css::ObjectViewBox::Xywh {
            x: ComputedLengthPercentage::from_points(37.5),
            y: ComputedLengthPercentage::from_points(18.75),
            width: ComputedLengthPercentage::from_points(75.0),
            height: ComputedLengthPercentage::from_points(37.5),
            radii: None,
        };

        assert!(apply_object_fit(
            &mut image,
            ObjectFit::Fill,
            css::BackgroundPosition::INITIAL,
            view_box,
        ));
        assert_eq!(
            image.paint_rect(),
            paint_space_rect(-50.0, -50.0, 200.0, 200.0)
        );
        let clip = image
            .clip()
            .expect("object-view-box installs a destination clip");
        assert_eq!(
            clip.commands,
            rectangular_object_view_box_clip(destination).commands
        );
    }

    #[test]
    fn rounded_object_view_box_uses_a_destination_corner_clip() {
        let view_box = crate::css::parse_object_view_box(
            "xywh(10pt 10pt 80pt 80pt round 12pt)",
            crate::css::ROOT_FONT_SIZE_PT,
        )
        .expect("valid rounded basic shape");
        let natural = LayoutSize::new(100.0, 100.0);
        let source = resolved_object_view_box(view_box.clone(), natural).unwrap();
        let geometry = concrete_object_geometry(
            paint_space_rect(10.0, 20.0, 80.0, 80.0),
            80.0,
            80.0,
            ObjectFit::Fill,
            css::BackgroundPosition::INITIAL,
        )
        .unwrap();

        let clip = object_view_box_clip(view_box, natural, source, geometry);

        assert!(
            clip.commands.len() > 5,
            "rounded clip contains curve commands"
        );
        assert_eq!(clip.additional_clips.len(), 1);
    }

    #[test]
    fn svg_object_view_box_projects_source_crop_before_destination_paint() {
        let asset = Rc::new(
            crate::svg::parse_svg_bytes(
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100" preserveAspectRatio="none"><rect width="100" height="100" fill="red"/></svg>"#,
            )
            .expect("simple SVG source"),
        );
        let view_box = css::ObjectViewBox::Xywh {
            x: ComputedLengthPercentage::ZERO,
            y: ComputedLengthPercentage::ZERO,
            width: ComputedLengthPercentage::from_percent(0.5),
            height: ComputedLengthPercentage::from_percent(1.0),
            radii: None,
        };

        let group = svg_replaced_group(
            &asset,
            paint_space_rect(10.0, 20.0, 100.0, 100.0),
            ObjectFit::Fill,
            css::BackgroundPosition::INITIAL,
            view_box,
        );

        let path = first_svg_path(&group).expect("cropped SVG produces a vector path");
        assert!(
            path.clip.is_some(),
            "source crop projects a destination clip"
        );
        let bounds = path.paint_bounds().expect("SVG path has paint bounds");
        // The full SVG source is intentionally painted at twice the target
        // width; the destination clip selects its left-half view box without
        // losing fractional source geometry.
        assert!((bounds.origin.x - 10.0).abs() < 0.01);
        assert!((bounds.origin.y - 20.0).abs() < 0.01);
        assert!((bounds.size.width - 200.0).abs() < 0.01);
        assert!((bounds.size.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn nested_positioned_page_span_keeps_the_furthest_requirement() {
        assert_eq!(
            merged_positioned_page_span_target(Some(2), Some(4)),
            Some(4)
        );
        assert_eq!(
            merged_positioned_page_span_target(Some(4), Some(2)),
            Some(4)
        );
        assert_eq!(merged_positioned_page_span_target(None, Some(3)), Some(3));
        assert_eq!(merged_positioned_page_span_target(Some(3), None), Some(3));
    }
}
