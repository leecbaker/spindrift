use super::*;
use crate::layout::block::ParentStartClearanceHypothesis;

/// Collapse two adjoining signed layout margins.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn collapse_margins(
    first: LayoutLength,
    second: LayoutLength,
) -> LayoutLength {
    let first = first.points();
    let second = second.points();
    layout_pt(if first >= 0.0 && second >= 0.0 {
        first.max(second)
    } else if first <= 0.0 && second <= 0.0 {
        first.min(second)
    } else {
        first + second
    })
}

/// Collapses an adjoining set of vertical margins.
///
/// CSS 2.2 collapses an adjoining margin set to the maximum positive margin
/// plus the minimum negative margin. This differs from pairwise collapsing
/// when a set contains more than two mixed-sign margins.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visudet.html#min-max-heights>
pub(in crate::layout) fn collapse_margin_set(
    margins: impl IntoIterator<Item = LayoutLength>,
) -> LayoutLength {
    let mut set = AdjoiningMarginSet::new();
    for margin in margins {
        set.include(margin);
    }
    set.collapsed()
}

/// A complete adjoining set of CSS block-axis margins.
///
/// CSS 2.2 collapses an arbitrary adjoining set by combining its greatest
/// positive margin with its most-negative margin. Retaining those extrema,
/// rather than repeatedly collapsing scalar pairs, preserves the result when
/// transparent descendants and self-collapsing siblings contribute later
/// margins to the same set.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct AdjoiningMarginSet {
    greatest_positive: LayoutLength,
    most_negative: LayoutLength,
}

impl AdjoiningMarginSet {
    pub(in crate::layout) fn new() -> Self {
        Self {
            greatest_positive: layout_pt(0.0),
            most_negative: layout_pt(0.0),
        }
    }

    pub(in crate::layout) fn from_margin(margin: LayoutLength) -> Self {
        let mut set = Self::new();
        set.include(margin);
        set
    }

    pub(in crate::layout) fn include(&mut self, margin: LayoutLength) {
        if margin > self.greatest_positive {
            self.greatest_positive = margin;
        }
        if margin < self.most_negative {
            self.most_negative = margin;
        }
    }

    pub(in crate::layout) fn merge(&mut self, other: Self) {
        self.include(other.greatest_positive);
        self.include(other.most_negative);
    }

    pub(in crate::layout) fn collapsed(self) -> LayoutLength {
        self.greatest_positive + self.most_negative
    }
}

pub(in crate::layout) fn page_start_margin(
    margin: LayoutLength,
    starts_at_page_top: bool,
) -> LayoutLength {
    if starts_at_page_top && margin.points() > 0.0 {
        layout_pt(0.0)
    } else {
        margin
    }
}

pub(in crate::layout) fn collapsed_start_margin_delta(
    previous_applied: LayoutLength,
    next: LayoutLength,
    starts_at_page_top: bool,
) -> LayoutLength {
    let collapsed = collapse_margins(previous_applied, next);
    layout_pt(page_start_margin(collapsed, starts_at_page_top).points() - previous_applied.points())
}

pub(in crate::layout) fn collapsed_margin_delta(
    previous_applied: LayoutLength,
    next: LayoutLength,
) -> LayoutLength {
    layout_pt(collapse_margins(previous_applied, next).points() - previous_applied.points())
}

/// The collapsed block-start margin set owned by one in-flow child.
///
/// An auto-height block and its first in-flow child's adjoining block-start
/// margins form one set. The child therefore carries only the additional local
/// margin needed after its parent or preceding sibling has already consumed a
/// portion of that set; it must not subtract its first descendant's margin a
/// second time.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visuren.html#block-formatting>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct AdjoiningBlockStartMargin {
    collapsed: LayoutLength,
    descendant_deferred_to_child: LayoutLength,
}

