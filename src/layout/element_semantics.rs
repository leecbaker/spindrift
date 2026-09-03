use super::*;
use crate::dom::{DocumentSyntax, ImageRendering, ObjectRendering};

const XHTML_NAMESPACE_URL: &str = "http://www.w3.org/1999/xhtml";
const SVG_NAMESPACE_URL: &str = "http://www.w3.org/2000/svg";

/// One used overflow value in a physical axis.
///
/// CSS Overflow's clipping and scrolling semantics are axis-specific.  This
/// representation deliberately does not let a caller reduce `clip visible`
/// to one generic "clipping" value.
/// <https://drafts.csswg.org/css-overflow-3/#overflow-properties>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum UsedOverflowAxis {
    Visible,
    Clip,
    ScrollContainer(css::Overflow),
}

impl UsedOverflowAxis {
    const fn from_overflow(overflow: css::Overflow) -> Self {
        match overflow {
            css::Overflow::Visible => Self::Visible,
            css::Overflow::Clip => Self::Clip,
            css::Overflow::Hidden | css::Overflow::Scroll | css::Overflow::Auto => {
                Self::ScrollContainer(overflow)
            }
        }
    }

    const fn overflow(self) -> css::Overflow {
        match self {
            Self::Visible => css::Overflow::Visible,
            Self::Clip => css::Overflow::Clip,
            Self::ScrollContainer(overflow) => overflow,
        }
    }

    pub(in crate::layout) const fn clips(self) -> bool {
        !matches!(self, Self::Visible)
    }

    pub(in crate::layout) const fn is_scroll_container(self) -> bool {
        matches!(self, Self::ScrollContainer(_))
    }

    pub(in crate::layout) const fn is_non_scrollable_clip(self) -> bool {
        matches!(self, Self::Clip)
    }
}

/// Used overflow for the physical horizontal and vertical axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct UsedOverflowAxes {
    pub(in crate::layout) horizontal: UsedOverflowAxis,
    pub(in crate::layout) vertical: UsedOverflowAxis,
}

impl UsedOverflowAxes {
    const VISIBLE: Self = Self {
        horizontal: UsedOverflowAxis::Visible,
        vertical: UsedOverflowAxis::Visible,
    };

    pub(in crate::layout) fn from_style(style: &ComputedStyle) -> Self {
        let (horizontal, vertical) = resolved_overflow_axes(style);
        Self {
            horizontal: UsedOverflowAxis::from_overflow(horizontal),
            vertical: UsedOverflowAxis::from_overflow(vertical),
        }
    }

    /// SVG viewport clipping does not establish a CSS scroll container for
    /// `hidden`; it is a non-scrollable viewport clip and therefore retains
    /// the authored overflow clip margin.
    /// <https://svgwg.org/svg2-draft/render.html#OverflowAndClipProperties>
    pub(in crate::layout) fn from_svg_viewport_style(style: &ComputedStyle) -> Self {
        let (horizontal, vertical) = resolved_overflow_axes(style);
        let axis = |overflow| match overflow {
            css::Overflow::Visible => UsedOverflowAxis::Visible,
            css::Overflow::Clip | css::Overflow::Hidden => UsedOverflowAxis::Clip,
            css::Overflow::Scroll | css::Overflow::Auto => {
                UsedOverflowAxis::ScrollContainer(overflow)
            }
        };
        Self {
            horizontal: axis(horizontal),
            vertical: axis(vertical),
        }
    }

    fn from_viewport_style(style: &ComputedStyle) -> Self {
        let (horizontal, vertical) = viewport_overflow_axes(style);
        Self {
            horizontal: UsedOverflowAxis::from_overflow(horizontal),
            vertical: UsedOverflowAxis::from_overflow(vertical),
        }
    }

    pub(in crate::layout) const fn clips_any_axis(self) -> bool {
        self.horizontal.clips() || self.vertical.clips()
    }

    pub(in crate::layout) const fn clips_x(self) -> bool {
        self.horizontal.clips()
    }

    pub(in crate::layout) const fn clips_y(self) -> bool {
        self.vertical.clips()
    }

    pub(in crate::layout) const fn non_scrollable_clip_x(self) -> bool {
        self.horizontal.is_non_scrollable_clip()
    }

    pub(in crate::layout) const fn non_scrollable_clip_y(self) -> bool {
        self.vertical.is_non_scrollable_clip()
    }

    fn representative(self, fallback: css::Overflow) -> css::Overflow {
        if self.horizontal == UsedOverflowAxis::Visible
            && self.vertical == UsedOverflowAxis::Visible
        {
            fallback
        } else {
            [self.horizontal, self.vertical]
                .into_iter()
                .map(UsedOverflowAxis::overflow)
                .find(|overflow| *overflow != css::Overflow::Visible)
                .unwrap_or(fallback)
        }
    }
}

/// Reserved physical scrollbar gutters around a scrollport.
///
/// These are layout lengths, not CSS box-model padding: callers must obtain
/// the adjusted paint rectangle through [`ScrollportGeometry`] rather than
/// subtracting a raw scalar in an arbitrary formatting context.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PhysicalScrollbarGutters {
    pub(in crate::layout) left: LayoutLength,
    pub(in crate::layout) right: LayoutLength,
    pub(in crate::layout) top: LayoutLength,
    pub(in crate::layout) bottom: LayoutLength,
}

impl PhysicalScrollbarGutters {
    /// The reserved space along the physical horizontal and vertical axes.
    ///
    /// A vertical scrollbar consumes horizontal layout space and vice versa.
    /// Keep this conversion on the physical scrollport primitive so formatting
    /// contexts do not independently re-derive it from overflow longhands.
    pub(in crate::layout) fn horizontal_extent(self) -> LayoutLength {
        LayoutLength::new(self.left.points() + self.right.points())
    }

