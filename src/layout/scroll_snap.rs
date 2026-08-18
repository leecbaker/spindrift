//! Static CSS Scroll Snap geometry and candidate selection.
//!
//! CSS Scroll Snap deliberately leaves user-input physics to the user agent.
//! Quire has no interactive session, so this module provides the deterministic
//! used-value policy used when painting a document's initial static view.
//! <https://www.w3.org/TR/css-scroll-snap-1/#scroll-snap-model>

use super::*;
use crate::dom::ElementId;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct ScrollOffset {
    pub(in crate::layout) x: f32,
    pub(in crate::layout) y: f32,
}

impl ScrollOffset {
    pub(in crate::layout) const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

/// Translate captured paint for a positive logical scroll offset. Paint-space
/// coordinates increase rightward and upward, while CSS logical scroll starts
/// can be left/right or top/bottom with writing mode and direction.
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
pub(in crate::layout) fn static_scroll_translation(
    offset: ScrollOffset,
    style: &ComputedStyle,
) -> PaintTranslation {
    let logical = WritingModeAxes::new(style.writing_mode, style.used_direction());
    let horizontal_side =
        logical.physical_start_side(logical.logical_axis_for_physical(PhysicalAxis::Horizontal));
    let vertical_side =
        logical.physical_start_side(logical.logical_axis_for_physical(PhysicalAxis::Vertical));
    PaintTranslation::new(
        offset.x * scroll_translation_sign(PhysicalAxis::Horizontal, horizontal_side),
        offset.y * scroll_translation_sign(PhysicalAxis::Vertical, vertical_side),
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct Scrollport {
    pub(in crate::layout) rect: PaintRect,
    /// The finite two-axis range measured from each logical scroll-start edge.
    max_offset: ScrollOffset,
}

/// One resolved physical scroll axis expressed from the container's logical
/// scroll-start side. Source geometry remains in physical paint coordinates;
/// only candidate arithmetic is widened and normalized here.
/// <https://www.w3.org/TR/css-scroll-snap-1/#scroll-snap-model>
/// <https://drafts.csswg.org/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone, Copy)]
struct ResolvedScrollAxis {
    physical_axis: PhysicalAxis,
    logical_start: PhysicalSide,
    port_start: f64,
    port_end: f64,
    max_offset: f64,
}

impl ResolvedScrollAxis {
    fn for_scrollport(
        port: Scrollport,
        style: &ComputedStyle,
        physical_axis: PhysicalAxis,
    ) -> Self {
        let logical = WritingModeAxes::new(style.writing_mode, style.used_direction());
        let logical_start =
            logical.physical_start_side(logical.logical_axis_for_physical(physical_axis));
        let (port_start, port_end, max_offset) = match physical_axis {
            PhysicalAxis::Horizontal => (
                port.rect.origin.x as f64,
                port.rect.max_x() as f64,
                port.max_offset.x as f64,
            ),
            PhysicalAxis::Vertical => (
                port.rect.origin.y as f64,
                port.rect.max_y() as f64,
                port.max_offset.y as f64,
            ),
        };
        Self {
            physical_axis,
            logical_start,
            port_start,
            port_end,
            max_offset: max_offset.max(0.0),
        }
    }

    fn scroll_translation_sign(self) -> f64 {
        scroll_translation_sign(self.physical_axis, self.logical_start) as f64
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct ScrollSnapCandidate {
    /// Provisional normal-flow layout can visit an eventually positioned box.
    /// The final positioned record replaces that provisional geometry.
    source_element: ElementId,
    pub(in crate::layout) border_box: PaintRect,
    pub(in crate::layout) style: ComputedStyle,
}

#[derive(Debug, Clone)]
pub(in crate::layout) struct ActiveScrollSnapScope {
    pub(in crate::layout) page_index: usize,
    is_document_root: bool,
    pub(in crate::layout) style: ComputedStyle,
    pub(in crate::layout) overflow_bounds: Option<PaintRect>,
    pub(in crate::layout) target_area: Option<(PaintRect, ComputedStyle)>,
    pub(in crate::layout) candidates: Vec<ScrollSnapCandidate>,
}

impl<'a> LayoutBuilder<'a> {
    pub(in crate::layout) fn begin_static_scroll_snap_scope(
        &mut self,
        style: &ComputedStyle,
        is_document_root: bool,
    ) -> bool {
        if style.scroll_snap_type == css::ScrollSnapType::None
            && !style.overflow_x.is_scrollable()
            && !style.overflow_y.is_scrollable()
            && !is_document_root
        {
            return false;
        }
        self.active_scroll_snap_scopes.push(ActiveScrollSnapScope {
            page_index: self.pages.len(),
            is_document_root,
            style: style.clone(),
            overflow_bounds: None,
            target_area: None,
            candidates: Vec::new(),
        });
        true
    }

    pub(in crate::layout) fn record_static_scroll_snap_area(
        &mut self,
        element: &Element,
        style: &ComputedStyle,
        border_box: PaintRect,
    ) {
        let page_index = self.pages.len();
        // Scroll snap areas belong to the nearest scroll container on the
        // containing-block chain. Recording a descendant in every enclosing
        // scope lets an outer container snap to an area captured by an inner
        // scroller, contrary to CSS Scroll Snap's containment rule.
        // <https://www.w3.org/TR/css-scroll-snap-1/#scroll-snap-model>
        if let Some(scope) = self
            .active_scroll_snap_scopes
            .iter_mut()
            .rev()
            .find(|scope| scope.page_index == page_index)
        {
            scope.overflow_bounds = Some(
                scope
                    .overflow_bounds
                    .map_or(border_box, |bounds| union_paint_rects(bounds, border_box)),
            );
            if style.scroll_snap_align != css::ScrollSnapAlign::default() {
                if let Some(candidate) = scope
                    .candidates
                    .iter_mut()
                    .find(|candidate| candidate.source_element == element.id)
                {
                    candidate.border_box = border_box;
                    candidate.style = style.clone();
                } else {
                    scope.candidates.push(ScrollSnapCandidate {
                        source_element: element.id,
                        border_box,
                        style: style.clone(),
                    });
                }
            }
        }
    }

    /// Record the fragment-navigation target for the innermost scroll
    /// container. HTML fragment navigation scrolls each target into its
    /// nearest scrollable ancestor before CSS Scroll Snap chooses a final
    /// snapped position.
    /// <https://html.spec.whatwg.org/multipage/browsing-the-web.html#scroll-to-fragid>
    pub(in crate::layout) fn record_static_scroll_target_area(
        &mut self,
        is_target: bool,
        border_box: PaintRect,
        style: &ComputedStyle,
    ) {
        if !is_target {
            return;
        }
        let page_index = self.pages.len();
        if let Some(scope) = self
            .active_scroll_snap_scopes
            .iter_mut()
            .rev()
            .find(|scope| scope.page_index == page_index)
        {
            scope.target_area = Some((border_box, style.clone()));
        }
    }

    pub(in crate::layout) fn finish_static_scroll_snap_scope(
        &mut self,
        active: bool,
        padding_box: PaintRect,
        content_bounds: PaintRect,
    ) -> ScrollOffset {
        if !active {
            return ScrollOffset::ZERO;
        }
        let scope = self
            .active_scroll_snap_scopes
            .pop()
            .expect("active scroll snap scope");
        let content_bounds = scope.overflow_bounds.map_or(content_bounds, |bounds| {
            union_paint_rects(content_bounds, bounds)
        });
        let padding_box = if scope.is_document_root {
            self.iframe_viewport.map_or(padding_box, |context| {
                let viewport = context.viewport;
                // An iframe has one finite scrolling viewport even though the
                // embedded static document is laid out on an unfragmented
                // canvas. Anchor that viewport at the root's block-start edge.
                paint_space_rect(
                    padding_box.origin.x,
                    padding_box.max_y() - viewport.height(),
                    viewport.width(),
                    viewport.height(),
                )
            })
        } else {
            padding_box
        };
        let port = Scrollport::from_padding_box(padding_box, content_bounds, &scope.style);
        let initial = scope
            .target_area
            .map(|(target, target_style)| {
                port.target_offset(
                    snap_area_for(target, &target_style),
                    &scope.style,
                    &target_style,
                )
            })
            .unwrap_or(ScrollOffset::ZERO);

        let offset = port.initial_offset(initial, &scope.style, &scope.candidates);
        if scope.is_document_root && self.iframe_viewport.is_some() {
            // A child browsing context materializes its propagated page canvas
            // after normal root contents have been captured. Retain the same
            // resolved root translation so that canvas paint cannot bypass
            // fragment navigation or the iframe viewport clip.
            // <https://www.w3.org/TR/css-scroll-snap-1/#scroll-snap-model>
            let translation = static_scroll_translation(offset, &scope.style);
            self.document_canvas_scroll_translation =
                PaintTranslation::new(-translation.x, -translation.y);
        }
        offset
    }
}

impl Scrollport {
    /// Construct the snapport by reducing a padding-box scrollport using the
    /// specified scroll-padding offsets. `auto` has Quire's static used value
    /// of zero; percentages resolve against the corresponding scrollport axis.
    pub(in crate::layout) fn from_padding_box(
        padding_box: PaintRect,
        content_bounds: PaintRect,
        style: &ComputedStyle,
    ) -> Self {
        let used = used_scroll_padding(style, padding_box);
        let width = (padding_box.size.width - used.left - used.right).max(0.0);
        let height = (padding_box.size.height - used.top - used.bottom).max(0.0);
        let rect = paint_space_rect(
            padding_box.origin.x + used.left,
            padding_box.origin.y + used.bottom,
            width,
            height,
        );
        let logical = WritingModeAxes::new(style.writing_mode, style.used_direction());
        let horizontal_start = logical
            .physical_start_side(logical.logical_axis_for_physical(PhysicalAxis::Horizontal));
        let vertical_start =
            logical.physical_start_side(logical.logical_axis_for_physical(PhysicalAxis::Vertical));
        let max_offset = ScrollOffset {
            x: match horizontal_start {
                PhysicalSide::Left => {
                    finite_nonnegative(content_bounds.max_x() - padding_box.max_x())
                }
                PhysicalSide::Right => {
                    finite_nonnegative(padding_box.origin.x - content_bounds.origin.x)
                }
                _ => unreachable!("horizontal logical start must be left or right"),
            },
            y: match vertical_start {
                PhysicalSide::Top => {
                    finite_nonnegative(padding_box.origin.y - content_bounds.origin.y)
                }
                PhysicalSide::Bottom => {
                    finite_nonnegative(content_bounds.max_y() - padding_box.max_y())
                }
                _ => unreachable!("vertical logical start must be top or bottom"),
            },
        };
        Self { rect, max_offset }
    }

    /// Select Quire's initial static offset. Mandatory containers choose the
    /// closest clamped candidate to the initial scroll position. Proximity containers only
    /// choose candidates already intersecting the initial snapport; this makes
    /// an initial static render stable without inventing interaction physics.
    pub(in crate::layout) fn initial_offset(
        self,
        initial: ScrollOffset,
        container_style: &ComputedStyle,
        candidates: &[ScrollSnapCandidate],
    ) -> ScrollOffset {
        let strictness = match container_style.scroll_snap_type {
            css::ScrollSnapType::None => return initial,
            css::ScrollSnapType::X(strictness)
            | css::ScrollSnapType::Y(strictness)
            | css::ScrollSnapType::Block(strictness)
            | css::ScrollSnapType::Inline(strictness)
            | css::ScrollSnapType::Both(strictness) => strictness,
        };
        let axes = snap_axes(container_style);
        let horizontal =
            ResolvedScrollAxis::for_scrollport(self, container_style, PhysicalAxis::Horizontal);
        let vertical =
            ResolvedScrollAxis::for_scrollport(self, container_style, PhysicalAxis::Vertical);
        let mut best_x = None;
        let mut best_y = None;
        for candidate in candidates {
            let area = snap_area(candidate);
            if strictness == css::ScrollSnapStrictness::Proximity {
                let intersects_snapport =
                    area.intersection(&self.rect).is_some_and(|intersection| {
                        intersection.size.width > 0.0 && intersection.size.height > 0.0
                    });
                if !intersects_snapport {
                    continue;
                }
            }
            if axes.x
                && let Some(offset) = axis_snap_offset_resolved(
                    horizontal,
                    area.origin.x,
                    area.max_x(),
                    candidate.style.scroll_snap_align,
                    container_style,
                    initial.x,
                )
            {
                best_x = closest_offset(best_x, offset, initial.x);
            }
            if axes.y
                && let Some(offset) = axis_snap_offset_resolved(
                    vertical,
                    area.origin.y,
                    area.max_y(),
                    candidate.style.scroll_snap_align,
                    container_style,
                    initial.y,
                )
            {
                best_y = closest_offset(best_y, offset, initial.y);
            }
        }
        ScrollOffset {
            x: best_x.unwrap_or(initial.x),
            y: best_y.unwrap_or(initial.y),
        }
    }

    /// Return the nearest clamped offset that makes a fragment target visible
    /// in the snapport. This is the static equivalent of HTML's target
    /// scrolling step; a following mandatory/proximity pass may adjust it.
    fn target_offset(
        self,
        target: PaintRect,
        container_style: &ComputedStyle,
        target_style: &ComputedStyle,
    ) -> ScrollOffset {
        let logical = WritingModeAxes::new(
            container_style.writing_mode,
            container_style.used_direction(),
        );
        let axis = |target_start: f32,
                    target_end: f32,
                    port_start: f32,
                    port_end: f32,
                    max: f32,
                    axis: PhysicalAxis| {
            let start_side = logical.physical_start_side(logical.logical_axis_for_physical(axis));
            let start_aligned = (side_coordinate(start_side, port_start, port_end)
                - side_coordinate(start_side, target_start, target_end))
                / scroll_translation_sign(axis, start_side);
            let start_aligned = start_aligned.clamp(0.0, max);
            axis_snap_offset(
                port_start,
                port_end,
                target_start,
                target_end,
                target_style.scroll_snap_align,
                container_style,
                axis,
                max,
                start_aligned,
            )
            .unwrap_or(start_aligned)
        };
        ScrollOffset {
            x: axis(
                target.origin.x,
                target.max_x(),
                self.rect.origin.x,
                self.rect.max_x(),
                self.max_offset.x,
                PhysicalAxis::Horizontal,
            ),
            y: axis(
                target.origin.y,
                target.max_y(),
                self.rect.origin.y,
                self.rect.max_y(),
                self.max_offset.y,
                PhysicalAxis::Vertical,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PhysicalSnapAxes {
    x: bool,
    y: bool,
}

fn snap_axes(style: &ComputedStyle) -> PhysicalSnapAxes {
    let logical = WritingModeAxes::new(style.writing_mode, style.used_direction());
    match style.scroll_snap_type {
        css::ScrollSnapType::X(_) => PhysicalSnapAxes { x: true, y: false },
        css::ScrollSnapType::Y(_) => PhysicalSnapAxes { x: false, y: true },
        css::ScrollSnapType::Both(_) => PhysicalSnapAxes { x: true, y: true },
        css::ScrollSnapType::Block(_) => match logical.physical_axis(LogicalAxis::Block) {
            PhysicalAxis::Horizontal => PhysicalSnapAxes { x: true, y: false },
            PhysicalAxis::Vertical => PhysicalSnapAxes { x: false, y: true },
        },
        css::ScrollSnapType::Inline(_) => match logical.physical_axis(LogicalAxis::Inline) {
            PhysicalAxis::Horizontal => PhysicalSnapAxes { x: true, y: false },
            PhysicalAxis::Vertical => PhysicalSnapAxes { x: false, y: true },
        },
        css::ScrollSnapType::None => PhysicalSnapAxes { x: false, y: false },
    }
}

fn used_scroll_padding(style: &ComputedStyle, scrollport: PaintRect) -> css::Edges {
    let used = |value: &css::ScrollPadding, basis: f32| match value {
        css::ScrollPadding::Auto => 0.0,
        css::ScrollPadding::LengthPercentage(value) => value
            .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(basis)))
            .unwrap_or_else(|| value.fixed_component())
            .points()
            .max(0.0),
    };
    css::Edges {
        top: used(&style.scroll_padding.top, scrollport.size.height),
        right: used(&style.scroll_padding.right, scrollport.size.width),
        bottom: used(&style.scroll_padding.bottom, scrollport.size.height),
        left: used(&style.scroll_padding.left, scrollport.size.width),
    }
}

fn snap_area(candidate: &ScrollSnapCandidate) -> PaintRect {
    snap_area_for(candidate.border_box, &candidate.style)
}

fn snap_area_for(border_box: PaintRect, style: &ComputedStyle) -> PaintRect {
    let margin = &style.scroll_margin;
    let top = margin.top.fixed_component().points();
    let right = margin.right.fixed_component().points();
    let bottom = margin.bottom.fixed_component().points();
    let left = margin.left.fixed_component().points();
    paint_space_rect(
        finite(border_box.origin.x - left),
        finite(border_box.origin.y - bottom),
        finite_nonnegative(border_box.size.width + left + right),
        finite_nonnegative(border_box.size.height + top + bottom),
    )
}

#[allow(clippy::too_many_arguments)]
fn axis_snap_offset(
    port_start: f32,
    port_end: f32,
    area_start: f32,
    area_end: f32,
    align: css::ScrollSnapAlign,
    container_style: &ComputedStyle,
    axis: PhysicalAxis,
    max_offset: f32,
    initial_offset: f32,
) -> Option<f32> {
    axis_snap_offset_resolved(
        ResolvedScrollAxis {
            physical_axis: axis,
            logical_start: WritingModeAxes::new(
                container_style.writing_mode,
                container_style.used_direction(),
            )
            .physical_start_side(
                WritingModeAxes::new(
                    container_style.writing_mode,
                    container_style.used_direction(),
                )
                .logical_axis_for_physical(axis),
            ),
            port_start: port_start as f64,
            port_end: port_end as f64,
            max_offset: max_offset.max(0.0) as f64,
        },
        area_start,
        area_end,
        align,
        container_style,
        initial_offset,
    )
}

fn axis_snap_offset_resolved(
    axis: ResolvedScrollAxis,
    area_start: f32,
    area_end: f32,
    align: css::ScrollSnapAlign,
    container_style: &ComputedStyle,
    initial_offset: f32,
) -> Option<f32> {
    let logical = WritingModeAxes::new(
        container_style.writing_mode,
        container_style.used_direction(),
    );
    let logical_axis = logical.logical_axis_for_physical(axis.physical_axis);
    let alignment = match logical_axis {
        LogicalAxis::Block => align.block,
        LogicalAxis::Inline => align.inline,
    };
    if alignment == css::ScrollSnapAlignment::None {
        return None;
    }
    let start_side = axis.logical_start;
    let sign = axis.scroll_translation_sign();
    let port_start = axis.port_start;
    let port_end = axis.port_end;
    let area_start = area_start as f64;
    let area_end = area_end as f64;
    let max_offset = axis.max_offset;
    let initial_offset = initial_offset as f64;

    // A snap area larger than the snapport has a range of valid positions:
    // every position where it completely covers the snapport. Choose the
    // member of that range closest to the current static offset. This is the
    // scroll-snap position definition for oversized snap areas.
    // <https://www.w3.org/TR/css-scroll-snap-1/#snap-positions>
    if area_end - area_start > port_end - port_start {
        let offset_start = (port_end - area_end) / sign;
        let offset_end = (port_start - area_start) / sign;
        let lower = offset_start.min(offset_end).max(0.0);
        let upper = offset_start.max(offset_end).min(max_offset);
        if lower.is_finite() && upper.is_finite() && lower <= upper {
            return finite_f64_to_f32(initial_offset.clamp(lower, upper));
        }
    }
    let desired = match alignment {
        css::ScrollSnapAlignment::None => return None,
        css::ScrollSnapAlignment::Center => {
            ((port_start + port_end - area_start - area_end) * 0.5) / sign
        }
        css::ScrollSnapAlignment::Start => {
            (side_coordinate_f64(start_side, port_start, port_end)
                - side_coordinate_f64(start_side, area_start, area_end))
                / sign
        }
        css::ScrollSnapAlignment::End => {
            let end_side = opposite_physical_side(start_side);
            (side_coordinate_f64(end_side, port_start, port_end)
                - side_coordinate_f64(end_side, area_start, area_end))
                / sign
        }
    };
    // A snap position outside the scrollable overflow range is unreachable.
    // Do not coerce it to an endpoint: that would let extreme scroll-margin
    // values turn a target that cannot be aligned into an arbitrary jump to
    // the end of the scroll range.
    // <https://www.w3.org/TR/css-scroll-snap-1/#snap-positions>
    if !desired.is_finite() || desired < 0.0 || desired > max_offset {
        return None;
    }
    finite_f64_to_f32(desired)
}

fn finite_f64_to_f32(value: f64) -> Option<f32> {
    value
        .is_finite()
        .then_some(value as f32)
        .filter(|value| value.is_finite())
}

fn scroll_translation_sign(axis: PhysicalAxis, logical_start: PhysicalSide) -> f32 {
    match (axis, logical_start) {
        (PhysicalAxis::Horizontal, PhysicalSide::Left) => -1.0,
        (PhysicalAxis::Horizontal, PhysicalSide::Right) => 1.0,
        (PhysicalAxis::Vertical, PhysicalSide::Top) => 1.0,
        (PhysicalAxis::Vertical, PhysicalSide::Bottom) => -1.0,
        _ => unreachable!("logical side must lie on the physical axis"),
    }
}

fn side_coordinate(side: PhysicalSide, start: f32, end: f32) -> f32 {
    match side {
        PhysicalSide::Left | PhysicalSide::Bottom => start,
        PhysicalSide::Right | PhysicalSide::Top => end,
    }
}

fn side_coordinate_f64(side: PhysicalSide, start: f64, end: f64) -> f64 {
    match side {
        PhysicalSide::Left | PhysicalSide::Bottom => start,
        PhysicalSide::Right | PhysicalSide::Top => end,
    }
}

fn opposite_physical_side(side: PhysicalSide) -> PhysicalSide {
    match side {
        PhysicalSide::Top => PhysicalSide::Bottom,
        PhysicalSide::Right => PhysicalSide::Left,
        PhysicalSide::Bottom => PhysicalSide::Top,
        PhysicalSide::Left => PhysicalSide::Right,
    }
}

fn closest_offset(current: Option<f32>, candidate: f32, initial: f32) -> Option<f32> {
    if !candidate.is_finite() {
        return current;
    }
    match current {
        Some(current) if (current - initial).abs() <= (candidate - initial).abs() => Some(current),
        _ => Some(candidate),
    }
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn finite_nonnegative(value: f32) -> f32 {
    finite(value).max(0.0)
}

fn union_paint_rects(left: PaintRect, right: PaintRect) -> PaintRect {
    let min_x = finite(left.origin.x.min(right.origin.x));
    let min_y = finite(left.origin.y.min(right.origin.y));
    let max_x = finite(left.max_x().max(right.max_x()));
    let max_y = finite(left.max_y().max(right.max_y()));
    paint_space_rect(
        min_x,
        min_y,
        finite_nonnegative(max_x - min_x),
        finite_nonnegative(max_y - min_y),
    )
}

#[cfg(test)]
mod tests {
    use crate::dom::ElementId;

    use super::*;

    fn style() -> ComputedStyle {
        let mut style = ComputedStyle::initial();
        style.scroll_snap_type = css::ScrollSnapType::Y(css::ScrollSnapStrictness::Mandatory);
        style
    }

    #[test]
    fn mandatory_initial_snap_selects_the_closest_clamped_candidate() {
        let mut target = style();
        target.scroll_snap_align.block = css::ScrollSnapAlignment::Start;
        let port = Scrollport {
            rect: paint_space_rect(0.0, 0.0, 100.0, 100.0),
            max_offset: ScrollOffset { x: 0.0, y: 500.0 },
        };
        let offset = port.initial_offset(
            ScrollOffset::ZERO,
            &style(),
            &[ScrollSnapCandidate {
                source_element: ElementId::next(),
                // A lower CSS block position has a smaller paint-space Y.
                border_box: paint_space_rect(0.0, -60.0, 10.0, 10.0),
                style: target,
            }],
        );
        assert_eq!(offset, ScrollOffset { x: 0.0, y: 150.0 });
    }

    #[test]
    fn proximity_initial_snap_ignores_offscreen_candidates() {
        let mut container = style();
        container.scroll_snap_type = css::ScrollSnapType::Y(css::ScrollSnapStrictness::Proximity);
        let mut target = style();
        target.scroll_snap_align.block = css::ScrollSnapAlignment::Start;
        let port = Scrollport {
            rect: paint_space_rect(0.0, 0.0, 100.0, 100.0),
            max_offset: ScrollOffset { x: 0.0, y: 500.0 },
        };
        assert_eq!(
            port.initial_offset(
                ScrollOffset::ZERO,
                &container,
                &[ScrollSnapCandidate {
                    source_element: ElementId::next(),
                    border_box: paint_space_rect(0.0, 110.0, 10.0, 10.0),
                    style: target,
                }],
            ),
            ScrollOffset::ZERO
        );
    }

    #[test]
    fn proximity_initial_snap_ignores_candidates_touching_the_snapport_edge() {
        let mut container = style();
        container.scroll_snap_type = css::ScrollSnapType::Y(css::ScrollSnapStrictness::Proximity);
        let mut target = style();
        target.scroll_snap_align.block = css::ScrollSnapAlignment::Start;
        let port = Scrollport {
            rect: paint_space_rect(0.0, 0.0, 100.0, 100.0),
            max_offset: ScrollOffset { x: 0.0, y: 500.0 },
        };
        assert_eq!(
            port.initial_offset(
                ScrollOffset::ZERO,
                &container,
                &[ScrollSnapCandidate {
                    source_element: ElementId::next(),
                    border_box: paint_space_rect(0.0, 100.0, 10.0, 10.0),
                    style: target,
                }],
            ),
            ScrollOffset::ZERO
        );
    }

    #[test]
    fn target_navigation_is_preserved_without_scroll_snapping() {
        let port = Scrollport {
            rect: paint_space_rect(0.0, 0.0, 100.0, 100.0),
            max_offset: ScrollOffset { x: 0.0, y: 500.0 },
        };
        let mut container = style();
        container.scroll_snap_type = css::ScrollSnapType::None;
        assert_eq!(
            port.initial_offset(ScrollOffset { x: 0.0, y: 175.0 }, &container, &[],),
            ScrollOffset { x: 0.0, y: 175.0 }
        );
    }

    #[test]
    fn unreachable_snap_position_is_not_coerced_to_scroll_range_endpoint() {
        let mut target = style();
        target.scroll_snap_align.block = css::ScrollSnapAlignment::Start;
        assert_eq!(
            axis_snap_offset(
                0.0,
                100.0,
                -f32::MAX,
                -f32::MAX,
                target.scroll_snap_align,
                &style(),
                PhysicalAxis::Vertical,
                1_000.0,
                0.0,
            ),
            None
        );
    }

    #[test]
    fn oversized_snap_area_uses_the_nearest_covering_position() {
        let mut target = style();
        target.scroll_snap_align.block = css::ScrollSnapAlignment::Start;
        assert_eq!(
            axis_snap_offset(
                0.0,
                100.0,
                -250.0,
                50.0,
                target.scroll_snap_align,
                &style(),
                PhysicalAxis::Vertical,
                500.0,
                0.0,
            ),
            Some(50.0)
        );
        assert_eq!(
            axis_snap_offset(
                0.0,
                100.0,
                -250.0,
                50.0,
                target.scroll_snap_align,
                &style(),
                PhysicalAxis::Vertical,
                500.0,
                175.0,
            ),
            Some(175.0)
        );
    }

    #[test]
    fn overflow_range_measures_from_logical_scroll_start() {
        let padding_box = paint_space_rect(100.0, 100.0, 100.0, 100.0);
        let content = paint_space_rect(-50.0, 50.0, 300.0, 250.0);

        let ltr = Scrollport::from_padding_box(padding_box, content, &style());
        assert_eq!(ltr.max_offset, ScrollOffset { x: 50.0, y: 50.0 });

        let mut vertical_rl = style();
        vertical_rl.writing_mode = WritingMode::VerticalRl;
        let vertical_rl = Scrollport::from_padding_box(padding_box, content, &vertical_rl);
        assert_eq!(vertical_rl.max_offset, ScrollOffset { x: 150.0, y: 50.0 });
    }

    #[test]
    fn mandatory_snap_selection_is_relative_to_fragment_navigation() {
        let mut target = style();
        target.scroll_snap_align.block = css::ScrollSnapAlignment::Start;
        let port = Scrollport {
            rect: paint_space_rect(0.0, 0.0, 100.0, 100.0),
            max_offset: ScrollOffset { x: 0.0, y: 500.0 },
        };
        let offset = port.initial_offset(
            ScrollOffset { x: 0.0, y: 360.0 },
            &style(),
            &[
                ScrollSnapCandidate {
                    source_element: ElementId::next(),
                    border_box: paint_space_rect(0.0, -60.0, 10.0, 10.0),
                    style: target.clone(),
                },
                ScrollSnapCandidate {
                    source_element: ElementId::next(),
                    border_box: paint_space_rect(0.0, -310.0, 10.0, 10.0),
                    style: target,
                },
            ],
        );
        assert_eq!(offset, ScrollOffset { x: 0.0, y: 400.0 });
    }
}