/// The complete start-margin set a parent has provisionally adjoined with its
/// first in-flow child.  A cleared child also needs the parent border edge as
/// its `clear:none` counterfactual position.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
/// <https://www.w3.org/TR/CSS22/visuren.html#flow-control>
#[derive(Clone, Copy, Debug)]
pub(in crate::layout) struct InheritedAdjoiningStartMargin {
    complete_margin: LayoutLength,
    parent_start_clearance_hypothesis: ParentStartClearanceHypothesis,
}

/// Lexically scoped adjoining start-margin state owned by one layout element.
///
/// A propagated parent/first-child margin remains available while the target
/// element decides whether its own first in-flow child continues the adjoining
/// chain. It must not be observed by later sibling subtrees laid out before the
/// target element returns.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
#[derive(Debug, Default)]
pub(in crate::layout) struct InheritedAdjoiningStartMarginScopes {
    scopes: Vec<InheritedAdjoiningStartMarginScope>,
}

#[derive(Clone, Copy, Debug)]
struct InheritedAdjoiningStartMarginScope {
    owner: ElementId,
    margin: InheritedAdjoiningStartMargin,
}

impl InheritedAdjoiningStartMarginScopes {
    pub(in crate::layout) fn push_for(
        &mut self,
        owner: ElementId,
        margin: InheritedAdjoiningStartMargin,
    ) {
        self.scopes
            .push(InheritedAdjoiningStartMarginScope { owner, margin });
    }

    pub(in crate::layout) fn current_for(
        &self,
        owner: ElementId,
    ) -> Option<InheritedAdjoiningStartMargin> {
        self.scopes
            .last()
            .filter(|scope| scope.owner == owner)
            .map(|scope| scope.margin)
    }

    pub(in crate::layout) fn pop_for(&mut self, owner: ElementId) {
        let popped = self.scopes.pop();
        debug_assert_eq!(
            popped.map(|scope| scope.owner),
            Some(owner),
            "adjoining start-margin scopes must be popped by their owning element"
        );
    }
}

impl InheritedAdjoiningStartMargin {
    /// Preserve the earliest parent edge in one transparent adjoining chain.
    /// Replacing it at each wrapper would make a deeply nested cleared box
    /// query a border edge already shifted by part of the same margin set.
    pub(in crate::layout) fn with_parent_start_hypothesis(
        complete_margin: LayoutLength,
        parent_start_clearance_hypothesis: ParentStartClearanceHypothesis,
    ) -> Self {
        Self {
            complete_margin,
            parent_start_clearance_hypothesis,
        }
    }

    pub(in crate::layout) fn complete_margin(self) -> LayoutLength {
        self.complete_margin
    }

    pub(in crate::layout) fn parent_start_clearance_hypothesis(
        self,
    ) -> ParentStartClearanceHypothesis {
        self.parent_start_clearance_hypothesis
    }
}

impl AdjoiningBlockStartMargin {
    /// Construct the margin set from a child's own margin and its adjoining
    /// first descendant's margin.
    ///
    /// CSS 2.2 collapses all adjoining positive margins to their maximum;
    /// negative margins follow the corresponding signed rule. The resulting
    /// value belongs to the whole adjoining set, not separately to each box.
    ///
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn from_child_and_descendant(
        child_margin: LayoutLength,
        descendant_margin: Option<LayoutLength>,
    ) -> Self {
        let collapsed = descendant_margin
            .map(|descendant| collapse_margins(child_margin, descendant))
            .unwrap_or(child_margin);
        Self {
            collapsed,
            // When the descendant is itself the collapsed result, leave that
            // contribution to the child's own start-margin pass. The sibling
            // delta must then cancel the preceding margin without consuming
            // the descendant twice. Mixed-sign margin sets remain whole at
            // this boundary because neither individual margin represents the
            // collapsed result.
            descendant_deferred_to_child: descendant_margin
                .filter(|descendant| *descendant == collapsed)
                .unwrap_or_else(|| layout_pt(0.0)),
        }
    }

    /// Construct an already-collapsed set for a self-collapsing child.
    ///
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn from_collapsed(collapsed: LayoutLength) -> Self {
        Self {
            collapsed,
            descendant_deferred_to_child: layout_pt(0.0),
        }
    }

