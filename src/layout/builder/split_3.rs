use super::*;

pub(in crate::layout) fn page_for_context(context: PageContext) -> Page {
    let mut page = Page::new(context.size.width(), context.size.height());
    page.rotation = context.rotation;
    page
}

pub(in crate::layout) fn canvas_background_style(style: &ComputedStyle) -> ComputedStyle {
    let mut style = style.clone();
    style.border_widths = css::Edges::ZERO;
    style.border_width_values = css::CssEdges::all(css::ComputedLengthPercentage::ZERO);
    style.border_styles = css::BorderStyles::NONE;
    style.border_width = 0.0;
    style.border_image = css::BorderImage::initial();
    style
}

pub(in crate::layout) fn page_style_has_visible_paint(style: &ComputedStyle) -> bool {
    style.background_color.is_some_and(Color::is_visible)
        || style.background_image.is_some()
        || used_border_width(style) > 0.0
        || style.border_image.source.is_some()
}

pub(in crate::layout) fn target_element_text(element: &Element) -> String {
    let mut output = String::new();
    for child in &element.children {
        collect_target_element_text(child, &mut output);
    }
    collapse_whitespace(&output)
}

pub(in crate::layout) fn collect_target_element_text(node: &Node, output: &mut String) {
    match &node.kind {
        NodeKind::Text(text) => {
            output.push_str(text);
            output.push(' ');
        }
        NodeKind::Element(element) => {
            for child in &element.children {
                collect_target_element_text(child, output);
            }
        }
    }
}

/// Resolves CSS page border and padding declarations to used physical edges.
///
/// CSS Paged Media applies border and padding to the page box, and CSS Box
/// Model resolves padding percentages against the containing block's inline
/// size before layout consumes used values:
/// <https://www.w3.org/TR/css-page-3/#page-model> and
/// <https://www.w3.org/TR/CSS22/box.html#padding-properties>.
pub(in crate::layout) fn page_box_edges_from_declarations_with_ch_advance(
    declarations: &Declarations,
    page_size: PageSize,
    ch_advance: f32,
) -> PageBoxEdges {
    if declarations.is_empty() {
        return PageBoxEdges::ZERO;
    }
    let mut style = ComputedStyle::initial();
    css::apply_declarations(&mut style, declarations);
    style.resolve_font_metric_lengths(ch_advance);
    PageBoxEdges {
        border: used_border_widths(&style),
        padding: css::page_padding_from_for_size_with_ch_advance(
            declarations,
            page_size,
            ch_advance,
        ),
    }
}

/// Used page background positioning area.
///
/// CSS Backgrounds and Borders defines `background-origin` as selecting the
/// border, padding, or content box used for background image positioning. CSS
/// Paged Media applies that box model to page boxes:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PageBackgroundArea {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
    pub(in crate::layout) width: f32,
    pub(in crate::layout) height: f32,
}

impl PageBackgroundArea {
    pub(in crate::layout) fn inset(self, edges: css::Edges) -> Self {
        Self {
            x: self.x + edges.left,
            y: self.y + edges.bottom,
            width: (self.width - edges.left - edges.right).max(0.0),
            height: (self.height - edges.top - edges.bottom).max(0.0),
        }
    }
}

/// Resolves the page background positioning area selected by `background-origin`.
///
/// For a page box, the border box starts inside the page margins, the padding
/// box is inset by page borders, and the content box is additionally inset by
/// page padding:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin> and
/// <https://www.w3.org/TR/css-page-3/#page-model>.
pub(in crate::layout) fn page_background_positioning_area(
    declarations: &Declarations,
    base_margins: PageMargins,
    page_size: PageSize,
    origin: css::BackgroundBox,
    ch_advance: f32,
) -> PageBackgroundArea {
    let edges =
        page_box_edges_from_declarations_with_ch_advance(declarations, page_size, ch_advance);
    let margins = css::page_margins_from_for_size_and_edges_with_ch_advance(
        declarations,
        base_margins,
        page_size,
        edges.total(),
        ch_advance,
    );
    let border_box = PageBackgroundArea {
        x: margins.left(),
        y: margins.bottom(),
        width: (page_size.width() - margins.left() - margins.right()).max(0.0),
        height: (page_size.height() - margins.top() - margins.bottom()).max(0.0),
    };

    match origin {
        css::BackgroundBox::Border => border_box,
        css::BackgroundBox::Padding => border_box.inset(edges.border),
        css::BackgroundBox::Content => border_box.inset(edges.border).inset(edges.padding),
    }
}

