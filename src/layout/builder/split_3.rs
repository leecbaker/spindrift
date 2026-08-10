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
    ch_advance: LayoutLength,
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
    ch_advance: LayoutLength,
) -> PaintRect {
    let edges =
        page_box_edges_from_declarations_with_ch_advance(declarations, page_size, ch_advance);
    let margins = css::page_margins_from_for_size_and_edges_with_ch_advance(
        declarations,
        base_margins,
        page_size,
        edges.total(),
        ch_advance,
    );
    let border_box = paint_space_rect(
        margins.left(),
        margins.bottom(),
        (page_size.width() - margins.left() - margins.right()).max(0.0),
        (page_size.height() - margins.top() - margins.bottom()).max(0.0),
    );

    match origin {
        css::BackgroundBox::Border | css::BackgroundBox::BorderArea => border_box,
        css::BackgroundBox::Padding => inset_paint_rect(border_box, edges.border),
        css::BackgroundBox::Content => {
            inset_paint_rect(inset_paint_rect(border_box, edges.border), edges.padding)
        }
    }
}

pub(in crate::layout) fn page_background_layers_for_paint(
    style: &ComputedStyle,
) -> Vec<css::BackgroundLayer> {
    if !style.background.background_layers.is_empty() {
        return style.background.background_layers.clone();
    }
    vec![css::BackgroundLayer {
        image: style.background.background_image.clone(),
        position: style.background.background_position.clone(),
        size: style.background.background_size.clone(),
        repeat: style.background.background_repeat,
        attachment: style.background.background_attachment,
        origin: style.background.background_origin,
        clip: style.background.background_clip,
    }]
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
        PageBreak::Auto
        | PageBreak::Avoid
        | PageBreak::AvoidPage
        | PageBreak::AvoidColumn
        | PageBreak::Page
        | PageBreak::Column => true,
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
    let recorded = page.record_paint_fragment_owned(fragment, PaintTranslation::identity());
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
    use crate::Css;
    use crate::css::{
        ComputedBoxValues, ComputedLengthPercentage, ComputedLengthPercentageOrAuto,
        ComputedLineHeight, CssEdges,
    };

    fn test_layout_builder<'a, Collection: crate::css::StylesheetCollection + ?Sized>(
        options: &'a RenderOptions,
        stylesheets: &'a Collection,
        resource_cache: &'a ResourceCache,
    ) -> LayoutBuilder<'a> {
        let stylesheets = crate::css::StylesheetCollection::stylesheet_view(stylesheets);
        LayoutBuilder::new(LayoutBuilderConfig {
            options,
            stylesheets,
            base_url: None,
            root_url: None,
            resource_cache,
            // The builder retains this reference for its lifetime; tests that do
            // not exercise iframes use one immutable empty fixture.
            iframe_documents: Box::leak(Box::new(HashMap::new())),
            iframe_viewport: None,
            page_progression_direction: Direction::Ltr,
            page_counter_initial_values: HashMap::new(),
            target_references: crate::layout::TargetReferenceSnapshot::default(),
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
        let ComputedLineHeight::Length(line_height) = &first_line.line_height_value else {
            panic!("expected first-line length line-height");
        };
        assert!(!line_height.requires_ch_advance());
        assert!(line_height.length_points() > 0.0);

        let first_letter = style.first_letter_style.as_ref().unwrap();
        let ComputedLengthPercentageOrAuto::LengthPercentage(margin_left) =
            &first_letter.box_values.margin.left
        else {
            panic!("expected first-letter length margin");
        };
        assert!(!margin_left.requires_ch_advance());
        assert!(margin_left.length_points() > 0.0);
    }

    #[test]
    fn page_background_positioning_uses_typed_paint_rects_for_each_box() {
        let declarations = Declarations::from_iter([
            ("border-top-width".to_string(), "7pt".to_string()),
            ("border-right-width".to_string(), "11pt".to_string()),
            ("border-bottom-width".to_string(), "13pt".to_string()),
            ("border-left-width".to_string(), "17pt".to_string()),
            ("border-top-style".to_string(), "solid".to_string()),
            ("border-right-style".to_string(), "solid".to_string()),
            ("border-bottom-style".to_string(), "solid".to_string()),
            ("border-left-style".to_string(), "solid".to_string()),
            ("padding-top".to_string(), "2pt".to_string()),
            ("padding-right".to_string(), "3pt".to_string()),
            ("padding-bottom".to_string(), "5pt".to_string()),
            ("padding-left".to_string(), "7pt".to_string()),
        ]);
        let margins = PageMargins::from_points(11.0, 13.0, 17.0, 19.0);
        let size = PageSize::from_points(200.0, 180.0);

        assert_eq!(
            page_background_positioning_area(
                &declarations,
                margins,
                size,
                css::BackgroundBox::Border,
                layout_pt(0.0),
            ),
            paint_space_rect(19.0, 17.0, 168.0, 152.0),
        );
        assert_eq!(
            page_background_positioning_area(
                &declarations,
                margins,
                size,
                css::BackgroundBox::Padding,
                layout_pt(0.0),
            ),
            paint_space_rect(36.0, 30.0, 140.0, 132.0),
        );
        assert_eq!(
            page_background_positioning_area(
                &declarations,
                margins,
                size,
                css::BackgroundBox::Content,
                layout_pt(0.0),
            ),
            paint_space_rect(43.0, 35.0, 130.0, 125.0),
        );
    }

    #[test]
    fn first_named_page_establishes_the_viewport_once() {
        let options = RenderOptions::default();
        let stylesheets = vec![css::parse_stylesheet(&Css::from_string(
            "@page { size: 300pt 400pt } @page chapter { size: 200pt 200pt }",
        ))];
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.current_page_name = Some("chapter".to_string());
        builder.rebuild_empty_current_page_context();
        let initial = builder.initial_viewport_context;

        builder.current_page_has_flow_content = true;
        builder.push_page();
        builder.current_page_name = None;
        builder.rebuild_empty_current_page_context();

        assert_eq!(builder.initial_viewport_context, initial);
        assert_eq!(initial.size, PageSize::from_points(200.0, 200.0));
        assert_eq!(
            builder.current_page_context.size,
            PageSize::from_points(300.0, 400.0)
        );
    }

    #[test]
    fn named_page_boundary_requires_in_flow_predecessor() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        builder.current_page_name = Some("float-page".to_string());

        // A preceding float has paint and pagination occupancy, but it is
        // out of normal flow and cannot form a class-A boundary for the next
        // page-name group.
        builder.current_page_has_flow_content = true;
        let scope = builder.enter_page_name_scope_for_value(Some("article"));
        assert_eq!(builder.pages.len(), 0);
        assert_eq!(builder.current_page_name.as_deref(), Some("article"));
        builder.exit_page_name_scope(
            scope.map(|previous_page_name| PageNameScope::Inline { previous_page_name }),
        );

        builder.mark_current_page_flow_content();
        builder.enter_page_name_scope_for_value(Some("appendix"));
        assert_eq!(builder.pages.len(), 1);
        assert_eq!(builder.current_page_name.as_deref(), Some("appendix"));
    }

    #[test]
    fn viewport_units_use_the_immutable_initial_context_after_a_named_transition() {
        let options = RenderOptions::default();
        let stylesheets = Vec::new();
        let resource_cache = ResourceCache::default();
        let mut builder = test_layout_builder(&options, &stylesheets, &resource_cache);
        let initial = PageContext {
            size: PageSize::from_points(200.0, 200.0),
            ..PageContext::from_options(&options)
        };
        let destination = PageContext {
            size: PageSize::from_points(300.0, 400.0),
            ..PageContext::from_options(&options)
        };
        builder.initial_viewport_context = initial;
        builder.current_page_context = destination;
        let style = ComputedStyle {
            box_values: ComputedBoxValues {
                width: ComputedLengthPercentageOrAuto::LengthPercentage(
                    ComputedLengthPercentage::from_vw(100.0),
                ),
                height: css::PhysicalHeight::from_computed(
                    ComputedLengthPercentageOrAuto::LengthPercentage(
                        ComputedLengthPercentage::from_vh(100.0),
                    ),
                ),
                ..ComputedBoxValues::initial()
            },
            ..ComputedStyle::initial()
        };

        let resolved = builder.style_with_current_viewport_lengths(&style);
        let ComputedLengthPercentageOrAuto::LengthPercentage(width) = &resolved.box_values.width
        else {
            panic!("expected viewport-resolved width");
        };
        let ComputedLengthPercentageOrAuto::LengthPercentage(height) =
            resolved.box_values.height.value()
        else {
            panic!("expected viewport-resolved height");
        };
        assert_eq!(width.length_points(), initial.area_width());
        assert_eq!(height.length_points(), initial.area_height());
    }
}