    /// Return the collapsed value for bookkeeping that continues the same
    /// adjoining margin set through later transparent boxes.
    pub(in crate::layout) fn value(self) -> LayoutLength {
        self.collapsed
    }

    /// Return the child-local delta after the parent has consumed its
    /// block-start margin set.
    ///
    /// The parent collapses directly with the complete adjoining set, so
    /// page-start trimming applies before the local delta is computed.
    ///
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn child_delta_at_parent_start(
        self,
        parent_applied: LayoutLength,
        starts_at_page_top: bool,
    ) -> LayoutLength {
        collapsed_start_margin_delta(parent_applied, self.collapsed, starts_at_page_top)
    }

    /// Return the child-local delta after a preceding sibling's adjoining
    /// block-end margin. When a first descendant itself supplies the
    /// collapsed value, defer that part to the child's own layout pass.
    ///
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn child_delta_after_sibling(
        self,
        previous_sibling_margin: LayoutLength,
    ) -> LayoutLength {
        layout_pt(
            collapsed_margin_delta(previous_sibling_margin, self.collapsed).points()
                - self.descendant_deferred_to_child.points(),
        )
    }
}

/// Applies CSS Box Model Level 4 `margin-trim: block-start` to the first
/// in-flow child adjoining a block container's block-start edge.
///
/// The margin to trim is the collapsed adjoining margin set at the parent's
/// block-start edge, not just the child's authored `margin-top`. Cancelling
/// that collapsed contribution is observable when CSS 2.2 block-in-inline
/// splitting exposes a self-collapsing block as the first in-flow child.
///
/// <https://drafts.csswg.org/css-box-4/#margin-trim>
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn trim_adjoining_block_start_margin(
    parent_style: &ComputedStyle,
    child_style: &mut ComputedStyle,
    is_first_flow_child: bool,
    descendant_start_margin: Option<f32>,
) -> bool {
    if !parent_style.margin_trim.block_start || !is_first_flow_child {
        return false;
    }
    let adjoining_start_margin = descendant_start_margin
        .map(|descendant| {
            collapse_margins(layout_pt(child_style.margin.top), layout_pt(descendant)).points()
        })
        .unwrap_or(child_style.margin.top);
    child_style.margin.top -= adjoining_start_margin;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjoining_margin_set_preserves_extrema_across_merges() {
        let mut outer = AdjoiningMarginSet::from_margin(layout_pt(-20.0));
        outer.include(layout_pt(40.0));
        let inner = AdjoiningMarginSet::from_margin(layout_pt(-30.0));
        outer.merge(inner);

        assert_eq!(outer.collapsed(), layout_pt(10.0));
    }

    fn inherited_margin() -> InheritedAdjoiningStartMargin {
        InheritedAdjoiningStartMargin::with_parent_start_hypothesis(
            layout_pt(12.0),
            ParentStartClearanceHypothesis::new(PageTopBlockPosition::new(40.0)),
        )
    }

    #[test]
    fn inherited_adjoining_margin_scope_is_visible_only_to_its_owner() {
        let owner = ElementId::next();
        let sibling = ElementId::next();
        let mut scopes = InheritedAdjoiningStartMarginScopes::default();
        scopes.push_for(owner, inherited_margin());

        assert_eq!(
            scopes
                .current_for(owner)
                .map(|margin| margin.complete_margin()),
            Some(layout_pt(12.0))
        );
        assert!(scopes.current_for(sibling).is_none());

        scopes.pop_for(owner);
        assert!(scopes.current_for(owner).is_none());
    }

    #[test]
    #[should_panic(
        expected = "adjoining start-margin scopes must be popped by their owning element"
    )]
    fn inherited_adjoining_margin_scope_rejects_mismatched_pop() {
        let owner = ElementId::next();
        let sibling = ElementId::next();
        let mut scopes = InheritedAdjoiningStartMarginScopes::default();
        scopes.push_for(owner, inherited_margin());

        scopes.pop_for(sibling);
    }
}