pub(in crate::layout) fn page_background_layers_for_paint(
    style: &ComputedStyle,
) -> Vec<css::BackgroundLayer> {
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

/// Clips page background image tiles to the selected background painting area.
///
/// CSS Backgrounds separates `background-origin`, which positions the image,
/// from `background-clip`, which clips painting:
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-origin> and
/// <https://www.w3.org/TR/css-backgrounds-3/#the-background-clip>. Page boxes
/// use the same border/padding/content boxes through CSS Paged Media:
/// <https://www.w3.org/TR/css-page-3/#page-model>.
pub(in crate::layout) fn clip_background_images_to_area(
    images: Vec<RenderedImage>,
    clip: PageBackgroundArea,
) -> Vec<RenderedImage> {
    images
        .into_iter()
        .filter_map(|image| clip_background_image_to_area(image, clip))
        .collect()
}

pub(in crate::layout) fn clip_background_image_to_area(
    mut image: RenderedImage,
    clip: PageBackgroundArea,
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

/// Returns whether a forced break target is satisfied by the next page number.
///
/// CSS Fragmentation defines `left`/`right` as spread sides and `recto`/`verso`
/// as first/opposite page sides in the current page progression:
/// <https://www.w3.org/TR/css-break-3/#valdef-break-before-recto> and
/// <https://www.w3.org/TR/css-page-3/#spread-pseudos>.
pub(in crate::layout) fn forced_break_satisfied(
    forced_break: PageBreak,
    next_page_number: usize,
    page_progression_direction: Direction,
) -> bool {
    let is_left = page_is_left(next_page_number, page_progression_direction);
    match forced_break {
        PageBreak::Auto | PageBreak::Avoid | PageBreak::Page => true,
        PageBreak::Left => is_left,
        PageBreak::Right => !is_left,
        PageBreak::Recto => is_recto_page(next_page_number, page_progression_direction),
        PageBreak::Verso => !is_recto_page(next_page_number, page_progression_direction),
    }
}

/// Returns whether a page is on the left side of the spread.
///
/// CSS Paged Media spread pseudo-classes follow the page progression direction:
/// <https://www.w3.org/TR/css-page-3/#spread-pseudos>.
pub(in crate::layout) fn page_is_left(
    page_number: usize,
    page_progression_direction: Direction,
) -> bool {
    match page_progression_direction {
        Direction::Ltr => page_number.is_multiple_of(2),
        Direction::Rtl => !page_number.is_multiple_of(2),
    }
}

pub(in crate::layout) fn anonymous_block_is_plain_text_with_style(
    children: &[box_tree::FormattingBox<'_>],
    style: &ComputedStyle,
) -> bool {
    children
        .iter()
        .all(|child| matches!(child, box_tree::FormattingBox::Text(box_) if *box_.style == *style))
}

/// Returns whether a page is the recto side for forced recto/verso breaks.
///
/// CSS Fragmentation maps `recto` to the first side of a spread in the current
/// page progression and `verso` to the opposite side:
/// <https://www.w3.org/TR/css-break-3/#valdef-break-before-recto>.
pub(in crate::layout) fn is_recto_page(
    page_number: usize,
    page_progression_direction: Direction,
) -> bool {
    match page_progression_direction {
        Direction::Ltr => !page_is_left(page_number, page_progression_direction),
        Direction::Rtl => page_is_left(page_number, page_progression_direction),
    }
}

pub(in crate::layout) fn append_fixed_layer_to_page(page: &mut Page, layer: &FixedPaintLayer) {
    let fragment = fixed_layer_fragment(layer);
    let recorded = page.record_paint_fragment(&fragment, PaintVector::new(0.0, 0.0));
    page.append_recorded_paint_fragment(recorded);
    page.sort_paint_tree_stacking_contexts();
}

pub(in crate::layout) fn positioned_layer_fragment(layer: &PositionedPaintLayer) -> PaintFragment {
    PaintFragment::from_stacking_context(layer.context.clone().with_links(layer.links.clone()))
}

pub(in crate::layout) fn fixed_layer_fragment(layer: &FixedPaintLayer) -> PaintFragment {
    PaintFragment::from_stacking_context(layer.context.clone().with_links(layer.links.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::{
        ComputedBoxValues, ComputedLengthPercentage, ComputedLengthPercentageOrAuto,
        ComputedLineHeight, CssEdges,
    };

    fn test_layout_builder<'a>(
        options: &'a RenderOptions,
        stylesheets: &'a [Stylesheet],
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
        LayoutBuilder::new(LayoutBuilderConfig {
            options,
            stylesheets,
            base_url: None,
            root_url: None,
            resource_cache,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            font_system: FontSystem::new(),
        })
    }

    #[test]
    fn resolves_font_metric_lengths_in_typographic_pseudo_styles() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let mut style = ComputedStyle {
            font_size: 20.0,
            ..ComputedStyle::initial()
        };
        style.first_line_style = Some(Box::new(ComputedStyle {
            line_height_value: ComputedLineHeight::Length(ComputedLengthPercentage::from_ch(2.0)),
            ..style.clone()
        }));
        style.first_letter_style = Some(Box::new(ComputedStyle {
            box_values: ComputedBoxValues {
                margin: CssEdges {
                    left: ComputedLengthPercentageOrAuto::LengthPercentage(
                        ComputedLengthPercentage::from_ch(3.0),
                    ),
                    ..ComputedBoxValues::initial().margin
                },
                ..ComputedBoxValues::initial()
            },
            ..style.clone()
        }));

        builder.resolve_style_font_metric_lengths(&mut style);

        let first_line = style.first_line_style.as_ref().unwrap();
        let ComputedLineHeight::Length(line_height) = first_line.line_height_value else {
            panic!("expected first-line length line-height");
        };
        assert_eq!(line_height.ch, 0.0);
        assert!(line_height.length_points() > 0.0);

        let first_letter = style.first_letter_style.as_ref().unwrap();
        let ComputedLengthPercentageOrAuto::LengthPercentage(margin_left) =
            first_letter.box_values.margin.left
        else {
            panic!("expected first-letter length margin");
        };
        assert_eq!(margin_left.ch, 0.0);
        assert!(margin_left.length_points() > 0.0);
    }
}
