use super::*;
use crate::dom::DocumentSyntax;

const XHTML_NAMESPACE_URL: &str = "http://www.w3.org/1999/xhtml";
const SVG_NAMESPACE_URL: &str = "http://www.w3.org/2000/svg";

/// Return whether an element has HTML rendering semantics.
///
/// XHTML documents retain XML parsing and selector semantics (including
/// namespace and case sensitivity), but elements in the XHTML namespace use
/// HTML's rendering definitions.  Conversely, an arbitrary XML `<img>` or
/// `<table>` is not promoted to an HTML replaced element or table merely by
/// its local name.
/// <https://html.spec.whatwg.org/multipage/xhtml.html>
pub(super) fn has_html_rendering_semantics(element: &Element) -> bool {
    element.namespace_url == XHTML_NAMESPACE_URL
        || (element.document_syntax == DocumentSyntax::Html && element.namespace_url.is_empty())
}

/// Used overflow propagation for the HTML document canvas.
///
/// CSS Overflow propagates the root element's overflow to the viewport. When
/// the HTML root has visible overflow, its first eligible body child provides
/// that propagated value instead; the source element then uses `visible` for
/// layout. This is a used-value concern, kept separate from `ComputedStyle`:
/// <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct DocumentCanvasOverflowContext {
    viewport_overflow_source: Option<ElementId>,
    root_has_containment: bool,
    viewport_overflow_x: css::Overflow,
    viewport_overflow_y: css::Overflow,
    viewport_uses_auto_overflow: bool,
    viewport_has_auto_overflow: bool,
}

impl Default for DocumentCanvasOverflowContext {
    fn default() -> Self {
        Self {
            viewport_overflow_source: None,
            root_has_containment: false,
            viewport_overflow_x: css::Overflow::Visible,
            viewport_overflow_y: css::Overflow::Visible,
            viewport_uses_auto_overflow: false,
            viewport_has_auto_overflow: false,
        }
    }
}

impl DocumentCanvasOverflowContext {
    pub(in crate::layout) fn from_page_box(page_box: &box_tree::PageBox<'_>) -> Self {
        let Some((html, _, html_style, html_children)) = page_box
            .children
            .iter()
            .find_map(box_tree::FormattingBox::element_parts)
            .filter(|(element, _, _, _)| {
                has_html_rendering_semantics(element) && element.tag == "html"
            })
        else {
            return Self::default();
        };
        let body = html_children.iter().find_map(|child| {
            child
                .element_parts()
                .filter(|(element, _, style, _)| {
                    has_html_rendering_semantics(element)
                        && element.tag == "body"
                        && !style.display.is_none()
                })
                .map(|(element, _, style, _)| (element, style))
        });
        let root_has_containment = style_has_property_containment(html_style);
        let body_provides_viewport_overflow = body.is_some()
            && !root_has_containment
            && body.is_some_and(|(_, style)| !style_has_property_containment(style))
            && html_style.overflow_x == css::Overflow::Visible
            && html_style.overflow_y == css::Overflow::Visible;
        let (viewport_overflow_source, viewport_style) = if body_provides_viewport_overflow {
            let (body, body_style) = body.expect("body overflow source was checked above");
            (Some(body.id), body_style)
        } else if !root_has_containment {
            (Some(html.id), html_style)
        } else {
            (None, html_style)
        };
        let (viewport_overflow_x, viewport_overflow_y) = viewport_overflow_axes(viewport_style);
        Self {
            viewport_overflow_source,
            root_has_containment,
            viewport_overflow_x,
            viewport_overflow_y,
            // Keep static-PDF scrollbar chrome opt-in: a visible viewport is
            // specified as auto, but ordinary overflowing pages must not gain
            // synthetic scrollbar tracks merely because their canvas is
            // longer than a page.
            viewport_uses_auto_overflow: effective_overflow_for_style(viewport_style)
                == css::Overflow::Auto,
            viewport_has_auto_overflow: false,
        }
    }

