# Replay context contract

Layout in Quire has speculative selection phases and later paint/fragmentation
replay phases.  A replay must not infer semantic input from whichever mutable
`LayoutBuilder` state happens to be active at that later point.

The shared boundaries are:

- `PlacedFormattingContext` carries the placement, percentage bases, and
  `ReplayFloatScope` for independently placed flex and grid items. Table-cell
  inline planning and multicolumn child flow use the same scoped float
  boundary while retaining their distinct sizing algorithms.
- `InlineFloatReplay` is stored on every selected `InlineLineFragment`.
  `RequeryContainingBlock` is for ordinary source flow.  Transactions that
  have already committed an inline float or initial-letter exclusion use
  `FrozenSelectedBand`; it is reused only on the source fragmentainer and is
  re-resolved after relocation.
- `InlineLineSequence` carries `ReplayFloatScope`.  Nested independent
  sequences, including ruby base and annotation levels, use
  `IsolatedFormattingContext` while both collecting and painting.

`LayoutSnapshot` is an opaque wrapper around `RollbackLayoutState`. It remains
rollback machinery, not durable replay input. Every mutable `LayoutBuilder`
field belongs to one ownership class:

- immutable configuration/source input;
- persistent measurement or source cache;
- pass state, such as the selected footnote measurement mode;
- rollback state, restored by `LayoutSnapshot`; or
- a durable replay artifact that outlives the speculative pass which produced
  it.

In particular, immutable stylesheet/resource inputs and source-measurement
caches are never restored; footnote reservation/measurement selection is pass
state; pagination cursors, lexical scope stacks, counter state, and pending
page-local output are rollback state; and positioned/multicolumn deferred
children plus their containing-block spans are durable artifacts. A rollback
or replay transaction asserts its scope depths and keeps the latter artifacts
outside the scratch owner.

New replaying layout features must add immutable data to their replay artifact
rather than reading cursor, page, exclusion, or positioned state from the
builder during replay. Scratch positioned effects use a distinct page-index
space and are converted to document page indices only together with their
captured paint. Flex, grid, tables, and multicolumn layout may keep their own
sizing algorithms, but independently placed children should use the shared
placement and float-scope boundary.

Principal-box paint ownership is likewise explicit replay input. A
`PrincipalBoxPaintMode` reaches only the replayed formatting-context root:
`RootPaints` lets Block, Flex, or Grid emit its own background, border,
outline, and container-owned decoration; `ParentPaints` records that the
placing parent already emitted that root decoration at its resolved geometry.
Descendant layout always starts with `RootPaints`. This is not rollback state,
so speculative snapshots cannot leak ownership to a later descendant.

Split-grid source replay is an isolated transaction: it moves materialized
pages, page-local paint layers, and durable multicol positioned artifacts out
of `LayoutBuilder` before taking its local rollback snapshot. The resulting
per-item artifact owns continuous-coordinate paint plus source-marked semantic
effects. Each committed grid fragment clips the paint and consumes only the
events in its half-open source interval, so a shared boundary is assigned once
to the later slice.
