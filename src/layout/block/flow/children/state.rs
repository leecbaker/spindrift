use crate::css::{ClampContinuation, ComputedStyle, UsedLineClamp};
use crate::document::paint::geometry::{PaintPoint, PaintRect, PaintSize};
use crate::layout::{
    BlockEndMarginCollapse, LayoutLength, style_establishes_multicol_formatting_context,
};

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct ChildFlowTraversalOutcome {
    pub(in crate::layout) pending_end_margin_collapse: Option<BlockEndMarginCollapse>,
    pub(in crate::layout) collapsed_start_margin_offset: LayoutLength,
    /// Static geometry exported by the rendered-legend source child, when
    /// this traversal owns an HTML fieldset.  It is deliberately kept out of
    /// the ordinary block cursor state: the legend's border interruption is a
    /// parent decoration concern, not an ink-bound-based flow adjustment.
    pub(in crate::layout) rendered_legend: Option<RenderedLegendGeometry>,
}

/// Untransformed source-fragment geometry required by HTML's rendered-legend
/// border rule.
///
/// The margin rectangle is retained separately because HTML excludes the
/// fieldset border behind the legend's *margin box*, while the legend itself
/// paints its border box in normal CSS paint order.
///
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
/// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct RenderedLegendGeometry {
    pub(in crate::layout) border_box: PaintRect,
    pub(in crate::layout) margin_box: PaintRect,
    pub(in crate::layout) source_fragmentainer: usize,
}

impl RenderedLegendGeometry {
    /// Derive the static margin rectangle from an already resolved physical
    /// border rectangle.  CSS logical margins have been mapped to these four
    /// physical used edges at this boundary.
    pub(in crate::layout) fn from_static_border_box(
        border_box: PaintRect,
        margins: crate::css::Edges,
        source_fragmentainer: usize,
    ) -> Self {
        let margin_box = PaintRect::new(
            PaintPoint::new(
                border_box.origin.x - margins.left,
                border_box.origin.y - margins.bottom,
            ),
            PaintSize::new(
                border_box.size.width + margins.left + margins.right,
                border_box.size.height + margins.top + margins.bottom,
            ),
        );
        Self {
            border_box,
            margin_box,
            source_fragmentainer,
        }
    }

    /// Derive the fieldset-border-only visible regions around this legend's
    /// static margin rectangle.  This is a physical paint-boundary type: the
    /// CSS logical sides have already been mapped by the resolved layout
    /// geometry above.
    ///
    /// <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
    /// <https://www.w3.org/TR/css-writing-modes-4/#logical-to-physical>
    pub(in crate::layout) fn border_exclusion(
        self,
        fieldset_border_box: PaintRect,
        source_fragmentainer: usize,
    ) -> Option<RenderedLegendBorderExclusion> {
        if self.source_fragmentainer != source_fragmentainer {
            return None;
        }
        let hole = self.margin_box.intersection(&fieldset_border_box)?;
        // An auto-sized ordinary block spans the fieldset's entire inline
        // axis. Do not turn that unresolved intermediate geometry into a
        // whole-border erasure: the HTML fit-content override must establish
        // the rendered legend's finite inline span before border exclusion is
        // meaningful.
        if hole.size.width >= fieldset_border_box.size.width - 0.01
            || hole.size.height >= fieldset_border_box.size.height - 0.01
        {
            return None;
        }
        let mut visible_regions = Vec::with_capacity(4);
        let push = |regions: &mut Vec<PaintRect>, x, y, width, height| {
            if width > 0.0 && height > 0.0 {
                regions.push(PaintRect::new(
                    PaintPoint::new(x, y),
                    PaintSize::new(width, height),
                ));
            }
        };
        push(
            &mut visible_regions,
            fieldset_border_box.min_x(),
            fieldset_border_box.min_y(),
            hole.min_x() - fieldset_border_box.min_x(),
            fieldset_border_box.size.height,
        );
        push(
            &mut visible_regions,
            hole.max_x(),
            fieldset_border_box.min_y(),
            fieldset_border_box.max_x() - hole.max_x(),
            fieldset_border_box.size.height,
        );
        push(
            &mut visible_regions,
            hole.min_x(),
            fieldset_border_box.min_y(),
            hole.size.width,
            hole.min_y() - fieldset_border_box.min_y(),
        );
        push(
            &mut visible_regions,
            hole.min_x(),
            hole.max_y(),
            hole.size.width,
            fieldset_border_box.max_y() - hole.max_y(),
        );
        (!visible_regions.is_empty()).then_some(RenderedLegendBorderExclusion { visible_regions })
    }
}

/// Physical border-paint regions left after subtracting a rendered legend's
/// static margin rectangle.  This applies to border primitives only; fieldset
/// backgrounds retain their ordinary CSS paint area.
///
/// <https://html.spec.whatwg.org/multipage/rendering.html#the-fieldset-and-legend-elements>
#[derive(Debug, Clone, PartialEq)]
pub(in crate::layout) struct RenderedLegendBorderExclusion {
    pub(in crate::layout) visible_regions: Vec<PaintRect>,
}