    pub(in crate::layout) fn used_overflow(
        self,
        element: &Element,
        style: &ComputedStyle,
    ) -> css::Overflow {
        if !has_html_rendering_semantics(element) {
            return effective_overflow_for_style(style);
        }
        if self.is_viewport_overflow_source(element) {
            css::Overflow::Visible
        } else {
            effective_overflow_for_style(style)
        }
    }

    /// Return whether this exact principal element supplies the viewport's
    /// used overflow. Multiple HTML body elements must not be conflated.
    pub(in crate::layout) fn is_viewport_overflow_source(self, element: &Element) -> bool {
        self.viewport_overflow_source == Some(element.id)
    }

    /// Whether propagated root/body overflow clips the document viewport.
    pub(in crate::layout) fn viewport_clips_block_fragmentation(self) -> bool {
        // A viewport with an automatic axis remains printable in static
        // media. Only a fully non-automatic viewport with a hidden vertical
        // axis retains a finite page-height clip.
        self.viewport_overflow_x != css::Overflow::Auto
            && self.viewport_overflow_y == css::Overflow::Hidden
    }

    /// Records that the propagated automatic viewport overflow needs classic
    /// scrollbar tracks. PDF has no platform scroll UI, so layout retains this
    /// geometry explicitly for the final viewport-chrome paint phase.
    pub(in crate::layout) fn record_auto_overflow(
        &mut self,
        content_width: f32,
        content_height: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) {
        self.viewport_has_auto_overflow |= self.viewport_uses_auto_overflow
            && (content_width > viewport_width + 0.01 || content_height > viewport_height + 0.01);
    }

    pub(in crate::layout) fn has_auto_scrollbar_tracks(self) -> bool {
        self.viewport_has_auto_overflow
    }
}

/// Normalize propagated overflow for the viewport.
///
/// CSS Overflow treats visible viewport overflow as auto and clip as hidden,
/// independently in each physical axis.
/// <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>
fn viewport_overflow_axes(style: &ComputedStyle) -> (css::Overflow, css::Overflow) {
    let (overflow_x, overflow_y) = resolved_overflow_axes(style);
    let normalize = |overflow| match overflow {
        css::Overflow::Visible => css::Overflow::Auto,
        css::Overflow::Clip => css::Overflow::Hidden,
        overflow => overflow,
    };
    (normalize(overflow_x), normalize(overflow_y))
}

/// Collapse the two computed overflow axes to the representative value used
/// by layout decisions that only distinguish `visible` from a scroll/clip
/// container.
///
/// CSS Overflow keeps `overflow-x` and `overflow-y` distinct, and makes a
/// visible axis effectively scrollable when the other axis is non-visible.
/// Quire's clip geometry is rectangular, so any non-visible axis establishes
/// the shared clip/BFC decision while axis-specific scroll UI remains a
/// separate concern.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>
pub(super) fn effective_overflow_for_style(style: &ComputedStyle) -> css::Overflow {
    let (overflow_x, overflow_y) = resolved_overflow_axes(style);
    if overflow_x == css::Overflow::Visible && overflow_y == css::Overflow::Visible {
        return style.overflow;
    }
    [overflow_x, overflow_y]
        .into_iter()
        .find(|overflow| *overflow != css::Overflow::Visible)
        .unwrap_or(style.overflow)
}

/// Return Overflow's cross-axis adjusted computed values.
///
/// The cascade normally applies this adjustment, but layout also creates
/// derived styles (for fragments and table internals). Reapplying the pure
/// computed-value rule at the layout boundary keeps those derived values from
/// accidentally leaving a companion axis visible.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>
pub(super) fn resolved_overflow_axes(style: &ComputedStyle) -> (css::Overflow, css::Overflow) {
    let mut overflow_x = style.overflow_x;
    let mut overflow_y = style.overflow_y;
    if !matches!(overflow_x, css::Overflow::Visible | css::Overflow::Clip) {
        overflow_y = match overflow_y {
            css::Overflow::Visible => css::Overflow::Auto,
            css::Overflow::Clip => css::Overflow::Hidden,
            overflow => overflow,
        };
    }
    if !matches!(overflow_y, css::Overflow::Visible | css::Overflow::Clip) {
        overflow_x = match overflow_x {
            css::Overflow::Visible => css::Overflow::Auto,
            css::Overflow::Clip => css::Overflow::Hidden,
            overflow => overflow,
        };
    }
    (overflow_x, overflow_y)
}

