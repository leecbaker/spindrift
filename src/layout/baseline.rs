//! Typed physical baseline coordinates shared by layout modes.
//!
//! A baseline is a signed coordinate from a box edge, rather than an extent.
//! Keeping its physical axis and origin in the type prevents a horizontal
//! baseline from being used as a vertical line-box offset without an explicit
//! writing-mode projection.
//!
//! <https://drafts.csswg.org/css-align-3/#baseline-alignment>
//! <https://drafts.csswg.org/css-flexbox/#flex-baselines>

use crate::css::PhysicalSide;
use crate::units::{LayoutLength, SemanticLengthExt};

/// Marker for a baseline coordinate measured from a box's physical top edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct PhysicalTopBaselineAxis;

/// Marker for a baseline coordinate measured from a box's physical left edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) struct PhysicalLeftBaselineAxis;

/// A signed baseline coordinate from a physical border-box origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::layout) struct PhysicalBaselineOffset<Axis>(
    LayoutLength,
    std::marker::PhantomData<Axis>,
);

impl<Axis> PhysicalBaselineOffset<Axis> {
    pub(in crate::layout) const fn new(value: LayoutLength) -> Self {
        Self(value, std::marker::PhantomData)
    }

    pub(in crate::layout) const fn into_layout_length(self) -> LayoutLength {
        self.0
    }

    /// Scalar extraction is reserved for legacy layout/Taffy adapters.
    pub(in crate::layout) fn points(self) -> f32 {
        self.0.points()
    }

    pub(in crate::layout) fn offset_by(self, distance: LayoutLength) -> Self {
        Self::new(self.0 + distance)
    }

    pub(in crate::layout) fn half(self) -> Self {
        Self::new(self.0 * 0.5)
    }
}

pub(in crate::layout) type PhysicalTopBaselineOffset =
    PhysicalBaselineOffset<PhysicalTopBaselineAxis>;
pub(in crate::layout) type PhysicalLeftBaselineOffset =
    PhysicalBaselineOffset<PhysicalLeftBaselineAxis>;

/// First and last baselines on one physical axis.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct BaselinePair<Axis> {
    pub(in crate::layout) first: Option<PhysicalBaselineOffset<Axis>>,
    pub(in crate::layout) last: Option<PhysicalBaselineOffset<Axis>>,
}

impl<Axis> Default for BaselinePair<Axis> {
    fn default() -> Self {
        Self {
            first: None,
            last: None,
        }
    }
}

/// Final baseline sets measured from an atomic inline's physical border-box
/// top and left edges. The parent inline formatting context performs the one
/// logical-axis projection when it places the atom in a line.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::layout) struct PhysicalBaselineSets {
    pub(in crate::layout) vertical: BaselinePair<PhysicalTopBaselineAxis>,
    pub(in crate::layout) horizontal: BaselinePair<PhysicalLeftBaselineAxis>,
}

impl PhysicalBaselineSets {
    /// Project the first compatible physical baseline into the containing
    /// inline formatting context's logical block-start coordinate.
    ///
    /// `border_box_block_size` is measured in the containing context's
    /// logical block axis. The caller must supply the atom's border-box size,
    /// excluding its margins.
    pub(in crate::layout) fn first_from_logical_block_start(
        self,
        block_start: PhysicalSide,
        border_box_block_size: LayoutLength,
    ) -> Option<LayoutLength> {
        match block_start {
            PhysicalSide::Top => self
                .vertical
                .first
                .map(PhysicalTopBaselineOffset::into_layout_length),
            PhysicalSide::Bottom => self
                .vertical
                .first
                .map(|baseline| border_box_block_size - baseline.into_layout_length()),
            PhysicalSide::Left => self
                .horizontal
                .first
                .map(PhysicalLeftBaselineOffset::into_layout_length),
            PhysicalSide::Right => self
                .horizontal
                .first
                .map(|baseline| border_box_block_size - baseline.into_layout_length()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::layout_pt;

    fn sets() -> PhysicalBaselineSets {
        PhysicalBaselineSets {
            vertical: BaselinePair {
                first: Some(PhysicalTopBaselineOffset::new(layout_pt(7.0))),
                last: None,
            },
            horizontal: BaselinePair {
                first: Some(PhysicalLeftBaselineOffset::new(layout_pt(11.0))),
                last: None,
            },
        }
    }

    #[test]
    fn physical_baselines_project_from_each_logical_block_start_edge() {
        let sets = sets();
        let span = layout_pt(40.0);
        assert_eq!(
            sets.first_from_logical_block_start(PhysicalSide::Top, span),
            Some(layout_pt(7.0))
        );
        assert_eq!(
            sets.first_from_logical_block_start(PhysicalSide::Left, span),
            Some(layout_pt(11.0))
        );
        assert_eq!(
            sets.first_from_logical_block_start(PhysicalSide::Right, span),
            Some(layout_pt(29.0))
        );
        assert_eq!(
            sets.first_from_logical_block_start(PhysicalSide::Bottom, span),
            Some(layout_pt(33.0))
        );
    }
}
