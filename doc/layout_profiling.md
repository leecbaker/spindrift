# Layout phase profiling

The `layout-profile` Cargo feature records aggregate, per-document timings and
counts for the layout phases most likely to repeat during Grid sizing. It is a
developer diagnostic only: it has no public Rust or CLI API and normal builds
compile none of its state, timing, or logging code.

Build and render a document with the feature enabled:

```sh
cargo build --release --features layout-profile
RUST_LOG=spindrift::layout_profile=info target/release/spindrift input.html output.pdf
```

The logger emits exactly one structured `spindrift::layout_profile` record for each
document layout. Measurements are thread-local so concurrent renders do not
mix totals. A layout snapshot never includes the profile state: speculative
float probes and Grid replays remain visible in the completed document's
aggregate.

The record includes:

- `float_intrinsic_width_*`: float shrink-to-fit inline-size resolution.
- `float_auto_height_cache_*` and `float_auto_height_measurement_*`: cache
  effectiveness and isolated auto-height float replays.
- `grid_layout_final_*`, `grid_layout_intrinsic_*`, and
  `grid_layout_orthogonal_calls`: complete Grid layout attempts by purpose and
  physical-axis swapping containers.
- `grid_track_sizing_*`: Taffy track-sizing passes, classified by their parent
  final layout or intrinsic probe, with total sized items.
- `grid_baseline_plan_*`: baseline-plan measurement calls and items.
- `grid_feedback_*`: initial, container-basis, and column-basis item estimate
  sweeps plus each inline/block correction they trigger.
- `grid_item_replay_*`: final Grid item formatting-context replays.

All elapsed values are integer microseconds (`*_us`); use them comparatively,
not as a stable benchmark. For repeatable local comparisons, run the bundled
Criterion workloads, which mirror the two slow Grid WPT families without a
sibling WPT checkout:

```sh
cargo bench --features layout-profile --bench weasyprint_samples grid_float_baseline
```

The workload has 30 mixed-baseline floating grids with 116 items and 36
content-baseline floating grids with 90 items. It also supplies
baseline-neutral, non-floating, and all-horizontal variants for isolating the
cost of those dimensions.

For call-path rather than aggregate-cost diagnosis, combine this feature with
the existing macOS-only stack profiler:

```sh
cargo build --release --features stack-profile,layout-profile
```