/// The line-clamp budget shared by one block-flow child traversal.
///
/// The source clamp remains immutable so each child receives a style with the
/// currently available line slots, while replay restores the exact remaining
/// budget captured at the class-A break candidate.
pub(in crate::layout) struct BlockFlowChildTraversalState {
    source: Option<UsedLineClamp>,
    remaining: Option<usize>,
    descendant_clamp_line_slots: usize,
}

impl BlockFlowChildTraversalState {
    /// Convert source-order reachability into the terminal clamp signal.
    /// Keeping the conversion here prevents traversal sites from treating an
    /// out-of-flow effect as overflow by accident.
    pub(in crate::layout) fn continuation_for_later_in_flow_source(
        has_later_in_flow_source: bool,
    ) -> ClampContinuation {
        if has_later_in_flow_source {
            ClampContinuation::LaterInFlowContent
        } else {
            ClampContinuation::None
        }
    }

    pub(in crate::layout) fn new(style: &ComputedStyle) -> Self {
        // `continue: collapse`, which `line-clamp` sets, behaves as `auto`
        // on multicol containers. Do not create a budget that descendants
        // could accidentally spend while the multicol implementation lays
        // its independent formatting contexts.
        // <https://drafts.csswg.org/css-overflow-4/#continue>
        let source = (!style_establishes_multicol_formatting_context(style))
            .then(|| style.line_clamp.as_ref().map(UsedLineClamp::from_computed))
            .flatten();
        Self {
            remaining: source.as_ref().map(|clamp| clamp.max_lines),
            source,
            descendant_clamp_line_slots: 0,
        }
    }

    pub(in crate::layout) fn is_exhausted(&self) -> bool {
        self.remaining == Some(0)
    }

    /// The number of source line slots that may still be committed by this
    /// traversal.  A source-order side effect can use this to decide whether
    /// a speculative inline run reached the clamp boundary before committing
    /// its own paint or geometry.
    pub(in crate::layout) fn remaining_line_slots(&self) -> Option<usize> {
        self.remaining
    }

    /// Whether this traversal is spending an ancestor's clamp budget.
    ///
    /// A child's own `line-clamp` must not treat its later siblings as source
    /// that continues *inside* that child. Only a clamp propagated by this
    /// traversal can cross a child boundary.
    #[cfg(test)]
    pub(in crate::layout) fn has_active_clamp(&self) -> bool {
        self.remaining.is_some()
    }

    /// Capture the clamp portion of an avoid-break candidate explicitly.
    ///
    /// Builder snapshots restore paint and fragmentainer state, but the
    /// traversal's budget is deliberately local to the source traversal.
    pub(in crate::layout) fn capture_avoid_replay(&self) -> Option<usize> {
        self.remaining
    }

    /// Restore the budget associated with an avoid-break candidate before its
    /// source children are replayed.
    pub(in crate::layout) fn restore_avoid_replay(&mut self, remaining: Option<usize>) {
        self.remaining = remaining;
    }

    pub(in crate::layout) fn debit(&mut self, slots: usize) {
        if let Some(remaining) = &mut self.remaining {
            *remaining = remaining.saturating_sub(slots);
        }
    }

    /// Propagate the remaining budget together with a known later in-flow
    /// continuation. The inline selector consumes this only at the terminal
    /// line, so an intermediate child cannot create a marker.
    pub(in crate::layout) fn apply_to_with_continuation(
        &self,
        style: &mut ComputedStyle,
        continuation: ClampContinuation,
    ) {
        if let (Some(limit), Some(clamp)) = (self.remaining, self.source.as_ref()) {
            style.used_line_clamp =
                Some(clamp.with_remaining(limit).with_continuation(continuation));
        }
    }

    #[cfg(test)]
    pub(in crate::layout) fn apply_to(&self, style: &mut ComputedStyle) {
        self.apply_to_with_continuation(style, ClampContinuation::None);
    }

    pub(in crate::layout) fn style_with_remaining(
        &self,
        style: &ComputedStyle,
    ) -> Option<ComputedStyle> {
        self.style_with_remaining_and_continuation(style, ClampContinuation::None)
    }

    /// Clone a layout style for an inline run while preserving the shared
    /// budget and a known later in-flow continuation.
    pub(in crate::layout) fn style_with_remaining_and_continuation(
        &self,
        style: &ComputedStyle,
        continuation: ClampContinuation,
    ) -> Option<ComputedStyle> {
        self.remaining.map(|_| {
            let mut style = style.clone();
            self.apply_to_with_continuation(&mut style, continuation);
            style
        })
    }

    pub(in crate::layout) fn record_descendant_clamp_line_slots(&mut self, slots: usize) {
        self.descendant_clamp_line_slots += slots;
    }

    pub(in crate::layout) fn descendant_clamp_line_slots(&self) -> usize {
        self.descendant_clamp_line_slots
    }
}