pub(super) fn style_clips_overflow(style: &ComputedStyle) -> bool {
    effective_overflow_for_style(style).clips_overflow()
}

/// Return the physical axes that use the CSS Overflow clip edge.
///
/// `hidden`, `scroll`, and `auto` use a padding-edge scrollport; the expanded
/// overflow clip edge is specific to the `clip` keyword. A visible companion
/// axis remains unbounded.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-clip-edge>
pub(super) fn overflow_clip_edge_axes(style: &ComputedStyle) -> (bool, bool) {
    let (overflow_x, overflow_y) = resolved_overflow_axes(style);
    (
        overflow_x == css::Overflow::Clip,
        overflow_y == css::Overflow::Clip,
    )
}

/// Return the physical axes whose visual overflow is bounded at all.
pub(super) fn overflow_clipping_axes(style: &ComputedStyle) -> (bool, bool) {
    let (overflow_x, overflow_y) = resolved_overflow_axes(style);
    (
        overflow_x != css::Overflow::Visible,
        overflow_y != css::Overflow::Visible,
    )
}

/// Return whether any containment effect prevents special root/body canvas
/// property propagation.
///
/// CSS Containment Level 1 prevents a contained root or body principal box
/// from propagating background, overflow, and principal writing-mode canvas
/// properties. `content` and `strict` are already expanded into these bits.
/// <https://www.w3.org/TR/css-contain-1/#containment-layout>
pub(super) fn style_has_property_containment(style: &ComputedStyle) -> bool {
    style.contain.size
        || style.contain.layout
        || style.contain.paint
        || style.contain.style
        || !matches!(style.content_visibility, ContentVisibility::Visible)
}

/// Return whether size/layout/paint containment applies to this principal box.
///
/// CSS Containment excludes non-atomic inline boxes and layout-internal ruby
/// boxes. Table cells remain containment-capable, while the other internal
/// table track/group boxes do not establish the required principal formatting
/// context.
/// <https://www.w3.org/TR/css-contain-1/#containment-principal>
pub(super) fn property_containment_applies_to_element(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    if style.display.is_inline_level() && !style.display.is_atomic_inline() {
        return false;
    }
    if matches!(element.tag.as_str(), "rb" | "rbc" | "rt" | "rtc") {
        return false;
    }
    !matches!(
        style.display.inner,
        DisplayInner::TableColumnGroup
            | DisplayInner::TableColumn
            | DisplayInner::TableHeaderGroup
            | DisplayInner::TableRowGroup
            | DisplayInner::TableFooterGroup
            | DisplayInner::TableRow
    )
}