    pub(in crate::layout) fn vertical_extent(self) -> LayoutLength {
        LayoutLength::new(self.top.points() + self.bottom.points())
    }
}

/// A pre-layout scrollbar-gutter reservation for one physical padding box.
///
/// CSS Overflow adds a reserved scrollbar gutter to intrinsic sizes, but
/// deducts it from otherwise-allotted content space.  This record makes the
/// reservation available before a formatting context knows its final padding
/// rectangle, and is later consumed by [`ScrollportGeometry`] for clipping.
/// <https://drafts.csswg.org/css-overflow-3/#scrollbars-layout>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ScrollbarGutterReservation {
    gutters: PhysicalScrollbarGutters,
}

impl ScrollbarGutterReservation {
    /// Spindrift's static PDF backend has no native interactive scrollbar chrome,
    /// and therefore selects the CSS Overflow overlay-scrollbar UA policy.
    /// Overlay scrollbars reserve no layout space.
    /// <https://drafts.csswg.org/css-overflow-3/#scrollbar-gutter-property>
    pub(in crate::layout) fn static_pdf_overlay() -> Self {
        Self {
            gutters: PhysicalScrollbarGutters {
                left: LayoutLength::new(0.0),
                right: LayoutLength::new(0.0),
                top: LayoutLength::new(0.0),
                bottom: LayoutLength::new(0.0),
            },
        }
    }

    /// Resolve a static-media classic-scrollbar reservation.
    ///
    /// Callers that have not yet established automatic overflow pass `false`
    /// for both overflow flags, as required by CSS Overflow's initial
    /// no-scrollbar sizing assumption.
    pub(in crate::layout) fn for_style(
        style: &ComputedStyle,
        overflow: UsedOverflowAxes,
        has_overflow_x: bool,
        has_overflow_y: bool,
    ) -> Self {
        let thickness = match style.scrollbar_width {
            css::ScrollbarWidth::None => LayoutLength::new(0.0),
            css::ScrollbarWidth::Thin => LayoutLength::new(8.0 * css::CSS_PX_TO_PT),
            css::ScrollbarWidth::Auto => LayoutLength::new(15.0 * css::CSS_PX_TO_PT),
        };
        let reserves_x = reserves_scrollbar_gutter(
            overflow.horizontal,
            style.scrollbar_gutter,
            style.scrollbar_width,
            has_overflow_x,
        );
        let reserves_y = reserves_scrollbar_gutter(
            overflow.vertical,
            style.scrollbar_gutter,
            style.scrollbar_width,
            has_overflow_y,
        );
        let both_edges = matches!(
            style.scrollbar_gutter,
            css::ScrollbarGutter::Stable { both_edges: true }
        );
        Self {
            gutters: PhysicalScrollbarGutters {
                left: if reserves_y && both_edges {
                    thickness
                } else {
                    LayoutLength::new(0.0)
                },
                right: if reserves_y {
                    thickness
                } else {
                    LayoutLength::new(0.0)
                },
                top: if reserves_x && both_edges {
                    thickness
                } else {
                    LayoutLength::new(0.0)
                },
                bottom: if reserves_x {
                    thickness
                } else {
                    LayoutLength::new(0.0)
                },
            },
        }
    }

    pub(in crate::layout) fn gutters(self) -> PhysicalScrollbarGutters {
        self.gutters
    }

    pub(in crate::layout) fn horizontal_extent(self) -> LayoutLength {
        self.gutters.horizontal_extent()
    }

    pub(in crate::layout) fn vertical_extent(self) -> LayoutLength {
        self.gutters.vertical_extent()
    }
}

/// Resolved scrollport geometry for a single physical padding box.
///
/// The static PDF UA reserves deterministic classic-gutter space but does
/// not paint native interactive chrome. Keeping this value explicit lets
/// layout, background positioning, and clipping agree on that choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ScrollportGeometry {
    pub(in crate::layout) padding_box: PaintClip,
    pub(in crate::layout) scrollport: PaintClip,
    pub(in crate::layout) gutters: PhysicalScrollbarGutters,
}

impl ScrollportGeometry {
    /// Resolve static-media scrollbar reservation against known overflow.
    /// `auto` receives the actual overflow flags, while `stable` and `scroll`
    /// can reserve space before descendant layout has finished.
    pub(in crate::layout) fn for_padding_box(
        padding_box: PaintClip,
        style: &ComputedStyle,
        overflow: UsedOverflowAxes,
        has_overflow_x: bool,
        has_overflow_y: bool,
    ) -> Self {
        Self::for_padding_box_with_reservation(
            padding_box,
            ScrollbarGutterReservation::for_style(style, overflow, has_overflow_x, has_overflow_y),
        )
    }

    /// Construct a scrollport from an already-resolved layout reservation.
    pub(in crate::layout) fn for_padding_box_with_reservation(
        padding_box: PaintClip,
        reservation: ScrollbarGutterReservation,
    ) -> Self {
        let gutters = reservation.gutters();
        let left = gutters.left.points();
        let right = gutters.right.points();
        let top = gutters.top.points();
        let bottom = gutters.bottom.points();
        let scrollport = PaintClip::new(
            padding_box.x() + left,
            padding_box.y() + bottom,
            (padding_box.width() - left - right).max(0.0),
            (padding_box.height() - top - bottom).max(0.0),
        );
        Self {
            padding_box,
            scrollport,
            gutters,
        }
    }
}

