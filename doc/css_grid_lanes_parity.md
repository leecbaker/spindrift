# CSS Grid Lanes parity

## Intrinsic auto-repeat

Grid Lanes resolves an intrinsic `repeat(auto-fill|auto-fit, ...)` in two
stages. The hypothetical stage contributes a definite item only at its
normalized Grid line range and copies an automatic item to every automatic
start that fits. It uses the shared Grid track-sizing adapter with an
indefinite percentage basis, derives counting breadths, and selects a finite
repeat count from the definite preferred, maximum, or minimum grid-axis
constraint. Zero intrinsic slots use the one-CSS-pixel repeat-count floor
required by CSS Grid.

The final stage establishes `auto-fit` occupancy and collapse while the
topology is frozen, then materializes the equivalent active numbered-repeat
template before sizing, line resolution, and lane packing. Its ordinary Grid
track-sizing pass is authoritative for repeated and end implicit tracks: the
hypothetical counting breadths never enlarge used geometry. That pass hands
its already aligned offsets directly to Grid Lanes; expanding them back to
source lines gives collapsed tracks zero extent and never stretches or aligns
the source topology a second time.

Repeated line names and source-line provenance remain available for authored
placement even though final sizing uses the active topology. End implicit
tracks formed by the numbered-template replay are appended with distinct
`grid-auto-*` provenance. For the legacy intrinsic `auto-fit` WPT path, a
fixed fragment before the repeat is retained for source-line resolution but
normalized to an intrinsic `auto` slot in the private final template, matching
the reference numbered-repeat geometry. The reservation horizon is the
largest automatic span, as required by those legacy references.

This follows CSS Grid Level 3's [intrinsic masonry repeat sizing](https://www.w3.org/TR/css-grid-3/#masonry-intrinsic-repeat),
[auto-fit occupancy and collapse](https://www.w3.org/TR/css-grid-3/#masonry-auto-fit),
and [lane placement](https://www.w3.org/TR/css-grid-3/#grid-lanes-layout-and-placement-algorithm).

## Grid-axis definiteness

The Grid Lanes row axis retains the containing block's original percentage
basis. An automatic grid height remains indefinite during intrinsic repeat
sizing and item percentage resolution; its measured post-layout height is
not fed back as a percentage basis. This implements CSS Grid's cyclic
percentage rule for intrinsic grid sizing.

## Fixed-axis placement and subgrids

Grid Lanes normalizes fixed-axis placement once before sizing and packing.
This follows ordinary Grid conflict handling for reversed and equal line
pairs, line-plus-span values, two spans, named-area edges, and negative
lines. In particular, `grid-column: 2 / 5` occupies all three intervening
tracks rather than a one-track default span.

When that area is a subgrid axis, the child retains the used parent track
slice and its intervening gutters; descendant placement uses hypothetical
lines but cannot create child-owned inherited tracks. The subgrid's other
axis remains an ordinary Grid axis and uses its local gap.

## Positioned descendants and outline paint

Absolutely positioned Grid descendants use the Grid positioning containing
block, including the padding-edge fallback for automatic Grid placement. This
keeps their percentage sizes independent from the Grid item's used size.

Stacking-axis content distribution uses the packed stacking range as its sole
alignment subject. The same range's end bounds self-alignment of the final
item in a lane; a definite container size is the alignment container and does
not enlarge either range.

Quire also uses a documented compatibility paint policy for overlapping
outlines: an ordinary normal-flow Grid outline paints before auto/zero-z
positioned descendants, while an outline owned by a positioned or effect
stacking context remains in that context's final local outline phase. CSS UI
leaves this overlap ordering implementation-defined; the policy follows the
historical optional per-box placement in CSS 2.2 and matches the positioned
Grid WPT references.

See [CSS Grid §9.1](https://drafts.csswg.org/css-grid-1/#abspos),
[CSS UI §3](https://drafts.csswg.org/css-ui-4/#outline-painting), and
[CSS 2.2 Appendix E](https://www.w3.org/TR/CSS22/zindex.html).

## Remaining gaps

- Replaced/aspect-ratio row-lane items whose cyclic grid-axis percentage is
  coupled to an automatic cross-axis size are not yet raster-exact. Their
  virtual contribution and final packed size need a shared cross-axis
  definiteness representation.
- Row-lane auto-track repeats with cyclic percentage-sized items can still
  select too many repetitions. The virtual intrinsic contribution needs to
  preserve the percentage's Grid sizing phase without prematurely resolving
  it against a final lane area.
- General `grid-auto-columns` / `grid-auto-rows` sizing functions after an
  intrinsic repeat, beyond the covered default `auto` path.
- Subgrid fragmentation, row-lane stacking-axis alignment, inherited-axis
  local-gap delta adjustment, and complete writing-mode behavior for Grid
  Lanes. Horizontal column lanes
  support stacking-axis `normal`/`stretch`, positional, safe-overflow, and
  `align-self` alignment after normal placement, including fill and track
  reversal. The remaining auto-height `column-align-items-001`, `002`, `006`,
  `007`, and `016` PDF comparisons differ from their flex-based references'
  intrinsic track sizing rather than the Grid Lanes alignment geometry.
