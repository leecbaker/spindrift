# CSS Multi-column parity

Quire models a wrapped multicolumn formatting context as a source-ordered row
plan. Authored column block size, fragmentation progress capacity, and
container-local row offsets are distinct types. The plan records occupied
columns, row boundaries, actual final-row extent, and gap slots; paint replay
projects that geometry into physical page coordinates.

Implemented behavior includes:

- definite `column-height`, including zero, without exposing the positive
  termination capacity as CSS geometry;
- explicit `column-wrap: wrap` with `column-height: auto` and a definite
  principal block size, following the behavior selected by CSSWG issue 11754;
- balanced final rows after full fixed-height rows;
- nested complete-row packing, oversized row slicing, and discarded row-gap
  remainders at parent fragmentainer boundaries;
- individual spanner placement on the container-wide row grid, including
  consecutive and gap-crossing spanners;
- continuous principal ownership for direct positioned children, with
  fragmented ownership retained for descendants of fragmented in-flow
  containing blocks;
- separate in-flow row topology and physical fragment spans, so parallel
  positioned and float continuations participate in balancing, block-size,
  and paint projection without creating normal-flow break opportunities;
- opacity-scoped positioned descendants that lack a captured positioned
  containing block remain in their ancestor compositing group; and
- float-context class-C breaks retain an existing normal-flow background
  fragment across a cleared column boundary; and
- source-subject occupancy for monolithic size-contained boxes in zero-height
  columns.

The implementation follows CSS Multi-column Layout Level 2's
[multi-column model](https://drafts.csswg.org/css-multicol-2/#multi-column-model),
[`column-height`](https://drafts.csswg.org/css-multicol-2/#ch),
[`column-wrap`](https://drafts.csswg.org/css-multicol-2/#cwr), and
[spanning columns](https://drafts.csswg.org/css-multicol-2/#spanning-columns),
together with the CSS Fragmentation
[fragmentation model](https://www.w3.org/TR/css-break-3/#fragmentation-model).

Remaining limitations are recorded in `SPEC_DIVERGENCES.md`; that document is
the authoritative list rather than a change log. The focused
`css/css-multicol/column-height` WPT set currently passes 27 of 30 tests. The
remaining cases are `column-height-013` (promoted-wrapper decoration across
spanners), `column-height-024` (the final forced-break gap edge), and
`column-height-029` (nested spanner paint continuation).