pub(super) fn propagates_document_canvas_properties(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    is_document_canvas_element(element) && !style_has_property_containment(style)
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn element_propagates_document_canvas_properties(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        self.element_side_effect_suppression_depth == 0
            && propagates_document_canvas_properties(element, style)
            && !(element.tag == "body"
                && (self.document_canvas_overflow.root_has_containment
                    || self.document_canvas_root_background_defined()))
    }

    /// Returns the element's overflow after document-canvas propagation.
    pub(in crate::layout) fn used_overflow_for_element(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> css::Overflow {
        self.document_canvas_overflow.used_overflow(element, style)
    }

    /// Returns whether an element establishes a local overflow clip after
    /// document-canvas overflow propagation.
    pub(in crate::layout) fn element_used_overflow_clips(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        (self
            .used_overflow_for_element(element, style)
            .clips_overflow()
            || (style.contain.paint && property_containment_applies_to_element(element, style)))
            && !self
                .document_canvas_overflow
                .is_viewport_overflow_source(element)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplacedElementKind {
    Canvas,
    Image,
    Svg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ElementLayoutKind {
    None,
    Positioned,
    Canvas,
    Image,
    GeneratedImage,
    Svg,
    Flex,
    Grid,
    Table,
    InlineFlow,
    BlockFlow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DefinitionListItemKind {
    Term,
    Description,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ListContainerKind {
    Ordered,
    Unordered,
    Other,
}

pub(super) fn element_layout_kind(element: &Element, style: &ComputedStyle) -> ElementLayoutKind {
    if style.display.is_none() {
        return ElementLayoutKind::None;
    }
    if matches!(style.position, Position::Absolute | Position::Fixed) {
        return ElementLayoutKind::Positioned;
    }
    if matches!(style.content, Content::Replacement { .. }) {
        return ElementLayoutKind::GeneratedImage;
    }
    match replaced_element_kind(element) {
        Some(ReplacedElementKind::Canvas) => return ElementLayoutKind::Canvas,
        Some(ReplacedElementKind::Image) => return ElementLayoutKind::Image,
        Some(ReplacedElementKind::Svg) => return ElementLayoutKind::Svg,
        None => {}
    }
    if style.display.is_flex() {
        return ElementLayoutKind::Flex;
    }
    if style.display.is_grid() {
        return ElementLayoutKind::Grid;
    }
    if style.display.is_table() || is_html_table_element(element) {
        return ElementLayoutKind::Table;
    }
    if style.display.is_inline_level() && style.display.is_flow() {
        ElementLayoutKind::InlineFlow
    } else {
        ElementLayoutKind::BlockFlow
    }
}

pub(super) fn replaced_element_kind(element: &Element) -> Option<ReplacedElementKind> {
    // CSS Display treats embedded document/media elements as replaced
    // elements. HTML defines `<canvas>` with intrinsic dimensions and CSS
    // Images/Sizing treats external images as replaced elements; this port also
    // treats the target's inline root `<svg>` snippets as replaced atoms until
    // full SVG layout integration exists.
    // https://www.w3.org/TR/css-display-3/#replaced-element
    // https://html.spec.whatwg.org/multipage/canvas.html#the-canvas-element
    if element.namespace_url == SVG_NAMESPACE_URL && element.tag == "svg" {
        return Some(ReplacedElementKind::Svg);
    }
    if !has_html_rendering_semantics(element) {
        return None;
    }
    match element.tag.as_str() {
        // Canvas and embedded documents have no Quire paint resource unless a
        // future renderer supplies one, but CSS still lays each out as one
        // atomic replaced box.  The canvas path already provides the required
        // default-object geometry and paints only author-provided box
        // decoration.
        // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-iframe-element>
        "canvas" | "iframe" => Some(ReplacedElementKind::Canvas),
        // These HTML embedding elements all have a raster fallback path when
        // their selected resource is an image.  Keeping them on the same
        // replaced-image layout path gives CSS Images one concrete-object
        // implementation for img, embed, object, and video poster images.
        // <https://html.spec.whatwg.org/multipage/embedded-content.html>
        "img" | "embed" | "object" | "video" => Some(ReplacedElementKind::Image),
        _ => None,
    }
}

pub(super) fn is_replaced_element(element: &Element) -> bool {
    replaced_element_kind(element).is_some()
}

pub(super) fn is_horizontal_rule_element(element: &Element) -> bool {
    // HTML `hr` is a thematic break, not a CSS replaced element. Keep this as
    // a semantic hook for void/childless box-tree construction; layout and
    // painting are ordinary CSS block behavior from the UA stylesheet.
    // https://html.spec.whatwg.org/multipage/grouping-content.html#the-hr-element
    has_html_rendering_semantics(element) && element.tag == "hr"
}

pub(super) fn is_line_break_element(element: &Element) -> bool {
    // HTML `br` creates a forced line break in inline formatting.
    // https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element
    has_html_rendering_semantics(element) && element.tag == "br"
}

pub(super) fn is_document_canvas_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && matches!(element.tag.as_str(), "html" | "body")
}

/// Return whether the element's used overflow clips its own box.
///
/// Return raw style clipping for layout paths that do not have a document
/// canvas context. Context-aware principal block layout replaces this with the
/// selected viewport overflow source's used `visible` value.
pub(super) fn used_overflow_clips_element(element: &Element, style: &ComputedStyle) -> bool {
    style_clips_overflow(style)
        || (style.contain.paint && property_containment_applies_to_element(element, style))
}

pub(super) fn is_html_table_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "table"
}

pub(super) fn is_table_or_replaced_element(element: &Element) -> bool {
    is_html_table_element(element) || is_replaced_element(element)
}

pub(super) fn suppresses_ordered_mixed_flow_detection(element: &Element) -> bool {
    // These elements either paint the document canvas, manage list markers, or
    // run table construction; the ordered mixed-flow fallback would duplicate
    // those formatting-context-specific layout paths.
    is_document_canvas_element(element)
        || is_html_select_element(element)
        || is_html_select_item_element(element)
        || matches!(
            list_container_kind(element),
            ListContainerKind::Ordered | ListContainerKind::Unordered
        )
        || is_html_table_element(element)
}

pub(super) fn is_html_table_caption_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "caption"
}

pub(super) fn is_html_table_column_group_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "colgroup"
}

pub(super) fn is_html_table_column_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "col"
}

pub(super) fn is_html_table_row_group_element(element: &Element) -> bool {
    has_html_rendering_semantics(element)
        && matches!(element.tag.as_str(), "thead" | "tbody" | "tfoot")
}

pub(super) fn is_html_table_header_group_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "thead"
}

