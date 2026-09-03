# CSS Box Alignment Parity

Last updated: 2026-07-08

This document tracks Spindrift's implementation status for CSS Box Alignment
features that are shared across layout modes. The normative reference is CSS
Box Alignment Level 3, with layout-mode-specific behavior from CSS Flexible
Box Layout Level 1, CSS Display Level 3, CSS Writing Modes Level 4, and CSS
Fragmentation Level 3.

## Current Implementation

- The cascade parses and computes `align-content`, `justify-content`,
  `align-items`, `align-self`, `justify-items`, `justify-self`, and the
  `place-*` shorthands, including overflow-position modifiers where the
  grammar permits them, including explicit `safe normal` and `unsafe normal`
  values. Omitted overflow-position is preserved separately from explicit
  `safe` and `unsafe`, which lets layout modes apply their own initial-overflow
  rules.
- Flex containers map `align-content` to Taffy's flex line-packing model for
  positional, distributed, safe, and stretch values. Spindrift corrects wrapped
  `normal`/`stretch` overflow fallback after layout so overflowing stretched
  lines pack to `flex-start`, including `wrap-reverse`. Spindrift adds post-layout
  baseline content-alignment for wrapped row flex containers on both physical
  y and physical x cross axes, using vertical-writing horizontal baseline
  estimates when the CSS row axis is vertical. Those vertical row estimates
  use selected inline line-box baseline offsets, including the central
  baseline for `text-orientation:mixed`/`upright` and the alphabetic baseline
  for `text-orientation:sideways`. Missing row flex item baselines synthesize
  from flex border edges using the CSS Align alphabetic line-under baseline
  for sideways/horizontal typographic mode and the central baseline for
  vertical `text-orientation:mixed`/`upright` baseline-sharing groups. Row flex item
  estimates walk normal-flow block descendants for first/last baseline
  self-alignment before using synthesized fallback baselines. Wrapped row
  `align-items: baseline` self-alignment preserves each stretched flex line's
  cross-axis slot while replacing Taffy's synthesized baselines with measured
  item baselines.
  Baseline fallback alignment is writing-mode-aware, including the required
  safe start/end fallback for column flex line packing and column-axis
  self-alignment fallback where compatible baseline-sharing groups cannot
  form. Row-axis nested flex
  containers export first and last baselines from their intrinsic estimates for
  parent baseline alignment,
  including definite-width `row`, `row-reverse`, and `wrap-reverse` flex
  containers whose exported baselines come from their first and last estimated
  flex lines and the line's startmost/endmost items. Auto-width nested row flex
  containers constrained by definite `max-width` or `fit-content(<length>)`
  use that constraint for exported wrapped baselines. `width:max-content` row
  flex exports include main-axis gap lengths and definite flex-basis
  contributions when deciding whether the exported last baseline comes from
  the same line or a wrapped line. Nested vertical-writing row flex containers
  export physical x-axis first/last baselines for parent row flex
  baseline-sharing groups, including `wrap-reverse` line packing, when their
  wrapped line estimates have either a definite physical cross size or an
  auto cross size resolved from the wrapped line stack, and when percentage
  physical cross sizes resolve against a definite parent cross size.
- Ordinary block containers with a same-page definite used block size now apply
  `align-content` to their contents as a single block-axis alignment subject.
  Horizontal writing modes align on the physical y axis; vertical writing modes
  align captured descendant bounds on the logical horizontal block axis, so
  `vertical-lr` and `vertical-rl` use opposite physical x directions. `normal`
  behaves as start alignment, distribution values use their single-subject
  fallback, omitted positions fall back to start on overflow unless the block
  container is scrollable, explicit `safe` positions fall back to start,
  explicit `unsafe` positions may overflow, baseline values use their
  content-alignment fallback, and non-`normal` values establish an independent
  formatting context. Same-page descendant paint, links, and bookmark targets
  are translated with the aligned content.
- Table cells apply `align-content` to their content-box placement. `normal`
  preserves the legacy CSS 2.2 `vertical-align` mapping, while non-`normal`
  values use CSS Box Alignment content placement. Horizontal writing modes use
  the cell content-box block axis, and vertical writing modes align in-flow
  cell contents on the logical horizontal block axis. First- and last-baseline
  `align-content` cells participate in the row's baseline-sharing group when
  their inline axis is parallel to the table row's inline axis; orthogonal
  cells use the CSS Align baseline fallback. Row-spanning baseline cells
  participate in their start-most or end-most spanned row as required. Other
  single-subject values use the same block-axis fallback as ordinary block
  containers.
- Spindrift's current definition-list column layout applies multicol
  `align-content` overflow defaults: omitted positional values remain unsafe,
  explicit `safe` values fall back to block-start on overflow, and distribution
  values keep their safe fallback.
