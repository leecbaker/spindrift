# CSS Flexbox Parity

Last updated: 2026-07-08

This document tracks Quire's implementation status for CSS Flexible Box
Layout. The normative references are CSS Flexible Box Layout Level 1, CSS Box
Alignment Level 3, CSS Writing Modes Level 4, CSS Sizing Level 3, and CSS
Fragmentation Level 3. WeasyPrint tests and behavior are useful compatibility
references, but spec conformance is the priority when the two disagree.

## Current Implementation

- Flex layout is implemented under `src/layout/flex/`.
- Taffy 0.11.0 remains the core engine for line formation, flexible length
  distribution, wrapping, gaps, auto margins, and common alignment.
- Taffy-specific limitations and Quire adapter workarounds are tracked in
  `docs/TAFFY_SHORTCOMINGS.md`.
- Quire supplies CSS/PDF-specific adapter behavior for flex child collection,
  anonymous text items, intrinsic estimates, `flex-basis` keyword resolution,
  automatic minimum sizes, replaced-item sizing, baseline correction, absolute
  flex child static positions, inline-flex atoms, and PDF painting.
- Flex child collection shares the blockified formatting-context itemization
  path with grid child collection. Normal-flow flex items establish an
  independent formatting context for intrinsic estimates and replay, so first
  and last child block margins stay contained by the flex item, floats do not
  intrude into sibling items or outside block flow, and blockified inline-source
  item paint is normalized before item layout.
- Absolutely positioned flex children are collected out of flow. Their
  static-position probe uses fixed sole-item sizing and ignores authored
  flexing, including `flex-basis`, while normal absolute positioning computes
  the final used size. The probe exports both physical static-position starts
  to positioned layout, so `auto` inset pairs use the flex static-position
  rectangle, including both `justify-content` main-axis alignment and
  `align-items` cross-axis alignment in horizontal and vertical writing modes.
- Flex container `width:min-content`, `width:max-content`, and
  `width:fit-content` route through explicit flex intrinsic contribution
  records rather than final Taffy layout. Item records carry min/max main and
  cross contributions, flex base size, hypothetical main size, grow/shrink
  factors, definite cross-size contributions, and gap handling. Wrapped row
  min-content uses the largest item contribution, nowrap rows keep all items
  on one line, definite flex-basis values floor max-content contributions, and
  non-growing item contributions are capped by the outer flex base size for
  min-content sizing. Cyclic percentage gaps contribute only their
  non-percentage length component. Definite flex item widths contribute to
  column flex min-content inline sizing, including wrapped column containers
  with empty fixed-width items. When a definite-height wrapped column flex
  container forms multiple columns, min-content inline sizing uses the largest
  item cross contribution rather than summing columns; max-content sizing still
  uses the summed wrapped line cross sizes. Wrapped CSS column containers
  remeasure max-content item contributions with the largest max-content cross
  contribution as the available cross size, so percentage-width items and
  float-only block descendants participate in flex-basis calculation with the
  same available size used by the intrinsic cross-size algorithm.
- Simple inline-content multi-column flex items measure their inline sequence at
  the resolved column width and use the balanced column block-size for
  auto-height hypothetical cross sizing, so flex replay paints all balanced
  column lines and column rules.
- Normal-flow block flex containers preserve definite used widths even when
  they overflow the containing block, and fixed-width block flex containers
  resolve horizontal auto margins with the same CSS 2.2 block-width equation as
  normal block boxes. Orthogonal normal-flow flex containers with
  `width:auto` route through flex intrinsic sizing so their physical block axis
  shrink-wraps rather than filling the containing block's horizontal span.
  Auto-height row flex containers apply CSS min/max-height constraints to the
  used container height, including the rule that a larger `min-height` wins
  over a smaller `max-height`. Flex containers with an automatic height and a
  preferred `aspect-ratio` transfer their definite used width into a definite
  content height for flex item cross-size resolution.
