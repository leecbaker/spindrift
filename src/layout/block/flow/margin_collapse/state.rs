use super::*;
use crate::layout::block::ParentStartClearanceHypothesis;

/// Collapse two adjoining signed layout margins.
///
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
pub(in crate::layout) fn collapse_margins(
    first: LayoutLength,
    second: LayoutLength,
) -> LayoutLength {
    let zero = layout_pt(0.0);
    if first >= zero && second >= zero {
        if first >= second { first } else { second }
    } else if first <= zero && second <= zero {
        if first <= second { first } else { second }
    } else {
        first + second
    }
}

/// A complete adjoining set of CSS block-axis margins.
///
/// CSS 2.2 collapses an arbitrary adjoining set by combining its greatest
/// positive margin with its most-negative margin. Retaining those extrema,
/// rather than repeatedly collapsing scalar pairs, preserves the result when
/// transparent descendants and self-collapsing siblings contribute later
/// margins to the same set.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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

    pub(in crate::layout) fn merged(mut self, other: Self) -> Self {
        self.merge(other);
        self
    }

    pub(in crate::layout) fn with_margin(mut self, margin: LayoutLength) -> Self {
        self.include(margin);
        self
    }
}

/// A complete adjoining sibling-margin set and the part already applied to
/// the block-flow cursor.
///
/// Block layout consumes margins eagerly, while CSS requires every margin
/// adjoining through a self-collapsing block to be resolved as one set. Keep
/// the extrema independently from the cursor contribution so a later margin
/// can revise the collapsed result without losing mixed-sign provenance.
/// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::layout) struct PendingAdjoiningMargin {
    set: AdjoiningMarginSet,
    applied: LayoutLength,
}

impl PendingAdjoiningMargin {
    pub(in crate::layout) fn from_consumed_margin(margin: LayoutLength) -> Self {
        Self {
            set: AdjoiningMarginSet::from_margin(margin),
            applied: margin,
        }
    }

    pub(in crate::layout) fn from_consumed_set(set: AdjoiningMarginSet) -> Self {
        Self {
            set,
            applied: set.collapsed(),
        }
    }

    /// Start a pending set at a transparent parent's block-start edge.
    ///
    /// The returned delta is local to the parent's content traversal, while
    /// `applied` includes the margin already consumed when positioning the
    /// parent itself. Keeping both views prevents an only self-collapsing
    /// child from applying its start margin again at the parent's block-end.
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn from_parent_start_set(
        parent_applied: LayoutLength,
        child_set: AdjoiningMarginSet,
        starts_at_page_top: bool,
    ) -> (Self, LayoutLength) {
        let set = child_set.with_margin(parent_applied);
        let applied = page_start_margin(set.collapsed(), starts_at_page_top);
        (Self { set, applied }, applied - parent_applied)
    }

    pub(in crate::layout) fn applied(self) -> LayoutLength {
        self.applied
    }

    /// Add a margin that is already represented by the current cursor
    /// position, as happens when a parent start margin joins a transparent
    /// first-child chain.
    pub(in crate::layout) fn with_consumed_margin(mut self, margin: LayoutLength) -> Self {
        self.set.include(margin);
        self.applied = self.set.collapsed();
        self
    }

    /// Merge another adjoining set and return only the additional cursor
    /// contribution required by the new complete collapsed result.
    pub(in crate::layout) fn merge_set(&mut self, next: AdjoiningMarginSet) -> LayoutLength {
        self.set.merge(next);
        let collapsed = self.set.collapsed();
        let delta = collapsed - self.applied;
        self.applied = collapsed;
        delta
    }

    pub(in crate::layout) fn merge_margin(&mut self, next: LayoutLength) -> LayoutLength {
        self.merge_set(AdjoiningMarginSet::from_margin(next))
    }

    pub(in crate::layout) fn collapsed_with_margin(self, next: LayoutLength) -> LayoutLength {
        self.set.with_margin(next).collapsed()
    }
}