- Same-page grid layout maps common CSS Box Alignment values for
  `align-content`, `justify-content`, `align-items`, `justify-items`,
  `align-self`, and `justify-self` through Taffy's grid alignment model.
  Simple horizontal first- and last-baseline self-alignment between measured
  text items in the same row is corrected with a Spindrift-owned post-layout pass;
  horizontal `justify-self`/`justify-items` `self-start`/`self-end` placement
  also uses a post-layout correction so LTR/RTL grid items align the edge
  defined by their own inline direction; horizontal `align-self`/`align-items`
  `self-start`/`self-end` uses the same correction layer for block-axis
  placement. Horizontal
  `justify-self`/`justify-items` `left`/`right` uses the same correction layer
  so those keywords remain physical in LTR and RTL grids. Same-page horizontal
  grid container first/last exported baselines come from occupied grid rows for
  inline-grid and nested grid baseline-sharing cases.
  Broader baseline alignment and orthogonal writing-mode-sensitive
  `self-start`/`self-end` details still need follow-up handling.
- Existing smoke coverage checks flex line stretch/end/baseline behavior,
  `place-content` expansion, block-container centering, min-height-constrained
  block-container centering, default/safe/unsafe
  block overflow behavior, vertical-writing block-axis placement, table-cell
  content alignment overriding `vertical-align`, vertical-writing table-cell
  block-axis placement and overflow defaults for block and inline text subjects,
  compatible table-cell baseline groups, orthogonal table-cell baseline fallback,
  row-spanning first/last-baseline table-cell groups, definition-list column overflow defaults, translated
  descendant link annotations and bookmark targets, out-of-flow positioned
  descendants inside aligned blocks, nested definite, max-constrained,
  max-content-with-gap, and fit-content-constrained wrapped flex baseline
  exports, vertical-writing row flex first/last-baseline content packing,
  vertical-writing row flex item line-under baseline synthesis,
  wrapped row `align-items: baseline` line-slot preservation,
  nested vertical-writing row flex first/last `wrap-reverse` baseline exports,
  auto-width and definite-parent percentage nested vertical row flex first/last
  baseline exports,
  column flex first/last-baseline self-alignment fallback, basic same-page grid
  `justify-items` placement, horizontal `justify-self`/`justify-items`
  `self-start`/`self-end` direction-sensitive placement, horizontal
  `align-self`/`align-items` `self-start`/`self-end` direction-sensitive
  placement, RTL
  `justify-self`/`justify-items` `left`/`right` physical placement, horizontal
  first-/last-baseline same-row text alignment, simple spanning grid baseline
  groups, plus nested grid last exported baseline alignment,
  and the parser's valid/invalid overflow-position combinations.

## Remaining Gaps

- Grid alignment is partial. Common same-page self/content alignment keywords
  are passed to Taffy, and simple horizontal first-/last-baseline same-row
  text alignment plus occupied-row first/last exported grid container
  baselines are corrected with Spindrift-measured item baselines. Horizontal
  grid `justify-self`/`justify-items` and `align-self`/`align-items`
  `self-start`/`self-end`, plus `justify-self`/`justify-items` `left`/`right`,
  placement is covered for LTR/RTL grids. Simple spanning items share baselines
  with same-start-row or same-end-row peers, but full exported baseline
  synthesis, broader spanning baseline groups, broader writing-mode-specific
  self-alignment, page fragmentation, and broader WPT coverage remain incomplete.
- General multi-column `align-content` behavior needs a dedicated conformance
  pass once column balancing and fragmentation are represented as durable
  alignment subjects.
- Block-container and table-cell `align-content` are intentionally limited to
  same-page content. Fragmented block containers and fragmented table cells
  need page-fragment-aware alignment semantics before non-start values can be
  applied across page breaks.
- Nested vertical-writing flex baseline exports still need broader coverage for
  indefinite percentage cross sizes and rarer nested edge cases. True
  column-axis baseline sharing still needs vertical baseline-set support; until
  then, column flex first/last-baseline self-alignment and content baseline
  values use the CSS Align safe start/end fallback because their alignment axes
  cannot share the available block-axis baselines.

## Test Backlog

- Import WPT coverage for `align-content` on block containers, including
  writing modes, overflow-position fallback, distribution fallback, and
  interaction with `min-height`/`max-height`.
- Import table-cell `align-content` cases covering `normal`/`vertical-align`
  compatibility, rowspans, and fragmentation.
- Add targeted tests for general multi-column alignment once column fragments
  expose column boxes as alignment subjects.
- Expand flex baseline content-alignment tests across vertical writing modes,
  reverse directions, fallback cases, and fragmentation boundaries.
