# CSS Grid Lanes parity

## Intrinsic auto-repeat

Grid Lanes resolves an intrinsic `repeat(auto-fill|auto-fit, ...)` in two
stages. The hypothetical stage ignores authored grid-axis placement and
copies each item to every automatic start that fits. It derives the repeated
slot maxima, then selects a finite repeat count. The final stage materializes
that explicit topology before line resolution and lane packing.

This preserves fixed prefix/suffix tracks, repeated line names, and the
provenance required to collapse only empty repeated `auto-fit` tracks. End
implicit tracks are distinct `grid-auto-*` tracks; default `auto` implicit
tracks use their own intrinsic contribution rather than inheriting a repeated
track's used breadth.

This follows CSS Grid Level 3's [intrinsic masonry repeat sizing](https://www.w3.org/TR/css-grid-3/#masonry-intrinsic-repeat),
[auto-fit occupancy and collapse](https://www.w3.org/TR/css-grid-3/#masonry-auto-fit),
and [lane placement](https://www.w3.org/TR/css-grid-3/#grid-lanes-layout-and-placement-algorithm).

## Grid-axis definiteness

The Grid Lanes row axis retains the containing block's original percentage
basis. An automatic grid height remains indefinite during intrinsic repeat
sizing and item percentage resolution; its measured post-layout height is
not fed back as a percentage basis. This implements CSS Grid's cyclic
percentage rule for intrinsic grid sizing.

## Remaining gaps

- General `grid-auto-columns` / `grid-auto-rows` sizing functions after an
  intrinsic repeat, beyond the covered default `auto` path.
- Subgrid propagation, fragmentation, complete writing-mode behavior, and
  advanced alignment/safe-overflow behavior for Grid Lanes.
