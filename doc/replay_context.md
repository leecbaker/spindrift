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

`LayoutSnapshot` remains rollback machinery.  It is not durable replay input.
New replaying layout features should add immutable data to their replay
artifact rather than reading cursor, page, exclusion, or positioned state from
the builder during replay.  Flex, grid, tables, and multicolumn layout may
keep their own sizing algorithms, but independently placed children should use
the shared placement and float-scope boundary.