pub(super) fn is_html_table_footer_group_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "tfoot"
}

pub(super) fn is_html_table_row_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "tr"
}

pub(super) fn is_html_table_cell_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && matches!(element.tag.as_str(), "td" | "th")
}

/// Return whether this element is an HTML `select` form control.
///
/// HTML form controls have rendering behavior that is not fully modeled by
/// ordinary CSS boxes, while CSS Display still lets `display: none` suppress
/// their option subtrees:
/// <https://html.spec.whatwg.org/multipage/rendering.html#widgets> and
/// <https://drafts.csswg.org/css-display-3/#valdef-display-none>.
pub(super) fn is_html_select_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "select"
}

/// Return whether this element is an HTML `option` candidate.
///
/// `option` participates in select/optgroup form-control rendering, but its
/// CSS box is still omitted when `display: none` computes on the element:
/// <https://html.spec.whatwg.org/multipage/form-elements.html#the-option-element>
/// and <https://drafts.csswg.org/css-display-3/#valdef-display-none>.
pub(super) fn is_html_option_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "option"
}

/// Return whether this element is an HTML `optgroup` candidate.
///
/// `optgroup` groups options inside a select, and `display: none` on the group
/// suppresses the group's generated boxes and descendant option boxes:
/// <https://html.spec.whatwg.org/multipage/form-elements.html#the-optgroup-element>
/// and <https://drafts.csswg.org/css-display-3/#valdef-display-none>.
pub(super) fn is_html_optgroup_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "optgroup"
}

pub(super) fn is_html_select_item_element(element: &Element) -> bool {
    is_html_option_element(element) || is_html_optgroup_element(element)
}

pub(super) fn element_suppresses_direct_text_children(element: &Element) -> bool {
    is_html_select_element(element) || is_html_optgroup_element(element)
}

pub(super) fn has_html_select_context(parent: &Element, ancestors: &[ElementSignature]) -> bool {
    is_html_select_element(parent) || ancestors.iter().any(|ancestor| ancestor.tag == "select")
}

