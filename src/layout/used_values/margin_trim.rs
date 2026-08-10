use super::*;
/// A container-derived set of child margins whose used values are zero.
///
/// `margin-trim` is resolved by the parent formatting context, after it knows
/// which children adjoin its logical edges.  The plan deliberately records
/// physical sides only at that boundary; applying it changes both forms of a
/// child's margin so later sizing and replay cannot observe different values.
/// <https://drafts.csswg.org/css-box-4/#margin-trim>.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::layout) struct MarginTrimPlan {
    sides: Vec<TrimmedPhysicalMargins>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TrimmedPhysicalMargins {
    top: bool,
    right: bool,
    bottom: bool,
    left: bool,
}

impl MarginTrimPlan {
    pub(in crate::layout) fn for_item_count(item_count: usize) -> Self {
        Self {
            sides: vec![TrimmedPhysicalMargins::default(); item_count],
        }
    }

    pub(in crate::layout) fn trim(&mut self, item_index: usize, side: PhysicalSide) {
        let Some(sides) = self.sides.get_mut(item_index) else {
            return;
        };
        match side {
            PhysicalSide::Top => sides.top = true,
            PhysicalSide::Right => sides.right = true,
            PhysicalSide::Bottom => sides.bottom = true,
            PhysicalSide::Left => sides.left = true,
        }
    }

    pub(in crate::layout) fn apply_to_style(&self, item_index: usize, style: &mut ComputedStyle) {
        let Some(sides) = self.sides.get(item_index) else {
            return;
        };
        if sides.top {
            trim_used_item_margin_side(style, PhysicalSide::Top);
        }
        if sides.right {
            trim_used_item_margin_side(style, PhysicalSide::Right);
        }
        if sides.bottom {
            trim_used_item_margin_side(style, PhysicalSide::Bottom);
        }
        if sides.left {
            trim_used_item_margin_side(style, PhysicalSide::Left);
        }
    }

    pub(in crate::layout) fn is_empty(&self) -> bool {
        self.sides
            .iter()
            .all(|sides| !sides.top && !sides.right && !sides.bottom && !sides.left)
    }
}

/// Set one physical item margin to its specified zero used value.
///
/// Both the eagerly resolved edge and the computed source value must change:
/// CSS Flexbox and CSS Grid consume the former in their post-layout passes and
/// the latter at their Taffy sizing boundary.
/// <https://drafts.csswg.org/css-box-4/#margin-trim>.
pub(in crate::layout) fn trim_used_item_margin_side(style: &mut ComputedStyle, side: PhysicalSide) {
    let zero = css::ComputedLengthPercentageOrAuto::ZERO;
    match side {
        PhysicalSide::Top => {
            style.margin.top = 0.0;
            style.box_values.margin.top = zero;
        }
        PhysicalSide::Right => {
            style.margin.right = 0.0;
            style.box_values.margin.right = zero;
        }
        PhysicalSide::Bottom => {
            style.margin.bottom = 0.0;
            style.box_values.margin.bottom = zero;
        }
        PhysicalSide::Left => {
            style.margin.left = 0.0;
            style.box_values.margin.left = zero;
        }
    }
}

#[cfg(test)]
mod margin_trim_tests {
    use super::*;

    #[test]
    fn margin_trim_plan_zeros_resolved_and_computed_margin_values() {
        let mut style = ComputedStyle::initial();
        style.margin.top = 12.0;
        style.margin.left = 8.0;
        style.box_values.margin.top = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(12.0),
        );
        style.box_values.margin.left = css::ComputedLengthPercentageOrAuto::LengthPercentage(
            css::ComputedLengthPercentage::from_points(8.0),
        );
        let mut plan = MarginTrimPlan::for_item_count(1);
        plan.trim(0, PhysicalSide::Top);
        plan.trim(0, PhysicalSide::Left);

        plan.apply_to_style(0, &mut style);

        assert_eq!(style.margin.top, 0.0);
        assert_eq!(style.margin.left, 0.0);
        assert_eq!(
            style.box_values.margin.top,
            css::ComputedLengthPercentageOrAuto::ZERO
        );
        assert_eq!(
            style.box_values.margin.left,
            css::ComputedLengthPercentageOrAuto::ZERO
        );
    }
}