pub(in crate::layout) fn page_start_margin(
    margin: LayoutLength,
    starts_at_page_top: bool,
) -> LayoutLength {
    if starts_at_page_top && margin > layout_pt(0.0) {
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
    page_start_margin(collapsed, starts_at_page_top) - previous_applied
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
    set: AdjoiningMarginSet,
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
        descendant_margin: Option<AdjoiningMarginSet>,
    ) -> Self {
        let set = descendant_margin
            .map(|descendant| AdjoiningMarginSet::from_margin(child_margin).merged(descendant))
            .unwrap_or_else(|| AdjoiningMarginSet::from_margin(child_margin));
        let collapsed = set.collapsed();
        Self {
            set,
            // When the descendant is itself the collapsed result, leave that
            // contribution to the child's own start-margin pass. The sibling
            // delta must then cancel the preceding margin without consuming
            // the descendant twice. Mixed-sign margin sets remain whole at
            // this boundary because neither individual margin represents the
            // collapsed result.
            descendant_deferred_to_child: descendant_margin
                .map(AdjoiningMarginSet::collapsed)
                .filter(|descendant| *descendant == collapsed)
                .unwrap_or_default(),
        }
    }

    /// Construct an already-collapsed set for a self-collapsing child.
    ///
    /// <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
    pub(in crate::layout) fn from_set(set: AdjoiningMarginSet) -> Self {
        Self {
            set,
            descendant_deferred_to_child: layout_pt(0.0),
        }
    }

    /// Return the collapsed value for bookkeeping that continues the same
    /// adjoining margin set through later transparent boxes.
    pub(in crate::layout) fn value(self) -> LayoutLength {
        self.set.collapsed()
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
        let collapsed = self.set.with_margin(parent_applied).collapsed();
        page_start_margin(collapsed, starts_at_page_top) - parent_applied
    }

    pub(in crate::layout) fn child_delta_after_pending_sibling(
        self,
        mut previous: PendingAdjoiningMargin,
    ) -> LayoutLength {
        previous.merge_set(self.set) - self.descendant_deferred_to_child
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
    descendant_start_margin: Option<AdjoiningMarginSet>,
) -> bool {
    if !parent_style.margin_trim.block_start || !is_first_flow_child {
        return false;
    }
    let adjoining_start_margin = descendant_start_margin
        .map(|descendant| {
            AdjoiningMarginSet::from_margin(layout_pt(child_style.margin.top))
                .merged(descendant)
                .collapsed()
        })
        .unwrap_or_else(|| layout_pt(child_style.margin.top));
    child_style.margin.top -= adjoining_start_margin.points();
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

    #[test]
    fn pending_adjoining_margin_applies_only_changes_to_the_complete_set() {
        let mut pending = PendingAdjoiningMargin::from_consumed_margin(layout_pt(-5.5));
        let empty = AdjoiningMarginSet::from_margin(layout_pt(8.25)).with_margin(layout_pt(-5.5));

        assert_eq!(pending.merge_set(empty), layout_pt(8.25));
        assert_eq!(pending.applied(), layout_pt(2.75));
        assert_eq!(pending.merge_set(empty), layout_pt(0.0));
        assert_eq!(pending.merge_margin(layout_pt(8.25)), layout_pt(0.0));
        assert_eq!(pending.applied(), layout_pt(2.75));

        let mut positive = PendingAdjoiningMargin::from_consumed_margin(layout_pt(4.0));
        assert_eq!(positive.merge_margin(layout_pt(8.0)), layout_pt(4.0));
        assert_eq!(positive.applied(), layout_pt(8.0));

        let mut negative = PendingAdjoiningMargin::from_consumed_margin(layout_pt(-3.0));
        assert_eq!(negative.merge_margin(layout_pt(-9.0)), layout_pt(-6.0));
        assert_eq!(negative.applied(), layout_pt(-9.0));

        let mut zero = PendingAdjoiningMargin::from_consumed_margin(layout_pt(0.0));
        assert_eq!(zero.merge_margin(layout_pt(0.0)), layout_pt(0.0));
        assert_eq!(zero.applied(), layout_pt(0.0));

        let parent_transparent_child =
            AdjoiningMarginSet::from_margin(layout_pt(30.0)).with_margin(layout_pt(40.0));
        let (pending, local_delta) = PendingAdjoiningMargin::from_parent_start_set(
            layout_pt(40.0),
            parent_transparent_child,
            false,
        );
        assert_eq!(local_delta, layout_pt(0.0));
        assert_eq!(pending.applied(), layout_pt(40.0));
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
