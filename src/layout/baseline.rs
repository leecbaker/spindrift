//! Typed physical baseline coordinates shared by layout modes.
//!
//! A baseline is a signed coordinate from a box edge, rather than an extent.
//! Keeping its physical axis and origin in the type prevents a horizontal
//! baseline from being used as a vertical line-box offset without an explicit
//! writing-mode projection.
//!
//! <https://drafts.csswg.org/css-align-3/#baseline-alignment>
//! <https://drafts.csswg.org/css-flexbox/#flex-baselines>

use crate::css::{BaselineMetric, PhysicalSide};
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
/// top and left edges.
///
/// Each coordinate is the named baseline recorded by its corresponding
/// `*_metric`.  A formatting context that generates a set from one alignment
/// baseline must retain that name until it has applied the box's font baseline
/// table.  Coordinates alone are insufficient in vertical typographic modes,
/// where central and alphabetic baselines do not generally coincide:
/// <https://drafts.csswg.org/css-align-3/#baseline-alignment>.
#[derive(Debug, Clone, Copy)]
pub(in crate::layout) struct PhysicalBaselineSets {
    pub(in crate::layout) vertical: BaselinePair<PhysicalTopBaselineAxis>,
    pub(in crate::layout) horizontal: BaselinePair<PhysicalLeftBaselineAxis>,
    pub(in crate::layout) vertical_metric: BaselineMetric,
    pub(in crate::layout) horizontal_metric: BaselineMetric,
}

impl Default for PhysicalBaselineSets {
    fn default() -> Self {
        Self {
            vertical: BaselinePair::default(),
            horizontal: BaselinePair::default(),
            // Existing non-Flex exporters record the CSS alphabetic line
            // coordinate. Require new named-baseline exporters to opt in
            // explicitly rather than silently changing those call sites.
            vertical_metric: BaselineMetric::Alphabetic,
            horizontal_metric: BaselineMetric::Alphabetic,
        }
    }
}

impl PhysicalBaselineSets {
    /// Build a baseline set from a coordinate measured along one logical
    /// block axis.
    ///
    /// Atomic formatting contexts naturally export a coordinate from their
    /// own logical block-start edge.  Store that coordinate on the matching
    /// physical axis immediately so a perpendicular parent cannot later
    /// reinterpret it as a baseline on its own block axis.
    /// <https://drafts.csswg.org/css-align-3/#baseline-export>
    pub(in crate::layout) fn with_first_from_logical_block_start(
        mut self,
        block_start: PhysicalSide,
        border_box_block_size: LayoutLength,
        offset_from_block_start: LayoutLength,
        metric: BaselineMetric,
    ) -> Self {
        match block_start {
            PhysicalSide::Top => {
                self.vertical.first = Some(PhysicalTopBaselineOffset::new(offset_from_block_start));
                self.vertical_metric = metric;
            }
            PhysicalSide::Bottom => {
                self.vertical.first = Some(PhysicalTopBaselineOffset::new(
                    border_box_block_size - offset_from_block_start,
                ));
                self.vertical_metric = metric;
            }
            PhysicalSide::Left => {
                self.horizontal.first =
                    Some(PhysicalLeftBaselineOffset::new(offset_from_block_start));
                self.horizontal_metric = metric;
            }
            PhysicalSide::Right => {
                self.horizontal.first = Some(PhysicalLeftBaselineOffset::new(
                    border_box_block_size - offset_from_block_start,
                ));
                self.horizontal_metric = metric;
            }
        }
        self
    }

