//! Final used-value computation for a flex formatting context.
//!
//! The modules below mirror the phases of the Flexbox layout algorithm.  They
//! intentionally expose only crate-local implementation details; sibling flex
//! modules use the shared alignment, baseline, estimate, and Taffy adapters
//! directly.

use super::*;

mod algorithm;
mod balance;
mod baseline;
mod cross_axis;
mod cross_remeasurement;
mod fragmentation;
mod item_setup;
mod item_sizing;
mod line_sizing;
mod line_topology;
mod main_axis;
mod normal_flow;

pub(in crate::layout::flex) use self::algorithm::*;
pub(in crate::layout::flex) use self::balance::*;
pub(in crate::layout::flex) use self::baseline::*;
pub(in crate::layout::flex) use self::cross_axis::*;
pub(in crate::layout::flex) use self::cross_remeasurement::*;
pub(in crate::layout::flex) use self::fragmentation::*;
#[allow(unused_imports)]
pub(in crate::layout::flex) use self::item_setup::flex_sizing_children_with_used_box_edges;
pub(in crate::layout::flex) use self::item_sizing::*;
pub(in crate::layout::flex) use self::line_sizing::*;
pub(in crate::layout::flex) use self::line_topology::*;
pub(in crate::layout::flex) use self::main_axis::*;
pub(super) use self::normal_flow::*;

#[cfg(test)]
mod tests_algorithm;
#[cfg(test)]
mod tests_cross_axis;
#[cfg(test)]
mod tests_lines;
#[cfg(test)]
mod tests_remeasurement;
