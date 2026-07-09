# CSS Page Margin Box Parity

This note tracks support for CSS Paged Media margin boxes and the GCPM
generated-content features commonly used for WeasyPrint-compatible headers and
footers.

## Current Coverage

- The parser recognizes all 16 CSS Page 3 margin at-rules nested in `@page`,
  preserves exact at-rule names, and cascades declarations with page selector
  specificity, origins, importance, and cascade layers.
- Page margin boxes inherit from the page context and apply the HTML UA default
  alignment for the page-margin pseudo-elements. The first page context is
  rebuilt after document-root inheritance is known, so logical page margins and
  margin boxes use the root writing mode and direction from page one onward.
- Margin boxes are generated only when `content` is neither `normal` nor
  `none`; non-generated boxes still participate as zero-sized neighbors for
  variable-dimension resolution.
- Layout implements the CSS Page 3 fixed-dimension equation for corner and side boxes,
  including auto margins, overconstraint rules, negative used margins,
  fixed-axis percentage bases, min/max constraints, margins, borders, padding, and page
  border/padding interactions.
- Top/bottom and left/right triplets share the CSS Page 3 variable-dimension
  algorithm, including intrinsic min/max-content sizing from the shared CSS
  Text opportunity graph, min/max constraint saturation and re-allocation, and
  independent imaginary-side candidates for center/middle boxes. A generated
  center/middle box with an auto variable dimension receives the remaining
  space after definite symmetric side boxes, preserving its specified center
  alignment.
- Generated margin boxes resolve `width:min-content`,
  `width:max-content`, and `width:fit-content()` from those min/max-content
  inline sizes instead of treating intrinsic width keywords as `auto`.
- Generated page-margin content supports page counters, page-context counters,
  custom counter styles, `target-counter()`, `target-text()`, named strings,
  running elements, `leader()`, quotes, images, text decoration, text emphasis,
  shadows, whitespace processing, tabs, forced breaks, and soft wrapping.
- Page-margin fixed-box generated content maps physical margin-box rectangles
  through the margin box `writing-mode`, so vertical writing uses the physical
  height as logical inline size. `vertical-align` follows the CSS Paged Media
  table-cell rule and is resolved on the physical vertical axis regardless of
  writing mode.
- Variable physical block axes reuse the final generated-content line
  selection path with their fixed cross-axis content size as the inline
  constraint. This keeps orthogonal margin-box measurements consistent with
  soft wrapping, forced breaks, images, and replayable running content.
- Fixed physical axes also resolve `min-content`, `max-content`, and
  `fit-content` against the logical axis selected by the margin box
  `writing-mode`: vertical boxes use the replayable line-stack width for a
  physical block-size and inline intrinsic sizes for a physical height.
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
  establish stacking contexts that are sorted after replay. Negative `z-index`
  boxes replay after the page background but before the document canvas/content;
  non-negative boxes replay above document content. Both preserve opacity,
  clipping, and other effect groups.
- Background colors plus URL, linear-gradient, and radial-gradient background
  layers honor ordered layer painting, per-layer image, size, position, repeat,
  `background-origin`, and `background-clip` for normal boxes, page boxes, and
  margin boxes. Supported color, URL/image, linear-gradient, and
  radial-gradient paints are clipped to rounded `background-clip` areas for all
  three box classes.
- Viewport-dependent `@page` rules are resolved at page-context layout time,
  preserving the immutable initial page box needed for `vw`/`vh` descriptors.
  Page-margin box styles resolve their viewport-relative lengths against that
  same initial page box, before fixed-dimension used-value resolution, so empty
  generated boxes retain their specified background and margin geometry.
  Propagated canvas backgrounds paint the page padding box, including its
  padding band but leaving the page border visible, while page backgrounds use
  the special complete-page canvas painting area.
- Page-context visibility suppresses page backgrounds, borders, and generated
  boxes without hiding document content. Page outlines use the shared CSS UI
  outline painter and the final outline paint band. A `display:none` document
  root produces a blank canvas without author page decoration.

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
  page context without re-entering their source page group. Out-of-flow float
  occupancy is tracked separately from a normal-flow class-A predecessor, so
  it cannot manufacture a named-page boundary for the following sibling.
  Non-phantom inline line fragments likewise establish normal-flow page
  ownership before a following forced or named-page transition, so fixed
  descendants replay on every page spanned by that flow.
  Deeply fragmented flex layouts still need broader verification.
- Propagated root/body canvas colors and image layers now paint once, through
  the root-positioned canvas projection, independently from page-box
  backgrounds and borders. Remaining background work is limited to complex
  multi-page image positioning and unsupported CSS Images features.
- Layered backgrounds still need broader CSS Images coverage beyond Level 3
  linear/radial gradients and URL raster images.
- Variable-dimension sizing remains incomplete only for genuinely indefinite
  or mutually interdependent orthogonal cross-axis constraints.
- `writing-mode` is outside CSS Page 3's normative page-margin property list.
  Quire retains its normal CSS Writing Modes rendering for that undefined
  combination; three imported reftests (`dimensions-004`, `dimensions-013`,
  and `dimensions-014`) encode different compatibility behavior. The
  `quire-wpt` configuration records their exact, observed pixel ratios as
  runner-only expected differences; it does not alter either document or
  Quire's rendering.

## Verification

- Smoke tests cover all current page-margin behavior through local repository
  inputs, including WPT-derived fixed and variable dimensions, selector
  cascade, page counters, target references, named strings, generated text,
  vertical-writing fixed-box placement, background images, outlines,
  visibility, stacking order, and negative `z-index` replay.
- PDF tests verify that negative `z-index` page-margin opacity emits a
  transparency group, preventing regressions where below-document replay drops
  paint effects.
- Visual comparisons for imported WPT or WeasyPrint cases should follow
  `AGENTS/pdf_comparison.md`.
- With the documented exact-path runner compatibility settings, the 38
  `css/css-page/margin-boxes/` reftests pass. The three undefined
  `writing-mode` cases retain their measured visual differences in the result
  artifacts rather than being treated as renderer parity.