fn reserves_scrollbar_gutter(
    overflow: UsedOverflowAxis,
    gutter: css::ScrollbarGutter,
    width: css::ScrollbarWidth,
    has_overflow: bool,
) -> bool {
    if width == css::ScrollbarWidth::None || !overflow.is_scroll_container() {
        return false;
    }
    match overflow.overflow() {
        css::Overflow::Scroll => true,
        css::Overflow::Auto => {
            has_overflow || matches!(gutter, css::ScrollbarGutter::Stable { .. })
        }
        css::Overflow::Hidden => matches!(gutter, css::ScrollbarGutter::Stable { .. }),
        css::Overflow::Visible | css::Overflow::Clip => false,
    }
}

/// The principal HTML element currently supplying the viewport overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewportOverflowSource {
    Root(ElementId),
    Body(ElementId),
}

impl ViewportOverflowSource {
    const fn element_id(self) -> ElementId {
        match self {
            Self::Root(id) | Self::Body(id) => id,
        }
    }
}

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

/// Resolved document-level propagation for the HTML document canvas.
///
/// CSS Overflow propagates the root element's overflow to the viewport. When
/// the HTML root has visible overflow, its first eligible body child provides
/// that propagated value instead; the source element then uses `visible` for
/// layout. This is a used-value concern, kept separate from `ComputedStyle`:
/// <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct DocumentCanvasResolution {
    root: Option<ElementId>,
    /// The first eligible body whose properties propagate to the document
    /// canvas.  CSS Containment disables every such body propagation when
    /// either the root or that body has a non-`none` used `contain` value.
    /// <https://drafts.csswg.org/css-contain-1/#contain-property>
    propagated_body: Option<ElementId>,
    principal_flow: DocumentPrincipalFlow,
    viewport_overflow_source: Option<ViewportOverflowSource>,
    viewport_overflow: UsedOverflowAxes,
    viewport_uses_auto_overflow: bool,
    viewport_has_auto_overflow: bool,
}

impl Default for DocumentCanvasResolution {
    fn default() -> Self {
        Self {
            root: None,
            propagated_body: None,
            principal_flow: DocumentPrincipalFlow {
                writing_mode: WritingMode::HorizontalTb,
                direction: Direction::Ltr,
                text_orientation: TextOrientation::Mixed,
                source: PrincipalFlowSource::Root,
            },
            viewport_overflow_source: None,
            viewport_overflow: UsedOverflowAxes::VISIBLE,
            viewport_uses_auto_overflow: false,
            viewport_has_auto_overflow: false,
        }
    }
}

