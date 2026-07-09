# CSS Gaps Parity

This document tracks Quire's CSS Gaps Level 1 support for gap decorations. The
normative reference is CSS Gaps Level 1, with layout geometry supplied by CSS
Flexible Box Layout, CSS Grid Layout, CSS Multi-column Layout, and CSS Box
Alignment.

## Current Support

### WPT snapshot

The local renderable `css/css-gaps/grid/` WPT run currently passes 77 of 171
tests. The remaining failures are primarily fragmented and subgridded grid
decorations, plus complex intersection and writing-mode cases.

- `row-gap`, `column-gap`, `gap`, and the legacy `grid-row-gap`,
  `grid-column-gap`, and `grid-gap` aliases are computed properties used by
  flex, grid, and the current definition-list multicolumn path. The parser
  accepts `normal`, non-negative `<length-percentage>` values, and the
  `<line-width>` keywords.
- Gap decoration computed values now model the CSS Gaps property surface:
  `row-rule-*`, `column-rule-*`, `rule-*`, `rule-overlap`, and the cap/junction
  inset longhands and shorthands.
- Rule width/style/color values support comma lists and `repeat(<integer>, ...)`
  plus one `repeat(auto, ...)` segment for assignment to the resolved gutter
  count. When that assignment truncates an authored trailing expansion, its
  first values are retained in authored order.
- Same-page block and inline flex containers emit row and column rule strokes
  from resolved flex line/item gutter metadata. Same-page block and inline grid
  containers emit row and column rule strokes from Taffy's resolved
  track/gutter metadata, including empty tracks, and explicit grid intersection
  joins use the final grid extent derived from both resolved line offsets and
  placed grid areas, rather than extending fixed-track rules through unused
  container free space. Intersection joins use opposing-side item areas that
  span the perpendicular gap. Grid
  `rule-break: normal` discards discontiguous rule portions that cross a
  spanning item, while still joining contiguous portions. Grid
  `rule-visibility-items` filters atomic portions before empty areas can be
  absorbed by a joined segment, then rejoins contiguous visible portions.
  Grid endpoints at empty junctions are
  classified as caps when the crossing segment is suppressed by grid
  visibility metadata or by the crossing rule's own width/style/color.
  Definition-list and collected inline-sequence multicolumn paths emit column
  rules between resolved columns.
- Flex gap-decoration painting does not apply the grid/multicolumn-only
  `*-rule-visibility-items` filter to resolved flex gutters.
- Normal-flow fragmented flex containers project gap decorations into
  page-local fragments, clipping physical row-axis rule extent to the visible
  fragment block range. Block grid containers also replay gap decorations into
  each captured page fragment and clip row-gutter metadata to the fragment's
  visible block range.
- The shared painter assigns decoration values to gaps, splits rules at
  crossing gaps for `rule-break: intersection`, applies cap and junction
  insets including `overlap-join`, filters segments for
  `*-rule-visibility-items`, honors `rule-overlap`, and paints the CSS line
  styles used by borders. Double gap rules paint two symmetric stripes around
  the gap-rule centerline rather than reusing a box-edge border orientation.

## Remaining Gaps

- Empty-area and non-grid complex spanning intersection classification still
  need layout-owned segment endpoint metadata. The current shared painter can
  split and join inferred crossings, and grid item spans are line-aware for
  opposing-side explicit intersection joins, normal cross-intersection joins,
  visibility filtering, and cap endpoints at grid-empty or non-painting-rule
  junctions when Taffy exposes placement metadata, but it cannot see every
  spec-defined empty/spanning area without more input from flex, grid, and
  multicol layout.
- Fragmented grid containers still need layout-owned fragment metadata for
  exact CSS Grid and CSS Break behavior across named-page/page-size changes and
  complex item fragmentation. Multicolumn containers, plus remaining flex edge
  cases outside the normal-flow fragment replay path, need fragment-local
  gap-rule clipping and replay.

## Next Steps

- Extend the layout-mode-neutral gutter model with full row/column empty-area
  spans and segment endpoint classifications.
- Feed that model from multicolumn column geometry, and extend the current
  flex/grid metadata bridges with explicit endpoint and page-fragment
  classifications beyond the grid item line-span joins now available.
- Add WPT-derived cases for flex, grid, multicolumn, vertical writing modes,
  intersection overlap ordering, and fragmented grid/multicolumn containers.