- Flex item margins preserve negative length and percentage values through the
  Taffy layout bridge, so overlapping flex items participate in
  order-modified layout and paint in the CSS Flexbox `order` sequence.
- Max-content flex container sizing uses the concrete ideal flex-fraction
  algorithm from CSS Flexbox 9.9.1.1. The draft's web-compatible intrinsic
  sizing algorithm remains unresolved, so any future browser-compatibility
  deltas should be tracked as spec divergences rather than encoded as
  undefined behavior.
- Flex item base-size estimates use a definite stretched cross size as the
  descendant containing-block inline size. This covers vertical-writing items
  whose descendants resolve percentage padding from the stretched physical
  height before the item's row-axis flex base size is finalized, and
  horizontal row flex items whose inline replaced descendants resolve
  `height:100%` against the stretched item before transferring width through
  an intrinsic aspect ratio.
- Flex item `stretch` min cross sizes resolve through CSS Sizing Level 4
  stretch-fit sizing when the flex line/container cross size is definite.
  Final flex item border boxes are floored by their padding and borders, so a
  zero stretched cross-size target cannot create a negative content box or
  clip away item decorations.
- Replaced flex items with an intrinsic aspect ratio transfer cross-axis
  min/max constraints into the content-basis candidate used by
  `flex-basis:auto`, while keeping main-axis min/max constraints out of the
  flex base size. Raster image natural dimensions are converted from source
  image pixels to CSS px and then to Quire layout points before they contribute
  to flex max-content main-size calculations, including vertical-writing row
  flex items.
- Authored `aspect-ratio` values on non-replaced flex items participate in
  Quire's flex base-size and automatic minimum main-size calculations, including
  content-box transferred flex bases that Taffy expands through item padding
  and borders according to `box-sizing`. The flex/Taffy adapter carries those
  transferred sizes through semantic content-box and non-content typed lengths
  until the final Taffy scalar boundary; stretched cross sizes remain
  authoritative for final flex item geometry.
- Raster image flex items resolve CSS Sizing `aspect-ratio` fallback before
  flex sizing, so bare authored ratios override the natural image ratio while
  `auto <ratio>` keeps the natural ratio when present. Auto cross sizes that
  depend on the final flexed main size are corrected before flex line metadata
  and replay are finalized.
- Percentage `flex-basis` values fall back to content sizing when the flex
  container's main size is indefinite, including authored `0%`; this content
  basis ignores the item's authored main-size property as required by the flex
  base-size algorithm.
- Flex item estimates map inline-content logical inline/block contributions
  through the item's writing mode, so vertical-writing row flex items with
  forced inline breaks expose the correct physical main and cross sizes.
- Element flex item estimates use the same logical-to-physical mapping for
  simple vertical-writing inline content, so inherited `vertical-rl`
  `inline-flex` column wrapping uses line-height-sized physical item widths
  and keeps definite logical inline sizes as definite physical heights.
- Flex layout now records explicit per-line metadata for item source ranges,
  main/cross extents, first/last baseline candidates, and collapsed-item
  struts. `FlexFragmentPlan` now carries page-local boundary fragments, line
  ranges, original and page-local item bounds, content/decoration slices, and
  fragment metadata for block flex replay.
- Normal-flow block flex containers can now fragment at flex boundaries in
  paged media: row containers break by flex line, column containers break by
  item progression, and forced item `break-before`/`break-after` values are
  consumed at the flex-container layer. Oversized row-line and column-item
  fragments split into page-local item slices and replay split visual content
  from the source item layout with page-local clipping. Fragmented block flex
  containers clone simple container backgrounds onto page-local fragments.
  Document-canvas flex boxes and floated/atomic replay contexts continue to use
  their existing specialized pagination behavior.
- `visibility: collapse` now uses a visible-layout probe to measure collapsed
  item struts and source-line placement, then relayouts with collapsed items
  omitted from main-axis distribution. Wrapped lines are repacked when a strut
  expands an affected line.