    /// Project the first compatible physical baseline and retain the metric
    /// that names its coordinate.
    ///
    /// A baseline set is not necessarily alphabetic.  The enclosing inline
    /// alignment context selects the requested member of the set later, when
    /// it has both the atom's font table and the parent's dominant baseline.
    pub(in crate::layout) fn first_from_logical_block_start_with_metric(
        self,
        block_start: PhysicalSide,
        border_box_block_size: LayoutLength,
    ) -> Option<(LayoutLength, BaselineMetric)> {
        let (baseline, metric) = match block_start {
            PhysicalSide::Top => (
                self.vertical
                    .first
                    .map(PhysicalTopBaselineOffset::into_layout_length),
                self.vertical_metric,
            ),
            PhysicalSide::Bottom => (
                self.vertical
                    .first
                    .map(|baseline| border_box_block_size - baseline.into_layout_length()),
                self.vertical_metric,
            ),
            PhysicalSide::Left => (
                self.horizontal
                    .first
                    .map(PhysicalLeftBaselineOffset::into_layout_length),
                self.horizontal_metric,
            ),
            PhysicalSide::Right => (
                self.horizontal
                    .first
                    .map(|baseline| border_box_block_size - baseline.into_layout_length()),
                self.horizontal_metric,
            ),
        };
        baseline.map(|baseline| (baseline, metric))
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
            ..PhysicalBaselineSets::default()
        }
    }

    #[test]
    fn physical_baselines_project_from_each_logical_block_start_edge() {
        let sets = sets();
        let span = layout_pt(40.0);
        assert_eq!(
            sets.first_from_logical_block_start_with_metric(PhysicalSide::Top, span),
            Some((layout_pt(7.0), BaselineMetric::Alphabetic))
        );
        assert_eq!(
            sets.first_from_logical_block_start_with_metric(PhysicalSide::Left, span),
            Some((layout_pt(11.0), BaselineMetric::Alphabetic))
        );
        assert_eq!(
            sets.first_from_logical_block_start_with_metric(PhysicalSide::Right, span),
            Some((layout_pt(29.0), BaselineMetric::Alphabetic))
        );
        assert_eq!(
            sets.first_from_logical_block_start_with_metric(PhysicalSide::Bottom, span),
            Some((layout_pt(33.0), BaselineMetric::Alphabetic))
        );
    }

    #[test]
    fn named_physical_baselines_retain_their_metric() {
        let sets = PhysicalBaselineSets {
            vertical: BaselinePair {
                first: Some(PhysicalTopBaselineOffset::new(layout_pt(7.0))),
                last: Some(PhysicalTopBaselineOffset::new(layout_pt(17.0))),
            },
            horizontal: BaselinePair {
                first: Some(PhysicalLeftBaselineOffset::new(layout_pt(11.0))),
                last: Some(PhysicalLeftBaselineOffset::new(layout_pt(21.0))),
            },
            vertical_metric: BaselineMetric::Central,
            horizontal_metric: BaselineMetric::Central,
        };

        assert_eq!(sets.vertical_metric, BaselineMetric::Central);
        assert_eq!(sets.horizontal_metric, BaselineMetric::Central);
        assert_eq!(sets.vertical.first.unwrap().points(), 7.0);
        assert_eq!(sets.vertical.last.unwrap().points(), 17.0);
        assert_eq!(sets.horizontal.first.unwrap().points(), 11.0);
        assert_eq!(sets.horizontal.last.unwrap().points(), 21.0);
    }

    #[test]
    fn central_baseline_retains_metric_through_vertical_rl_right_edge_projection() {
        let sets = PhysicalBaselineSets {
            horizontal: BaselinePair {
                first: Some(PhysicalLeftBaselineOffset::new(layout_pt(37.5))),
                last: None,
            },
            horizontal_metric: BaselineMetric::Central,
            ..PhysicalBaselineSets::default()
        };

        assert_eq!(
            sets.first_from_logical_block_start_with_metric(PhysicalSide::Right, layout_pt(75.0)),
            Some((layout_pt(37.5), BaselineMetric::Central))
        );
    }

    #[test]
    fn perpendicular_axis_does_not_reinterpret_an_exported_coordinate() {
        let vertical_only = PhysicalBaselineSets::default().with_first_from_logical_block_start(
            PhysicalSide::Top,
            layout_pt(40.0),
            layout_pt(7.0),
            BaselineMetric::Alphabetic,
        );

        assert_eq!(
            vertical_only
                .first_from_logical_block_start_with_metric(PhysicalSide::Left, layout_pt(40.0),),
            None,
        );
    }
}