impl DocumentCanvasResolution {
    pub(in crate::layout) fn from_page_box<S>(page_box: &box_tree::PageBoxWith<'_, S>) -> Self
    where
        S: AsRef<ComputedStyle>,
    {
        let Some((html, _, html_style, html_children)) = page_box
            .children
            .iter()
            .find_map(|child| child.element_parts())
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
        let propagated_body = body.filter(|(_, style)| {
            !style_has_active_containment(html_style) && !style_has_active_containment(style)
        });
        // When body property propagation is disabled, root overflow still
        // supplies the viewport; the body keeps its own overflow behavior.
        // <https://drafts.csswg.org/css-contain-1/#contain-property>
        // <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>
        let body_provides_viewport_overflow = propagated_body.is_some()
            && html_style.overflow_x == css::Overflow::Visible
            && html_style.overflow_y == css::Overflow::Visible;
        let (viewport_overflow_source, viewport_style) = if body_provides_viewport_overflow {
            let (body, body_style) =
                propagated_body.expect("body overflow source was checked above");
            (Some(ViewportOverflowSource::Body(body.id)), body_style)
        } else {
            (Some(ViewportOverflowSource::Root(html.id)), html_style)
        };
        let viewport_overflow = UsedOverflowAxes::from_viewport_style(viewport_style);
        let principal_flow = propagated_body.map_or_else(
            || DocumentPrincipalFlow::from_style(html_style),
            |(body, body_style)| DocumentPrincipalFlow {
                writing_mode: body_style.writing_mode,
                direction: body_style.direction,
                text_orientation: body_style.text_orientation,
                source: PrincipalFlowSource::Body(body.id),
            },
        );
        Self {
            root: Some(html.id),
            propagated_body: propagated_body.map(|(body, _)| body.id),
            principal_flow,
            viewport_overflow_source,
            viewport_overflow,
            // Keep static-PDF scrollbar chrome opt-in: a visible viewport is
            // specified as auto, but ordinary overflowing pages must not gain
            // synthetic scrollbar tracks merely because their canvas is
            // longer than a page.
            viewport_uses_auto_overflow: effective_overflow_for_style(viewport_style)
                == css::Overflow::Auto,
            viewport_has_auto_overflow: false,
        }
    }

    /// Returns the writing-mode principal flow selected from the same
    /// cascaded root/body pair as canvas background and overflow propagation.
    pub(in crate::layout) fn principal_flow(self) -> DocumentPrincipalFlow {
        self.principal_flow
    }

    pub(in crate::layout) fn used_overflow(
        self,
        element: &Element,
        style: &ComputedStyle,
    ) -> css::Overflow {
        // Propagating the root or eligible body overflow to the viewport
        // changes both of the source element's used axes to `visible`.  Do
        // not use the computed shorthand as the representative fallback here:
        // it may still be `hidden`, `scroll`, or `auto`.
        // <https://drafts.csswg.org/css-overflow-3/#overflow-propagation>
        if self.is_viewport_overflow_source(element) {
            css::Overflow::Visible
        } else {
            UsedOverflowAxes::from_style(style).representative(style.overflow)
        }
    }

    /// Return the element's used physical overflow axes after document-canvas
    /// propagation.  The selected source becomes visible locally while every
    /// other principal element retains its own axis-specific behavior.
    pub(in crate::layout) fn used_overflow_axes(
        self,
        element: &Element,
        style: &ComputedStyle,
    ) -> UsedOverflowAxes {
        if has_html_rendering_semantics(element) && self.is_viewport_overflow_source(element) {
            UsedOverflowAxes::VISIBLE
        } else {
            UsedOverflowAxes::from_style(style)
        }
    }

    /// Return whether this exact principal element supplies the viewport's
    /// used overflow. Multiple HTML body elements must not be conflated.
    pub(in crate::layout) fn is_viewport_overflow_source(self, element: &Element) -> bool {
        self.viewport_overflow_source
            .is_some_and(|source| source.element_id() == element.id)
    }

    /// Whether `element` participates in the document canvas flow.
    ///
    /// A contained eligible body is an ordinary principal block, not a
    /// second canvas-flow source. CSS Containment disables the entire body
    /// propagation mechanism, not merely selected propagated properties.
    /// <https://drafts.csswg.org/css-contain-1/#contain-property>
    pub(in crate::layout) fn is_document_canvas_flow_element(self, element: &Element) -> bool {
        self.is_document_canvas_property_source(element)
    }

    /// Whether this element supplies the resolved document principal flow.
    ///
    /// The root always participates in document-canvas property propagation,
    /// but when an eligible body propagates its writing properties it alone
    /// supplies the principal-flow child coordinate system. Conversely, a
    /// containment-disabled body leaves the root as that source.
    /// <https://drafts.csswg.org/css-writing-modes-4/#principal-flow>
    pub(in crate::layout) fn is_document_principal_flow_source(self, element: &Element) -> bool {
        match self.principal_flow.source {
            PrincipalFlowSource::Root => self.root == Some(element.id),
            PrincipalFlowSource::Body(body) => body == element.id,
        }
    }

    /// Whether `element` supplies document-canvas properties. The root
    /// always does; an eligible body does only when containment leaves body
    /// propagation enabled.
    pub(in crate::layout) fn is_document_canvas_property_source(self, element: &Element) -> bool {
        self.root == Some(element.id) || self.propagated_body == Some(element.id)
    }

    /// Whether this element is the root canvas-background source.
    pub(in crate::layout) fn is_root_canvas_background_source(self, element: &Element) -> bool {
        self.root == Some(element.id)
    }

    /// Whether this element can provide the body fallback background for an
    /// otherwise transparent root canvas.
    pub(in crate::layout) fn is_body_canvas_background_fallback_source(
        self,
        element: &Element,
    ) -> bool {
        self.propagated_body == Some(element.id)
    }

    /// Whether propagated root/body overflow clips the document viewport.
    pub(in crate::layout) fn viewport_clips_block_fragmentation(self) -> bool {
        // A viewport with an automatic axis remains printable in static
        // media. Only a fully non-automatic viewport with a hidden vertical
        // axis retains a finite page-height clip.
        self.viewport_overflow.horizontal.overflow() != css::Overflow::Auto
            && self.viewport_overflow.vertical.overflow() == css::Overflow::Hidden
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
/// Spindrift's clip geometry is rectangular, so any non-visible axis establishes
/// the shared clip/BFC decision while axis-specific scroll UI remains a
/// separate concern.
/// <https://www.w3.org/TR/css-overflow-3/#overflow-properties>
pub(super) fn effective_overflow_for_style(style: &ComputedStyle) -> css::Overflow {
    UsedOverflowAxes::from_style(style).representative(style.overflow)
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

/// Return whether any containment effect prevents special root/body canvas
/// property propagation.
///
/// CSS Containment Level 2 prevents a root or body with active containment
/// from propagating background, overflow, and principal writing-mode canvas
/// properties. `content` and `strict` are already expanded into these bits;
/// `content-visibility: auto` and `hidden` add used containment without
/// changing the computed `contain` value.
/// <https://drafts.csswg.org/css-contain-2/#contain-property>
pub(super) fn style_has_active_containment(style: &ComputedStyle) -> bool {
    style.contain.size
        || style.contain.inline_size
        || style.contain.layout
        || style.contain.paint
        || style.contain.style
        || matches!(
            style.content_visibility,
            ContentVisibility::Auto | ContentVisibility::Hidden
        )
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
    if !property_containment_applies_to_style(style) {
        return false;
    }
    if matches!(element.tag.as_str(), "rb" | "rbc" | "rt" | "rtc") {
        return false;
    }
    true
}

/// Return whether a style can carry property containment when the paint path
/// has no element identity available.
///
/// This is deliberately the conservative shared subset of
/// [`property_containment_applies_to_element`]. Paint-only representations
/// such as inline atoms retain a used style but not necessarily their source
/// element. They must still reject non-atomic inline and layout-internal ruby
/// boxes rather than turning an authored `contain` bit into a stacking
/// context.
/// <https://www.w3.org/TR/css-contain-1/#containment-principal>
pub(super) fn property_containment_applies_to_style(style: &ComputedStyle) -> bool {
    if style.display.is_inline_level() && !style.display.is_atomic_inline() {
        return false;
    }
    if style.display.is_ruby_internal() {
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

/// The containment effects which apply to an element's principal box.
///
/// `contain` is a computed-value shorthand, but size, layout, and paint
/// containment have no effect on boxes without a containment-capable
/// principal box.  Keep that used-value distinction at this boundary so
/// formatting, positioning, clipping, and Grid cannot accidentally apply an
/// authored containment bit to a non-applicable inline or table-internal box.
/// <https://drafts.csswg.org/css-contain-1/#containment-principal>
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct UsedPropertyContainment {
    pub(super) size: bool,
    pub(super) inline_size: bool,
    pub(super) layout: bool,
    pub(super) paint: bool,
}

/// Whether descendants may enlarge an ancestor's scrollable-overflow area.
///
/// Layout and paint containment do not suppress descendant ink, but they do
/// isolate scrollable overflow at the contained principal box.
/// <https://www.w3.org/TR/css-contain-1/#containment-layout>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DescendantOverflowContribution {
    Scrollable,
    InkOnly,
}

impl UsedPropertyContainment {
    /// Layout and paint containment establish an independent formatting
    /// context; paint containment additionally clips descendant paint.
    /// <https://www.w3.org/TR/css-contain-1/#containment-layout>
    /// <https://www.w3.org/TR/css-contain-1/#containment-paint>
    pub(super) fn establishes_independent_formatting_context(self) -> bool {
        self.layout || self.paint
    }

    pub(super) fn establishes_fixed_position_containing_block(self) -> bool {
        self.layout || self.paint
    }

    pub(super) fn clips_descendant_paint(self) -> bool {
        self.paint
    }

    pub(super) fn descendant_overflow_contribution(self) -> DescendantOverflowContribution {
        if self.layout || self.paint {
            DescendantOverflowContribution::InkOnly
        } else {
            DescendantOverflowContribution::Scrollable
        }
    }
}

pub(super) fn used_property_containment(
    element: &Element,
    style: &ComputedStyle,
) -> UsedPropertyContainment {
    // Root and body principal boxes remain ordinary containment subjects.
    // The separate document-canvas resolver controls only their special HTML
    // property propagation.
    // <https://drafts.csswg.org/css-contain-1/#containment-principal>
    let applies = property_containment_applies_to_element(element, style);
    UsedPropertyContainment {
        size: applies && style.contain.size,
        inline_size: applies && style.contain.inline_size,
        layout: applies && style.contain.layout,
        paint: applies && style.contain.paint,
    }
}

pub(super) fn layout_containment_applies_to_element(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    used_property_containment(element, style).layout
}

pub(super) fn property_containment_establishes_independent_formatting_context(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    used_property_containment(element, style).establishes_independent_formatting_context()
}

pub(super) fn paint_containment_applies_to_element(
    element: &Element,
    style: &ComputedStyle,
) -> bool {
    used_property_containment(element, style).paint
}

pub(super) fn descendant_overflow_contribution_for_element(
    element: &Element,
    style: &ComputedStyle,
) -> DescendantOverflowContribution {
    used_property_containment(element, style).descendant_overflow_contribution()
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn element_propagates_document_canvas_properties(
        &self,
        element: &Element,
        _style: &ComputedStyle,
    ) -> bool {
        self.element_side_effect_suppression_depth == 0
            && self
                .document_canvas_overflow
                .is_document_canvas_property_source(element)
    }

    pub(in crate::layout) fn element_uses_document_canvas_flow(&self, element: &Element) -> bool {
        self.document_canvas_overflow
            .is_document_canvas_flow_element(element)
    }

    pub(in crate::layout) fn element_supplies_document_principal_flow(
        &self,
        element: &Element,
    ) -> bool {
        self.document_canvas_overflow
            .is_document_principal_flow_source(element)
    }

    /// Returns the element's overflow after document-canvas propagation.
    pub(in crate::layout) fn used_overflow_for_element(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> css::Overflow {
        self.document_canvas_overflow.used_overflow(element, style)
    }

    /// Axis-preserving counterpart to [`Self::used_overflow_for_element`].
    pub(in crate::layout) fn used_overflow_axes_for_element(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> UsedOverflowAxes {
        self.document_canvas_overflow
            .used_overflow_axes(element, style)
    }

    /// Returns whether an element establishes a local overflow clip after
    /// document-canvas overflow propagation.
    pub(in crate::layout) fn element_used_overflow_clips(
        &self,
        element: &Element,
        style: &ComputedStyle,
    ) -> bool {
        (self
            .used_overflow_axes_for_element(element, style)
            .clips_any_axis()
            || paint_containment_applies_to_element(element, style))
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
    // Table-internal display types are consumed by their enclosing table
    // fragment.  When one is the principal box here, no table ancestor has
    // supplied that fragment, so its anonymous-table wrapper must retain the
    // normal-flow static-position behavior of the principal element rather
    // than becoming an independent table root.
    // <https://drafts.csswg.org/css-display-3/#transformations>
    if style.display.is_table()
        && !matches!(
            style.display.inner,
            DisplayInner::TableColumnGroup
                | DisplayInner::TableColumn
                | DisplayInner::TableHeaderGroup
                | DisplayInner::TableRowGroup
                | DisplayInner::TableFooterGroup
                | DisplayInner::TableRow
                | DisplayInner::TableCell
                | DisplayInner::TableCaption
        )
    {
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
        // Canvas and embedded documents have no Spindrift paint resource unless a
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
        "img" if element.image_rendering != ImageRendering::AltText => {
            Some(ReplacedElementKind::Image)
        }
        "embed" | "video" => Some(ReplacedElementKind::Image),
        // HTML's Image Button state is an image-backed replaced control. It
        // shares the ordinary raster-image layout path, while every other
        // input state retains its form-control layout semantics.
        // <https://html.spec.whatwg.org/multipage/input.html#image-button-state-(type=image)>
        "input" if css::input_type(&element.tag, &element.attrs).as_deref() == Some("image") => {
            Some(ReplacedElementKind::Image)
        }
        // HTML `<object>` renders its fallback subtree unless the resource
        // selection algorithm chose a supported external representation.
        // The static renderer resolves that state before building CSS boxes.
        // <https://html.spec.whatwg.org/multipage/iframe-embed-object.html#the-object-element>
        "object" if element.object_rendering == ObjectRendering::Image => {
            Some(ReplacedElementKind::Image)
        }
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

/// Whether this element itself has a direct `<br>` child.
///
/// A direct break is collected by this element's raw inline fallback. A nested
/// break is owned by its nested formatting context and must not turn an
/// ancestor with frozen block children into an inline-content owner.
/// <https://html.spec.whatwg.org/multipage/text-level-semantics.html#the-br-element>
pub(super) fn element_has_direct_line_break(element: &Element) -> bool {
    element.children.iter().any(
        |child| matches!(&child.kind, NodeKind::Element(child) if is_line_break_element(child)),
    )
}

/// Return whether the element's used overflow clips its own box.
///
/// Return raw style clipping for layout paths that do not have a document
/// canvas context. Context-aware principal block layout replaces this with the
/// selected viewport overflow source's used `visible` value.
pub(super) fn used_overflow_clips_element(element: &Element, style: &ComputedStyle) -> bool {
    UsedOverflowAxes::from_style(style).clips_any_axis()
        || paint_containment_applies_to_element(element, style)
}

pub(super) fn is_html_table_element(element: &Element) -> bool {
    has_html_rendering_semantics(element) && element.tag == "table"
}

pub(super) fn is_table_or_replaced_element(element: &Element) -> bool {
    is_html_table_element(element) || is_replaced_element(element)
}

pub(super) fn suppresses_ordered_mixed_flow_detection(element: &Element) -> bool {
    // These elements manage list markers or table construction; the ordered
    // mixed-flow fallback would duplicate those formatting-context-specific
    // layout paths. The selected document canvas is deliberately *not*
    // excluded: a body with direct inline content around a fragmented block
    // must preserve DOM order instead of collecting all its text before that
    // block.
    // <https://www.w3.org/TR/CSS2/visuren.html#anonymous-block-level>
    is_html_select_element(element)
        || is_html_select_item_element(element)
        || matches!(
            list_container_kind(element),
            ListContainerKind::Ordered | ListContainerKind::Unordered
        )
        || is_html_table_element(element)
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

    fn resolve_document_canvas(document: &str, author_css: &str) -> DocumentCanvasResolution {
        let root = crate::dom::parse(document);
        let author = css::parse_stylesheet(&crate::css::Css::from_string(author_css));
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&author),
        );
        let page_box = box_tree::build_page_box(&root, &stylesheets, &ComputedStyle::initial());
        DocumentCanvasResolution::from_page_box(&page_box)
    }

    #[test]
    fn containment_bits_are_the_single_body_propagation_gate() {
        for contain in [
            "size",
            "inline-size",
            "layout",
            "style",
            "paint",
            "strict",
            "content",
        ] {
            let resolution = resolve_document_canvas(
                "<html><body>content</body></html>",
                &format!("body {{ contain: {contain}; writing-mode: vertical-rl }}"),
            );
            assert_eq!(
                resolution.principal_flow().source,
                PrincipalFlowSource::Root,
                "contain: {contain}"
            );
        }

        for content_visibility in ["auto", "hidden"] {
            let resolution = resolve_document_canvas(
                "<html><body>content</body></html>",
                &format!(
                    "body {{ content-visibility: {content_visibility}; writing-mode: vertical-rl }}"
                ),
            );
            assert_eq!(
                resolution.principal_flow().source,
                PrincipalFlowSource::Root,
                "content-visibility: {content_visibility}"
            );
        }

        let hidden_body = resolve_document_canvas(
            "<html><body hidden>content</body></html>",
            "body { writing-mode: vertical-rl }",
        );
        assert_eq!(
            hidden_body.principal_flow().source,
            PrincipalFlowSource::Root
        );
    }

    #[test]
    fn active_containment_on_root_or_body_disables_body_propagation() {
        let containment_declarations = [
            "contain: size",
            "contain: inline-size",
            "contain: layout",
            "contain: paint",
            "contain: style",
            "content-visibility: auto",
            "content-visibility: hidden",
        ];

        for selector in ["html", "body"] {
            for declaration in containment_declarations {
                let resolution = resolve_document_canvas(
                    "<html><body>content</body></html>",
                    &format!("html {{ writing-mode: vertical-lr }} {selector} {{ {declaration} }}"),
                );
                assert!(
                    resolution.propagated_body.is_none(),
                    "{selector} {{ {declaration} }}"
                );
                assert_eq!(
                    resolution.principal_flow().source,
                    PrincipalFlowSource::Root,
                    "{selector} {{ {declaration} }}"
                );
            }
        }
    }

    #[test]
    fn root_principal_properties_remain_used_when_containment_blocks_body() {
        let resolution = resolve_document_canvas(
            "<html><body>content</body></html>",
            "html { writing-mode: vertical-lr; direction: rtl; text-orientation: mixed } \
             body { contain: style; writing-mode: horizontal-tb; direction: ltr; text-orientation: upright }",
        );

        assert_eq!(
            resolution.principal_flow().source,
            PrincipalFlowSource::Root
        );
        assert_eq!(
            resolution.principal_flow().writing_mode,
            WritingMode::VerticalLr
        );
        assert_eq!(resolution.principal_flow().direction, Direction::Rtl);
        assert_eq!(
            resolution.principal_flow().text_orientation,
            TextOrientation::Mixed
        );
    }

    #[test]
    fn principal_flow_source_distinguishes_root_participation_from_body_propagation() {
        let resolve_and_assert = |author_css: &str, body_propagates: bool| {
            let root = crate::dom::parse("<html><body>content</body></html>");
            let document = root
                .as_element()
                .expect("parsed document has an element root");
            let html = document
                .children
                .iter()
                .filter_map(Node::as_element)
                .find(|element| element.tag == "html")
                .expect("document has an HTML root");
            let body = html
                .children
                .iter()
                .filter_map(Node::as_element)
                .find(|element| element.tag == "body")
                .expect("HTML root has a body");
            let author = css::parse_stylesheet(&crate::css::Css::from_string(author_css));
            let stylesheets = Stylesheets::for_document(
                css::html5_user_agent_stylesheet(),
                None,
                std::slice::from_ref(&author),
            );
            let page_box = box_tree::build_page_box(&root, &stylesheets, &ComputedStyle::initial());
            let resolution = DocumentCanvasResolution::from_page_box(&page_box);

            assert!(resolution.is_document_canvas_flow_element(html));
            assert_eq!(
                resolution.is_document_canvas_flow_element(body),
                body_propagates
            );
            assert_eq!(
                resolution.is_document_principal_flow_source(html),
                !body_propagates
            );
            assert_eq!(
                resolution.is_document_principal_flow_source(body),
                body_propagates
            );
        };

        resolve_and_assert("body { writing-mode: vertical-lr }", true);
        resolve_and_assert("body { contain: layout; writing-mode: vertical-lr }", false);
    }

    #[test]
    fn containment_disables_body_overflow_and_background_fallback_together() {
        let root = crate::dom::parse("<html><body>content</body></html>");
        let author = css::parse_stylesheet(&crate::css::Css::from_string(
            "html { overflow: visible } body { overflow: hidden; background: red; contain: style }",
        ));
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&author),
        );
        let page_box = box_tree::build_page_box(&root, &stylesheets, &ComputedStyle::initial());
        let resolution = DocumentCanvasResolution::from_page_box(&page_box);
        let document = root
            .as_element()
            .expect("parsed document has an element root");
        let html = document
            .children
            .iter()
            .filter_map(Node::as_element)
            .find(|element| element.tag == "html")
            .expect("document has an HTML root");
        let body = html
            .children
            .iter()
            .filter_map(Node::as_element)
            .find(|element| element.tag == "body")
            .expect("HTML root has a body");

        assert!(resolution.is_viewport_overflow_source(html));
        assert!(!resolution.is_viewport_overflow_source(body));
        assert!(!resolution.is_body_canvas_background_fallback_source(body));
        assert!(!resolution.is_document_canvas_flow_element(body));
    }

    #[test]
    fn positioned_body_still_supplies_viewport_overflow() {
        let root = crate::dom::parse("<html><body>content</body></html>");
        let author = css::parse_stylesheet(&crate::css::Css::from_string(
            "html { overflow: visible } body { overflow: hidden; position: absolute }",
        ));
        let stylesheets = Stylesheets::for_document(
            css::html5_user_agent_stylesheet(),
            None,
            std::slice::from_ref(&author),
        );
        let page_box = box_tree::build_page_box(&root, &stylesheets, &ComputedStyle::initial());
        let resolution = DocumentCanvasResolution::from_page_box(&page_box);
        let document = root
            .as_element()
            .expect("parsed document has an element root");
        let html = document
            .children
            .iter()
            .filter_map(Node::as_element)
            .find(|element| element.tag == "html")
            .expect("document has an HTML root");
        let body = html
            .children
            .iter()
            .filter_map(Node::as_element)
            .find(|element| element.tag == "body")
            .expect("HTML root has a body");

        assert!(resolution.is_viewport_overflow_source(body));
    }

    #[test]
    fn root_overflow_is_used_by_the_viewport() {
        let html = html_element("html");
        let mut style = ComputedStyle::initial();
        style.overflow = css::Overflow::Hidden;

        assert_eq!(
            DocumentCanvasResolution {
                viewport_overflow_source: Some(ViewportOverflowSource::Root(html.id)),
                ..DocumentCanvasResolution::default()
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
            DocumentCanvasResolution {
                viewport_overflow_source: Some(ViewportOverflowSource::Body(body.id)),
                ..DocumentCanvasResolution::default()
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
            DocumentCanvasResolution::default().used_overflow(&body, &style),
            css::Overflow::Hidden
        );
    }

    #[test]
    fn canvas_background_fallback_uses_only_the_propagated_body() {
        let html = html_element("html");
        let body = html_element("body");
        let context = DocumentCanvasResolution {
            root: Some(html.id),
            propagated_body: Some(body.id),
            ..DocumentCanvasResolution::default()
        };

        assert!(context.is_root_canvas_background_source(&html));
        assert!(!context.is_body_canvas_background_fallback_source(&html));
        assert!(context.is_body_canvas_background_fallback_source(&body));
    }

    #[test]
    fn only_the_selected_body_loses_its_local_overflow_clip() {
        let selected = html_element("body");
        let other = html_element("body");
        let mut style = ComputedStyle::initial();
        style.overflow = css::Overflow::Hidden;
        let context = DocumentCanvasResolution {
            viewport_overflow_source: Some(ViewportOverflowSource::Body(selected.id)),
            ..DocumentCanvasResolution::default()
        };

        assert!(context.is_viewport_overflow_source(&selected));
        assert!(!context.is_viewport_overflow_source(&other));
        assert!(style_clips_overflow(&style));
    }

    #[test]
    fn used_containment_ignores_table_internal_principal_boxes() {
        let element = html_element("div");
        let mut style = ComputedStyle::initial();
        style.display = Display::TABLE_ROW_GROUP;
        style.contain.layout = true;
        style.contain.paint = true;

        assert_eq!(
            used_property_containment(&element, &style),
            UsedPropertyContainment {
                size: false,
                inline_size: false,
                layout: false,
                paint: false,
            }
        );
        assert!(!property_containment_establishes_independent_formatting_context(&element, &style));
        assert_eq!(
            descendant_overflow_contribution_for_element(&element, &style),
            DescendantOverflowContribution::Scrollable
        );
    }

    #[test]
    fn used_containment_applicability_matrix_keeps_only_eligible_principal_boxes() {
        let element = html_element("div");
        let excluded = [
            Display::INLINE,
            Display::TABLE_COLUMN_GROUP,
            Display::TABLE_COLUMN,
            Display::TABLE_ROW_GROUP,
            Display::TABLE_HEADER_GROUP,
            Display::TABLE_FOOTER_GROUP,
            Display::TABLE_ROW,
        ];

        for display in excluded {
            let mut style = ComputedStyle::initial();
            style.display = display;
            style.contain.size = true;
            style.contain.inline_size = true;
            style.contain.layout = true;
            style.contain.paint = true;
            assert_eq!(
                used_property_containment(&element, &style),
                UsedPropertyContainment {
                    size: false,
                    inline_size: false,
                    layout: false,
                    paint: false,
                },
                "{display:?} must not regain containment from copied styles",
            );
        }

        let mut inline_block = ComputedStyle::initial();
        inline_block.display = Display::INLINE_BLOCK;
        inline_block.contain.layout = true;
        assert!(used_property_containment(&element, &inline_block).layout);

        let mut table_cell = ComputedStyle::initial();
        table_cell.display = Display::TABLE_CELL;
        table_cell.contain.inline_size = true;
        table_cell.contain.paint = true;
        assert!(used_property_containment(&element, &table_cell).inline_size);
        assert!(used_property_containment(&element, &table_cell).paint);
    }

    #[test]
    fn used_containment_keeps_table_cell_paint_effects() {
        let element = html_element("td");
        let mut style = ComputedStyle::initial();
        style.display = Display::TABLE_CELL;
        style.contain.paint = true;

        assert!(paint_containment_applies_to_element(&element, &style));
        assert!(property_containment_establishes_independent_formatting_context(&element, &style));
        assert_eq!(
            descendant_overflow_contribution_for_element(&element, &style),
            DescendantOverflowContribution::InkOnly
        );
    }

    #[test]
    fn body_propagation_checks_all_active_containment() {
        let mut style = ComputedStyle::initial();
        assert!(!style_has_active_containment(&style));

        style.contain.inline_size = true;
        assert!(style_has_active_containment(&style));
        style.contain.inline_size = false;

        style.contain.style = true;
        assert!(style_has_active_containment(&style));
        style.contain.style = false;

        for content_visibility in [ContentVisibility::Auto, ContentVisibility::Hidden] {
            style.content_visibility = content_visibility;
            assert!(style_has_active_containment(&style));
        }
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
    fn scrollport_geometry_keeps_none_gutters_zero() {
        let mut style = ComputedStyle::initial();
        style.overflow_x = css::Overflow::Auto;
        style.overflow_y = css::Overflow::Auto;
        style.scrollbar_gutter = css::ScrollbarGutter::Stable { both_edges: true };
        style.scrollbar_width = css::ScrollbarWidth::None;

        let geometry = ScrollportGeometry::for_padding_box(
            PaintClip::new(0.0, 0.0, 100.0, 80.0),
            &style,
            UsedOverflowAxes::from_style(&style),
            true,
            true,
        );
        assert_eq!(geometry.padding_box, geometry.scrollport);
        assert_eq!(geometry.gutters.left.points(), 0.0);
        assert_eq!(geometry.gutters.right.points(), 0.0);
        assert_eq!(geometry.gutters.top.points(), 0.0);
        assert_eq!(geometry.gutters.bottom.points(), 0.0);
    }

    #[test]
    fn scrollbar_reservation_preserves_forced_thin_and_both_edge_geometry() {
        let mut style = ComputedStyle::initial();
        style.overflow_x = css::Overflow::Scroll;
        style.overflow_y = css::Overflow::Scroll;
        style.scrollbar_width = css::ScrollbarWidth::Thin;
        style.scrollbar_gutter = css::ScrollbarGutter::Stable { both_edges: true };

        let reservation = ScrollbarGutterReservation::for_style(
            &style,
            UsedOverflowAxes::from_style(&style),
            false,
            false,
        );
        let thickness = 8.0 * css::CSS_PX_TO_PT;
        assert_eq!(reservation.gutters().left.points(), thickness);
        assert_eq!(reservation.gutters().right.points(), thickness);
        assert_eq!(reservation.gutters().top.points(), thickness);
        assert_eq!(reservation.gutters().bottom.points(), thickness);
        assert_eq!(reservation.horizontal_extent().points(), thickness * 2.0);
        assert_eq!(reservation.vertical_extent().points(), thickness * 2.0);
    }

    #[test]
    fn xhtml_namespace_uses_html_rendering_semantics() {
        let mut html = html_element("html");
        html.document_syntax = DocumentSyntax::Xml;
        html.namespace_url = XHTML_NAMESPACE_URL.to_string();
        let mut image = html_element("img");
        image.document_syntax = DocumentSyntax::Xml;
        image.namespace_url = XHTML_NAMESPACE_URL.to_string();

        assert!(has_html_rendering_semantics(&html));
        assert_eq!(
            replaced_element_kind(&image),
            Some(ReplacedElementKind::Image)
        );
    }

    #[test]
    fn only_html_image_buttons_are_replaced_images() {
        let mut image_button = html_element("input");
        image_button
            .attrs
            .insert("type".to_string(), "IMAGE".to_string());
        assert_eq!(
            replaced_element_kind(&image_button),
            Some(ReplacedElementKind::Image)
        );

        for input_type in [None, Some("text"), Some("not-a-real-state")] {
            let mut input = html_element("input");
            if let Some(input_type) = input_type {
                input
                    .attrs
                    .insert("type".to_string(), input_type.to_string());
            }
            assert_eq!(replaced_element_kind(&input), None, "{input_type:?}");
        }
    }

    #[test]
    fn unnamespaced_xml_elements_do_not_acquire_html_rendering_semantics() {
        let mut image = html_element("img");
        image.document_syntax = DocumentSyntax::Xml;

        assert_eq!(replaced_element_kind(&image), None);
    }
}