- Wrapped row flex containers apply Quire post-Taffy baseline passes for
  `align-content: baseline` and `align-content: last baseline`, using recorded
  first/last line baseline sets and content baselines synthesized from flex
  item estimates when needed. Horizontal writing modes align physical y
  baselines, and vertical-writing CSS row containers align physical x baselines
  from horizontal baseline estimates that use the selected inline line-box
  baseline: central for `text-orientation:mixed`/`upright` and alphabetic for
  `text-orientation:sideways`. Missing row flex item
  baselines synthesize from flex border edges using the CSS Align alphabetic
  line-under baseline for sideways/horizontal typographic mode and the central
  baseline for vertical `text-orientation:mixed`/`upright` baseline-sharing
  groups. Wrapped row `align-items: baseline` self-alignment preserves each
  stretched flex line's cross-axis slot while replacing Taffy's synthesized
  baselines with measured item baselines. Row flex item baseline estimates
  also walk normal-flow block
  descendants, so a block flex item can export the first/last baseline of its
  in-flow block content rather than falling back to a synthesized border edge.
  Empty `inline-flex` row containers do not export a flex baseline; their
  parent inline formatting context synthesizes the atom baseline from the
  margin box, so padding, borders, and margins align with adjacent text as
  CSS Inline requires.
  Baseline fallback paths for single-item self-alignment and line
  content-alignment use CSS Align logical start/end sides through
  writing-mode-aware side mapping, including column flex content-alignment and
  column-axis self-alignment fallback when compatible baseline sharing cannot
  apply.
  Row-axis nested flex containers export first and last baselines from their
  intrinsic estimates so they can join parent flex baseline-sharing groups; definite
  wrapped `row`, `row-reverse`, and `wrap-reverse` nested flex containers form
  estimated flex lines so their exported first and last baselines come from the
  first and last wrapped lines, using each line's startmost/endmost item and
  the correct reversed cross-axis line offsets. Auto-width nested row flex
  containers constrained by definite `max-width` or `fit-content(<length>)`
  also use that constraint when estimating wrapped exported baselines.
  `width:max-content` row flex containers include main-axis gap lengths and
  definite flex-basis contributions in their intrinsic main size, so parent
  baseline alignment observes the same line breaks as the nested flex layout.
  Vertical-writing nested row flex containers export physical x-axis
  first/last baselines for parent row flex baseline-sharing groups, including
  `wrap-reverse` line packing, when their wrapped line estimates have either a
  definite physical cross size or an auto cross size resolved from the wrapped
  line stack, and when percentage physical cross sizes resolve against a
  definite parent cross size.
- Existing smoke coverage includes row/column and reverse directions, wrapping,
  `order`, `flex-grow`/`flex-shrink`, intrinsic `flex-basis` keywords,
  percentage and mixed math sizing, gaps, auto margins, baseline alignment,
  `self-start`/`self-end`, safe alignment fallback, absolute flex children,
  generated `::before`/`::after` flex items including order sorting,
  `display: contents` child flattening including mixed inline/block
  descendants exposed as flex items, inline-flex baselines including empty
  margin-box synthesis, replaced images, single-subject distributed
  `justify-content` fallback, post-minimum
  main-axis repacking, vertical-writing row flex items with forced inline
  breaks, and several nested flex/table cases.
- The Taffy adapter maps vertical-writing row flex containers that use Taffy's
  physical column axis onto the correct horizontal cross-start side. This
  covers `vertical-rl` row wrapping, `wrap-reverse`, single-line cross-axis
  `flex-start` alignment, child writing-mode-independent explicit item sizes,
  avoids letting `direction` flip vertical-writing column block-axis flow, and
  covers `vertical-rl; direction:rtl` wrapped column line stacking.

## Remaining Gaps

- Paged flex fragmentation is partial. Boundary breaks between row lines and
  column items work for normal-flow block flex containers, and oversized
  row-line/column-item fragments now produce page-local item slices with visual
  content continuation. Full border/outline/background decoration slicing and
  complete fragment-plan metadata for links, running elements, named pages, and
  other PDF side effects still need work.
