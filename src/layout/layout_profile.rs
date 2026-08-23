//! Feature-gated aggregate diagnostics for expensive layout phases.
//!
//! This module deliberately keeps the profile outside [`LayoutSnapshot`]. A
//! snapshot restores rendering state, whereas the profile describes every
//! layout attempt, including speculative Grid and float replays.

use std::cell::RefCell;
use std::time::{Duration, Instant};

std::thread_local! {
    static DOCUMENTS: RefCell<Vec<ActiveDocument>> = const { RefCell::new(Vec::new()) };
}

/// Start collecting one document's layout aggregate. Dropping the guard emits
/// one `quire::layout_profile` record for that document.
#[must_use = "the document profile guard must cover the complete layout"]
pub(in crate::layout) fn begin_document() -> LayoutProfileDocument {
    DOCUMENTS.with(|documents| documents.borrow_mut().push(ActiveDocument::default()));
    LayoutProfileDocument
}

/// A document-scoped profile guard.
pub(in crate::layout) struct LayoutProfileDocument;

impl Drop for LayoutProfileDocument {
    fn drop(&mut self) {
        let document = DOCUMENTS.with(|documents| {
            documents
                .borrow_mut()
                .pop()
                .expect("layout-profile document scopes must drop in LIFO order")
        });
        let stats = document.stats;
        log::info!(
            target: "quire::layout_profile",
            "document_us={} float_intrinsic_width_calls={} float_intrinsic_width_us={} float_auto_height_cache_hits={} float_auto_height_cache_misses={} float_auto_height_measurements={} float_auto_height_measurement_us={} grid_layout_final_calls={} grid_layout_intrinsic_calls={} grid_layout_orthogonal_calls={} grid_layout_us={} grid_track_sizing_final_passes={} grid_track_sizing_intrinsic_passes={} grid_track_sizing_items={} grid_track_sizing_us={} grid_baseline_plan_calls={} grid_baseline_plan_items={} grid_baseline_plan_us={} grid_feedback_initial_sweeps={} grid_feedback_container_sweeps={} grid_feedback_column_sweeps={} grid_feedback_items={} grid_feedback_us={} grid_feedback_inline_corrections={} grid_feedback_block_corrections={} grid_item_replays={} grid_item_replay_us={}",
            document.started.elapsed().as_micros(),
            stats.float_intrinsic_width.calls,
            micros(stats.float_intrinsic_width.elapsed),
            stats.float_auto_height_cache_hits,
            stats.float_auto_height_cache_misses,
            stats.float_auto_height_measurement.calls,
            micros(stats.float_auto_height_measurement.elapsed),
            stats.grid_layout_final.calls,
            stats.grid_layout_intrinsic.calls,
            stats.grid_layout_orthogonal_calls,
            micros(stats.grid_layout_final.elapsed + stats.grid_layout_intrinsic.elapsed),
            stats.grid_track_sizing_final.calls,
            stats.grid_track_sizing_intrinsic.calls,
            stats.grid_track_sizing_items,
            micros(stats.grid_track_sizing_final.elapsed + stats.grid_track_sizing_intrinsic.elapsed),
            stats.grid_baseline_plan.calls,
            stats.grid_baseline_plan.items,
            micros(stats.grid_baseline_plan.elapsed),
            stats.grid_feedback_initial_sweeps,
            stats.grid_feedback_container_sweeps,
            stats.grid_feedback_column_sweeps,
            stats.grid_feedback_items,
            micros(stats.grid_feedback.elapsed),
            stats.grid_feedback_inline_corrections,
            stats.grid_feedback_block_corrections,
            stats.grid_item_replay.calls,
            micros(stats.grid_item_replay.elapsed),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::layout) enum GridProfilePurpose {
    Final,
    Intrinsic,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::layout) enum GridFeedbackSweep {
    Initial,
    Container,
    Column,
}

#[must_use = "the profile guard must cover the measured phase"]
pub(in crate::layout) fn float_intrinsic_width_scope() -> LayoutProfileScope {
    LayoutProfileScope::new(LayoutProfilePhase::FloatIntrinsicWidth, 0, None)
}

pub(in crate::layout) fn record_float_auto_height_cache_hit() {
    with_stats(|stats| stats.float_auto_height_cache_hits += 1);
}

pub(in crate::layout) fn record_float_auto_height_cache_miss() {
    with_stats(|stats| stats.float_auto_height_cache_misses += 1);
}

#[must_use = "the profile guard must cover the measured phase"]
pub(in crate::layout) fn float_auto_height_measurement_scope() -> LayoutProfileScope {
    LayoutProfileScope::new(LayoutProfilePhase::FloatAutoHeightMeasurement, 0, None)
}

#[must_use = "the profile guard must cover the measured phase"]
pub(in crate::layout) fn grid_layout_scope(
    purpose: GridProfilePurpose,
    item_count: usize,
    orthogonal: bool,
) -> LayoutProfileScope {
    with_document(|document| {
        if orthogonal {
            document.stats.grid_layout_orthogonal_calls += 1;
        }
        document.grid_purposes.push(purpose);
    });
    LayoutProfileScope::new(
        LayoutProfilePhase::GridLayout(purpose),
        item_count,
        Some(purpose),
    )
}

#[must_use = "the profile guard must cover the measured phase"]
pub(in crate::layout) fn grid_track_sizing_scope(item_count: usize) -> LayoutProfileScope {
    let purpose = DOCUMENTS.with(|documents| {
        documents
            .borrow()
            .last()
            .and_then(|document| document.grid_purposes.last().copied())
    });
    let phase = purpose
        .map(LayoutProfilePhase::GridTrackSizing)
        .unwrap_or(LayoutProfilePhase::Ignored);
    LayoutProfileScope::new(phase, item_count, None)
}

#[must_use = "the profile guard must cover the measured phase"]
pub(in crate::layout) fn grid_baseline_plan_scope(item_count: usize) -> LayoutProfileScope {
    LayoutProfileScope::new(LayoutProfilePhase::GridBaselinePlan, item_count, None)
}

#[must_use = "the profile guard must cover the measured phase"]
pub(in crate::layout) fn grid_feedback_sweep_scope(
    sweep: GridFeedbackSweep,
    item_count: usize,
) -> LayoutProfileScope {
    LayoutProfileScope::new(LayoutProfilePhase::GridFeedback(sweep), item_count, None)
}

pub(in crate::layout) fn record_grid_feedback_inline_correction() {
    with_stats(|stats| stats.grid_feedback_inline_corrections += 1);
}

pub(in crate::layout) fn record_grid_feedback_block_correction() {
    with_stats(|stats| stats.grid_feedback_block_corrections += 1);
}

#[must_use = "the profile guard must cover the measured phase"]
pub(in crate::layout) fn grid_item_replay_scope() -> LayoutProfileScope {
    LayoutProfileScope::new(LayoutProfilePhase::GridItemReplay, 1, None)
}

#[must_use = "the profile guard must cover the measured phase"]
pub(in crate::layout) struct LayoutProfileScope {
    phase: LayoutProfilePhase,
    item_count: usize,
    started: Instant,
    grid_layout_purpose: Option<GridProfilePurpose>,
}

impl LayoutProfileScope {
    fn new(
        phase: LayoutProfilePhase,
        item_count: usize,
        grid_layout_purpose: Option<GridProfilePurpose>,
    ) -> Self {
        Self {
            phase,
            item_count,
            started: Instant::now(),
            grid_layout_purpose,
        }
    }
}

impl Drop for LayoutProfileScope {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed();
        with_document(|document| {
            document.stats.record(self.phase, self.item_count, elapsed);
            if let Some(purpose) = self.grid_layout_purpose {
                let popped = document.grid_purposes.pop();
                debug_assert_eq!(popped, Some(purpose));
            }
        });
    }
}

#[derive(Debug, Clone, Copy)]
enum LayoutProfilePhase {
    Ignored,
    FloatIntrinsicWidth,
    FloatAutoHeightMeasurement,
    GridLayout(GridProfilePurpose),
    GridTrackSizing(GridProfilePurpose),
    GridBaselinePlan,
    GridFeedback(GridFeedbackSweep),
    GridItemReplay,
}

struct ActiveDocument {
    started: Instant,
    stats: LayoutProfileStats,
    grid_purposes: Vec<GridProfilePurpose>,
}

impl Default for ActiveDocument {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            stats: LayoutProfileStats::default(),
            grid_purposes: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
struct LayoutProfileStats {
    float_intrinsic_width: TimedCount,
    float_auto_height_cache_hits: u64,
    float_auto_height_cache_misses: u64,
    float_auto_height_measurement: TimedCount,
    grid_layout_final: TimedCount,
    grid_layout_intrinsic: TimedCount,
    grid_layout_orthogonal_calls: u64,
    grid_track_sizing_final: TimedCount,
    grid_track_sizing_intrinsic: TimedCount,
    grid_track_sizing_items: u64,
    grid_baseline_plan: TimedCount,
    grid_feedback: TimedCount,
    grid_feedback_initial_sweeps: u64,
    grid_feedback_container_sweeps: u64,
    grid_feedback_column_sweeps: u64,
    grid_feedback_items: u64,
    grid_feedback_inline_corrections: u64,
    grid_feedback_block_corrections: u64,
    grid_item_replay: TimedCount,
}

impl LayoutProfileStats {
    fn record(&mut self, phase: LayoutProfilePhase, item_count: usize, elapsed: Duration) {
        match phase {
            LayoutProfilePhase::Ignored => {}
            LayoutProfilePhase::FloatIntrinsicWidth => self.float_intrinsic_width.add(elapsed),
            LayoutProfilePhase::FloatAutoHeightMeasurement => {
                self.float_auto_height_measurement.add(elapsed);
            }
            LayoutProfilePhase::GridLayout(GridProfilePurpose::Final) => {
                self.grid_layout_final.add(elapsed);
            }
            LayoutProfilePhase::GridLayout(GridProfilePurpose::Intrinsic) => {
                self.grid_layout_intrinsic.add(elapsed);
            }
            LayoutProfilePhase::GridTrackSizing(GridProfilePurpose::Final) => {
                self.grid_track_sizing_final.add(elapsed);
                self.grid_track_sizing_items += item_count as u64;
            }
            LayoutProfilePhase::GridTrackSizing(GridProfilePurpose::Intrinsic) => {
                self.grid_track_sizing_intrinsic.add(elapsed);
                self.grid_track_sizing_items += item_count as u64;
            }
            LayoutProfilePhase::GridBaselinePlan => {
                self.grid_baseline_plan.add(elapsed);
                self.grid_baseline_plan.items += item_count as u64;
            }
            LayoutProfilePhase::GridFeedback(sweep) => {
                self.grid_feedback.add(elapsed);
                self.grid_feedback_items += item_count as u64;
                match sweep {
                    GridFeedbackSweep::Initial => self.grid_feedback_initial_sweeps += 1,
                    GridFeedbackSweep::Container => self.grid_feedback_container_sweeps += 1,
                    GridFeedbackSweep::Column => self.grid_feedback_column_sweeps += 1,
                }
            }
            LayoutProfilePhase::GridItemReplay => self.grid_item_replay.add(elapsed),
        }
    }
}

#[derive(Debug, Default)]
struct TimedCount {
    calls: u64,
    items: u64,
    elapsed: Duration,
}

impl TimedCount {
    fn add(&mut self, elapsed: Duration) {
        self.calls += 1;
        self.elapsed += elapsed;
    }
}

fn with_stats(callback: impl FnOnce(&mut LayoutProfileStats)) {
    with_document(|document| callback(&mut document.stats));
}

fn with_document(callback: impl FnOnce(&mut ActiveDocument)) {
    DOCUMENTS.with(|documents| {
        if let Some(document) = documents.borrow_mut().last_mut() {
            callback(document);
        }
    });
}

fn micros(duration: Duration) -> u128 {
    duration.as_micros()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current_stats() -> LayoutProfileStats {
        DOCUMENTS.with(|documents| {
            let documents = documents.borrow();
            let stats = &documents.last().expect("test document is active").stats;
            LayoutProfileStats {
                float_intrinsic_width: TimedCount {
                    calls: stats.float_intrinsic_width.calls,
                    ..TimedCount::default()
                },
                float_auto_height_cache_hits: stats.float_auto_height_cache_hits,
                float_auto_height_cache_misses: stats.float_auto_height_cache_misses,
                grid_layout_final: TimedCount {
                    calls: stats.grid_layout_final.calls,
                    ..TimedCount::default()
                },
                grid_layout_intrinsic: TimedCount {
                    calls: stats.grid_layout_intrinsic.calls,
                    ..TimedCount::default()
                },
                grid_track_sizing_final: TimedCount {
                    calls: stats.grid_track_sizing_final.calls,
                    ..TimedCount::default()
                },
                grid_track_sizing_intrinsic: TimedCount {
                    calls: stats.grid_track_sizing_intrinsic.calls,
                    ..TimedCount::default()
                },
                grid_track_sizing_items: stats.grid_track_sizing_items,
                grid_baseline_plan: TimedCount {
                    calls: stats.grid_baseline_plan.calls,
                    items: stats.grid_baseline_plan.items,
                    ..TimedCount::default()
                },
                grid_feedback_initial_sweeps: stats.grid_feedback_initial_sweeps,
                grid_feedback_container_sweeps: stats.grid_feedback_container_sweeps,
                grid_feedback_column_sweeps: stats.grid_feedback_column_sweeps,
                grid_feedback_items: stats.grid_feedback_items,
                grid_feedback_inline_corrections: stats.grid_feedback_inline_corrections,
                grid_feedback_block_corrections: stats.grid_feedback_block_corrections,
                grid_item_replay: TimedCount {
                    calls: stats.grid_item_replay.calls,
                    ..TimedCount::default()
                },
                ..LayoutProfileStats::default()
            }
        })
    }

    #[test]
    fn accumulates_and_classifies_phase_counts() {
        let document = begin_document();
        drop(float_intrinsic_width_scope());
        record_float_auto_height_cache_hit();
        record_float_auto_height_cache_miss();
        let final_grid = grid_layout_scope(GridProfilePurpose::Final, 3, true);
        drop(grid_track_sizing_scope(3));
        drop(final_grid);
        let intrinsic_grid = grid_layout_scope(GridProfilePurpose::Intrinsic, 3, false);
        drop(grid_track_sizing_scope(2));
        drop(intrinsic_grid);
        drop(grid_baseline_plan_scope(3));
        drop(grid_feedback_sweep_scope(GridFeedbackSweep::Initial, 3));
        drop(grid_feedback_sweep_scope(GridFeedbackSweep::Container, 3));
        drop(grid_feedback_sweep_scope(GridFeedbackSweep::Column, 3));
        record_grid_feedback_inline_correction();
        record_grid_feedback_block_correction();
        drop(grid_item_replay_scope());

        let stats = current_stats();
        assert_eq!(stats.float_intrinsic_width.calls, 1);
        assert_eq!(stats.float_auto_height_cache_hits, 1);
        assert_eq!(stats.float_auto_height_cache_misses, 1);
        assert_eq!(stats.grid_layout_final.calls, 1);
        assert_eq!(stats.grid_layout_intrinsic.calls, 1);
        assert_eq!(stats.grid_track_sizing_final.calls, 1);
        assert_eq!(stats.grid_track_sizing_intrinsic.calls, 1);
        assert_eq!(stats.grid_track_sizing_items, 5);
        assert_eq!(stats.grid_baseline_plan.calls, 1);
        assert_eq!(stats.grid_baseline_plan.items, 3);
        assert_eq!(stats.grid_feedback_initial_sweeps, 1);
        assert_eq!(stats.grid_feedback_container_sweeps, 1);
        assert_eq!(stats.grid_feedback_column_sweeps, 1);
        assert_eq!(stats.grid_feedback_items, 9);
        assert_eq!(stats.grid_feedback_inline_corrections, 1);
        assert_eq!(stats.grid_feedback_block_corrections, 1);
        assert_eq!(stats.grid_item_replay.calls, 1);
        drop(document);
    }

    #[test]
    fn begins_each_document_with_fresh_aggregate_state() {
        let first = begin_document();
        record_float_auto_height_cache_hit();
        drop(first);

        let second = begin_document();
        let stats = current_stats();
        assert_eq!(stats.float_auto_height_cache_hits, 0);
        drop(second);
    }
}
