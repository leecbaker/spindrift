# CSS Page Margin Box Parity

This note tracks support for CSS Paged Media margin boxes and the GCPM
generated-content features commonly used for WeasyPrint-compatible headers and
footers.

## Current Coverage

- The parser recognizes all 16 CSS Page 3 margin at-rules nested in `@page`,
  preserves exact at-rule names, and cascades declarations with page selector
  specificity, origins, importance, and cascade layers.
- Page margin boxes inherit from the page context and apply the HTML UA default
  alignment for the page-margin pseudo-elements.
- Margin boxes are generated only when `content` is neither `normal` nor
  `none`; non-generated boxes still participate as zero-sized neighbors for
  variable-dimension resolution.
- Layout implements CSS Page 3 fixed dimensions for corner and side boxes,
  including auto margins, overconstraint rules, negative used margins,
  percentages, min/max constraints, margins, borders, padding, and page
  border/padding interactions.
- Top/bottom and left/right triplets share the CSS Page 3 variable-dimension
  algorithm, including intrinsic min/max-content sizing from the shared CSS
  Text opportunity graph.
- Generated margin boxes resolve `width:min-content`,
  `width:max-content`, and `width:fit-content()` from those min/max-content
  inline sizes instead of treating intrinsic width keywords as `auto`.
- Generated page-margin content supports page counters, page-context counters,
  custom counter styles, `target-counter()`, `target-text()`, named strings,
  running elements, `leader()`, quotes, images, text decoration, text emphasis,
  shadows, whitespace processing, tabs, forced breaks, and soft wrapping.
- Page-margin content resolution is item-based internally: inline generated
  content remains typed, and `element()` resolves to an embedded running-element
  item. Captures that normal layout can represent are replayed through an
  isolated source-box layout pass, preserving box paint and dimensions in the
  margin box, including supported root `::before`/`::after` generated content
  without double-applying source counters.
- `string-set` stores typed generated-content lists for text, attrs with string
  fallbacks, counters, quotes, leaders, `content(first-letter)`,
  `content(marker)` for textual markers, `content(before)`, `content(after)`,
  generated-pseudo target cross-references, URL image items, and Level 3
  linear/radial gradient image items, so supported non-text inline items can
  replay through `string()` in margin boxes.
- `string(..., start)` and `element(..., start)` use final assignment placement:
  named strings update from the source box's first emitted fragment, and running
  elements use a zero-size source marker because the source box is removed from
  normal flow. Flex item layout now records page-local fragment metadata and
  updates running-element markers from moved flex item fragments. Table-cell
  source `string-set` and `position: running()` capture from the final visible
  cell fragment, with running cells removed from table grid construction.
  Running-only table rows still capture the removed cell assignment from the
  collapsed row source marker. Split table-cell child replay consumes table
  fragment metadata for direct
  source named-string and running-element assignments. Cells and cell children
  that move or split across pages use their first visible table-cell fragment
  as the source.
  Fragmented table rows now capture `string-set` from the first visible row
  piece, including rows moved to a later page by overflow. Repeated table
  header/footer copies suppress element side effects during replay, so visual
  copies do not emit duplicate named strings, running elements, anchors, or
  bookmarks. Table-root named strings use the final moved table fragment for
  `string(..., start)`, and basic table-root running-element replay preserves
  table paint in page-margin boxes. Table-row running elements are removed from
  table flow and assigned from a zero-size post-break source marker, so
  `element(..., start)` uses the final row source page. Pre-rendered nested
  table/flex descendants inside split table cells preserve captured
  named-string and running-element assignments when their visible fragment is
  replayed. Rowspanning table-cell child fragments, including nested
  table/flex descendants, use the first visible page fragment for
  `string(..., start)` and `element(..., start)` lookups. Running table rows
  replay nested table and flex descendants through the existing isolated
  `element()` path.
- Paint order follows clockwise page-margin tree order, and margin boxes
  establish stacking contexts. Negative `z-index` boxes are replayed below the
  document stack while preserving opacity, clipping, and other effect groups.
- Background colors plus URL, linear-gradient, and radial-gradient background
  layers honor ordered layer painting, per-layer image, size, position, repeat,
  `background-origin`, and `background-clip` for normal boxes, page boxes, and
  margin boxes. Supported color, URL/image, linear-gradient, and
  radial-gradient paints are clipped to rounded `background-clip` areas for all
  three box classes.

## Remaining Work

- `element()` replay for fragmented flex captures and more complex table-root
  captures needs broader WeasyPrint/WPT coverage. Table assignment propagation
  still needs broader coverage for rare rowspan/collapsed-track/repeated-copy
  interactions.
- Full `string-set` content lists do not yet preserve box-preserving generated
  fragments beyond supported inline quote, leader, textual marker, target
  cross-reference, URL image, and Level 3 linear/radial gradient image items.
- Page-name propagation suppresses flex item entry values while preserving
  descendant class-A switches inside flex items. Fragmented table rows now
  switch named page context from row, cell, and cell-descendant `page` values,
  including explicit `page:auto`. Row-spanning cell `page` values persist
  across spanned row boundaries, while an explicit `page:auto` row exits the
  named context. Repeated table header/footer copies paint in the destination
  page context without re-entering their source page group. Deeply fragmented
  flex layouts still need broader verification.
- Layered backgrounds still need broader CSS Images coverage beyond Level 3
  linear/radial gradients and URL raster images.

## Verification

- Smoke tests cover all current page-margin behavior through local repository
  inputs, including WPT-derived fixed and variable dimensions, selector
  cascade, page counters, target references, named strings, generated text,
  background images, outlines, visibility, stacking order, and negative
  `z-index` replay.
- PDF tests verify that negative `z-index` page-margin opacity emits a
  transparency group, preventing regressions where below-document replay drops
  paint effects.
- Visual comparisons for imported WPT or WeasyPrint cases should follow
  `AGENTS/pdf_comparison.md`.