- `visibility: collapse` still needs broader audits for column-axis and
  `wrap-reverse` strut placement, collapsed replaced items, and rare
  writing-mode combinations.
- Baseline handling now covers horizontal and vertical-writing row
  `align-content` baseline packing. Baseline fallback alignment is
  writing-mode aware for the covered self- and content-alignment cases,
  including column flex content-alignment fallback, and nested vertical-writing
  row flex containers export text-orientation-aware horizontal baselines for
  definite wrapped rows, auto-width wrapped rows, definite-parent percentage
  wrapped rows, and `wrap-reverse`. Row flex items with normal-flow block
  descendants export descendant text baselines for first/last baseline
  self-alignment. True column-axis baseline sharing still needs vertical
  baseline-set support; indefinite percentage nested exported baselines and
  rarer nested edge cases also need more work.
- Intrinsic flex container sizing now has a dedicated contribution pipeline,
  but still needs WPT-backed auditing for the unresolved web-compatible
  algorithm, deeply nested flex/table descendants, complex block-child
  multicolumn descendants, rare
  orthogonal-flow combinations, and exact child block-size estimates for
  descendant formatting contexts beyond the covered wrapped-column float cases.
- Flex layout still needs a WPT-backed audit for generated content inside
  anonymous flex text items, absolute static-position edge cases, and remaining
  reverse/wrap-reverse combinations outside the covered vertical-writing
  adapter tests.

## Test Backlog

- Import local WPT cases by behavior group: line breaking, flexible lengths,
  intrinsic sizing, auto minimums, alignment, collapsed items, writing modes,
  absolute children, and fragmentation.
- Port high-value local WeasyPrint tests from
  `WeasyPrint/tests/layout/test_flex.py`, especially column pagination,
  page-break interactions, replaced flex items, auto margins, and nested flex.
- Existing local conformance smoke coverage in
  `tests/smoke/flex_conformance.rs` and `tests/smoke/layout_inline_flex.rs`
  covers collapsed-item struts, flex intrinsic min/max-content widths,
  intrinsic percentage-gap behavior, baseline alignment, row-axis baseline content
  alignment, definite wrapped nested `row`/`row-reverse`/`wrap-reverse`
  baseline exports, max-constrained auto-width nested baseline exports,
  fit-content-constrained nested baseline exports, definite-parent percentage
  nested vertical baseline exports, column flex content
  alignment fallback, baseline fallback in vertical and `wrap-reverse` cases,
  vertical-writing row flex item line-under baseline synthesis,
  wrapped row `align-items: baseline` line-slot preservation,
  absolutely positioned flex static positions including vertical-lr/vertical-rl
  auto-position content-box coverage, `display: contents` flattening
  including WPT-shaped mixed inline/block flex items, generated `::before` and
  `::after` flex item generation/order sorting, `order`-modified paint order
  with overlapping negative-margin flex items, wrapped-line repacking after
  collapsed struts, row line fragmentation, column item fragmentation,
  oversized flex item slices, split-item visual continuation, and forced flex
  item breaks, including wrapped column flex min-content cross-size aggregation
  and max-content available cross-size propagation for percentage-width
  float-only flex items, plus stretched vertical flex-item descendant
  percentage padding and non-negative border-box sizing for `min-height:
  stretch` flex items. Add
  Quire-specific regression tests for full PDF fragment
  metadata, running elements and links inside fragmented flex items, full
  cloned decorations, and Taffy adapter edge cases as fragmentation is
  completed. Current flex sizing coverage includes auto-height row containers
  with min/max-height constraints.

## Implementation Notes

- Keep Taffy as the primary algorithm unless a specific required CSS behavior
  cannot be represented through its public model.
- Prefer adding explicit adapter metadata, such as flex-line and baseline
  records, over re-inferring layout structure at each downstream use site.
- Update `SPEC_DIVERGENCES.md` whenever a remaining gap is implemented or
  narrowed.