pub(super) fn definition_list_item_kind(element: &Element) -> DefinitionListItemKind {
    if !has_html_rendering_semantics(element) {
        return DefinitionListItemKind::Other;
    }
    match element.tag.as_str() {
        "dt" => DefinitionListItemKind::Term,
        "dd" => DefinitionListItemKind::Description,
        _ => DefinitionListItemKind::Other,
    }
}

pub(super) fn is_definition_list_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "dl"
}

pub(super) fn list_container_kind(element: &Element) -> ListContainerKind {
    if !has_html_rendering_semantics(element) {
        return ListContainerKind::Other;
    }
    match element.tag.as_str() {
        "ol" => ListContainerKind::Ordered,
        "ul" => ListContainerKind::Unordered,
        _ => ListContainerKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html_element(tag: &str) -> Element {
        let NodeKind::Element(element) = Node::element(tag).kind else {
            unreachable!("element constructor must produce an element")
        };
        element
    }

    #[test]
    fn root_overflow_is_used_by_the_viewport() {
        let html = html_element("html");
        let mut style = ComputedStyle::initial();
        style.overflow = css::Overflow::Hidden;

        assert_eq!(
            DocumentCanvasOverflowContext {
                viewport_overflow_source: Some(html.id),
                ..DocumentCanvasOverflowContext::default()
            }
            .used_overflow(&html, &style),
            css::Overflow::Visible
        );
    }

    #[test]
    fn propagated_body_overflow_is_visible_for_layout() {
        let body = html_element("body");
        let mut style = ComputedStyle::initial();
        style.overflow = css::Overflow::Hidden;

        assert_eq!(
            DocumentCanvasOverflowContext {
                viewport_overflow_source: Some(body.id),
                root_has_containment: false,
                viewport_overflow_x: css::Overflow::Visible,
                viewport_overflow_y: css::Overflow::Visible,
                viewport_uses_auto_overflow: false,
                viewport_has_auto_overflow: false,
            }
            .used_overflow(&body, &style),
            css::Overflow::Visible
        );
    }

    #[test]
    fn non_propagated_body_overflow_remains_effective() {
        let body = html_element("body");
        let mut style = ComputedStyle::initial();
        style.overflow = css::Overflow::Hidden;

        assert_eq!(
            DocumentCanvasOverflowContext::default().used_overflow(&body, &style),
            css::Overflow::Hidden
        );
    }

    #[test]
    fn only_the_selected_body_loses_its_local_overflow_clip() {
        let selected = html_element("body");
        let other = html_element("body");
        let mut style = ComputedStyle::initial();
        style.overflow = css::Overflow::Hidden;
        let context = DocumentCanvasOverflowContext {
            viewport_overflow_source: Some(selected.id),
            ..DocumentCanvasOverflowContext::default()
        };

        assert!(context.is_viewport_overflow_source(&selected));
        assert!(!context.is_viewport_overflow_source(&other));
        assert!(style_clips_overflow(&style));
    }

    #[test]
    fn viewport_normalization_is_per_axis() {
        let mut style = ComputedStyle::initial();
        style.overflow_x = css::Overflow::Clip;
        style.overflow_y = css::Overflow::Visible;

        assert_eq!(
            viewport_overflow_axes(&style),
            (css::Overflow::Hidden, css::Overflow::Auto)
        );
    }

    #[test]
    fn xhtml_namespace_uses_html_rendering_semantics() {
        let mut html = html_element("html");
        html.document_syntax = DocumentSyntax::Xml;
        html.namespace_url = XHTML_NAMESPACE_URL.to_string();
        let mut image = html_element("img");
        image.document_syntax = DocumentSyntax::Xml;
        image.namespace_url = XHTML_NAMESPACE_URL.to_string();

        assert!(is_document_canvas_element(&html));
        assert_eq!(
            replaced_element_kind(&image),
            Some(ReplacedElementKind::Image)
        );
    }

    #[test]
    fn unnamespaced_xml_elements_do_not_acquire_html_rendering_semantics() {
        let mut image = html_element("img");
        image.document_syntax = DocumentSyntax::Xml;

        assert_eq!(replaced_element_kind(&image), None);
    }
}
