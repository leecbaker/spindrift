# CSS Grid Parity

Last updated: 2026-08-13

This document tracks Spindrift's CSS Grid implementation. It is a living
tracking document: update it whenever grid behavior is added, narrowed,
deferred, or found to diverge from the specs. The normative references are
CSS Grid Layout Level 1, CSS Grid Layout Level 2, CSS Box Alignment Level 3,
CSS Display Level 3, CSS Writing Modes Level 4, CSS Sizing Level 3, and CSS
Fragmentation Level 3. WeasyPrint behavior is useful compatibility context,
but spec conformance is the priority when they disagree.

## Current Implementation

- `margin-trim` derives zero used margins from placed logical grid areas before
  final track sizing and replay. It covers all logical edges and writing modes,
  keeps empty tracks relevant, and ignores collapsed `auto-fit` tracks.
  Fragmentation-specific trimming remains deferred.

### WPT snapshot

The focused local `css/css-grid/grid-model/` reftest run passes **44 of 47**
runnable tests. Grid replay now uses Taffy's resolved item border-box origin
without reapplying the start margin, preserving Grid items' independent
formatting contexts and direct-item `::first-line`/`::first-letter` state.
Grid item replay clears both cached and typed margins after Taffy positions an
item's margin box, so normal-flow replay cannot reapply fixed margins.
Intrinsic Grid probes measure cleared floats in source order, so their
contributions agree with final float layout. Scroll-container replay now
applies the resolved `overflow-x`/`overflow-y` clip to stretched item paint,
including vertical writing modes and RTL, while definite-size Grid containers
use the shared normal-flow prebreak rule at a fragmentainer boundary.
Paged PDF keeps native scrollbar reservation/interaction out of scope; this
work covers the common scrollport clipping geometry rather than scrollbar
chrome.

Two focused cases are intentionally not counted as Grid implementation
targets: `display-inline-grid.html` compares authored fixed Grid tracks with
legacy table row-height distribution, and
`grid-container-ignores-first-letter-002.html` requires HTML button control
display semantics in addition to Grid.

The last full local renderable `css/css-grid/` WPT run passed 661 of 1,414
tests. The focused `css/css-grid/abspos/` run now passes 105 of 142 tests,
including all 16 positioned grid-descendant containing-block tests and all 17
orthogonal positioned-grid-item tests. The focused
`css/css-grid/grid-lanes/items/` run passes all 18 cases, including cyclic
percentage item sizing, flexible-track automatic minima, and replaced items
in `minmax(auto, 0)` tracks. This includes 257 of 736 renderable Grid Lanes
tests. The remaining
failures are concentrated in Level 3 Grid Lanes track sizing, subgrids,
baseline alignment, positioned descendants, and the corresponding fragmented
or writing-mode behavior.

- Spindrift has grid-specific geometry scaffolding in `src/layout/grid.rs`,
  including typed grid coordinates and projection into page-top paint space.
- Standard Grid Level 1 containers and normal-flow items consume effective
  zoom at a distinct used-style boundary. Fixed track breadths, implicit
  tracks, gaps, box geometry, and intrinsic contributions scale once;
  percentages resolve against the zoomed Grid content box, while `fr` and
  intrinsic track sizing remain algorithmic. Grid Lanes Level 3's
  lanes-specific model and positioned layout remain outside this milestone.
- Grid's CSS-to-Taffy boundary is shared with Flex for direction, box sizing,
  common alignment keywords, borders, and percentage gaps. Grid item physical
  margin and padding percentages resolve against the Grid container's logical
  inline size before entering Taffy's physical axes, including vertical
  writing modes, RTL, mixed `calc()` values, and zoom. The resolved edge
  metrics are retained with the placed item and reused by replay and
  post-layout corrections, preventing either physical-width resolution or a
  second application of the same edge.
- Taffy 0.13.0 is the core Grid engine. Its template-area representation
  preserves authored row/column dimensions even when cells are unnamed, and it
  resolves horizontal-tb `self-start`/`self-end` against each item's direction.
  Spindrift retains vertical-writing, baseline, fragmentation, and physical-side
  corrections outside that public model.
- CSS display parsing now accepts `grid`, `inline-grid`, `block grid`,
  `inline grid`, and `run-in grid`. Grid boxes are recognized as independent
  formatting contexts.
- Computed values now carry Grid Level 1 track lists, template areas, auto
  tracks, auto-flow, and item placement longhands. The first parser pass
  covers practical explicit tracks, `repeat()`, `auto-fill`/`auto-fit`
  auto-repeat with fixed-size validation, `minmax()`, `fit-content()`, `fr`,
  template areas, auto tracks, auto-flow, and basic line/span placement.
  Grid track breadth length-percentages reject definitely negative values in
  bare tracks, `minmax()`, `fit-content()`, and auto-repeat fragments.
  `grid-row` and `grid-column` shorthand expansion is also
  implemented, including omitted-end forms that expand to `auto`, and
  `grid-area` shorthand expansion works for basic placement values. Named line
  occurrence syntax such as `main 2` and `span main 2` parses and is preserved
  through Taffy placement. `grid-area` shorthand expansion covers one- through
  four-value forms, omitted custom-ident end values, and invalid overlong
  shorthand rejection. Grid placement line names now reject non-identifier
  tokens and CSS-wide/custom-reserved keywords in both longhands and
  shorthands, escaped grid custom idents are decoded to canonical names, and
  bracketed track line names use the same validation and CSS tokenization,
  including escaped hex terminator whitespace and rejection of empty `[]`
  line-name lists.
  `grid-template-areas` string tokens decode CSS string escapes before area
  tokenization, including through `grid-template` shorthand expansion.
  `grid-template` shorthand expansion supports `none`,
  `<track-list> / <track-list>`, and string-based template rows with optional
  row sizes and column tracks. `grid` shorthand expansion supports explicit
  template forms and both row/column `auto-flow` branches.
  `grid-template-areas` rejects non-rectangular named areas and invalid cell
  tokens.
- Layout dispatch has a dedicated grid entrypoint. The first same-page path
  collects normal-flow grid items, runs a Taffy-backed Grid Level 1 layout,
  and replays each item through Spindrift's existing block, inline, table, flex,
  replaced, paint, and side-effect machinery. Grid container
  roots accept explicit parent-owned principal-box paint during flex replay;
  this suppresses only the grid root's decoration and gap rules, never its
  descendants' paint.
  definite/available-height setup reserves block-axis margins through the
  shared cursor-bounds fragmentainer capacity primitive used by block, flex,
  and table fragmentation. Grid now builds a grid-local committed fragment plan
  from resolved row offsets and the current fragmentainer before painting:
  row-boundary breaks are preferred, and oversized row bands fall back to the
  shared source-slice decision used by flex and table. Fragmented grid
  container backgrounds, outlines, and gap decoration projection consume those
  committed slices when a planned slice is available for the target page. The
  plan also derives fragment records that pair each source slice with the
  committed fragment offset, active fragmentainer kind, and any transition
  required before that fragment; a grid fragment cursor maps those committed
  source offsets to page-local paint geometry for the current paged-media replay
  path. Row-boundary planning now derives forced and avoid break metadata
  from grid item styles and item row spans: forced row boundaries are committed
  even when the grid would otherwise fit, and avoid-constrained row boundaries
  are skipped while a later non-avoid boundary can still make progress. The
  forced/avoid row-boundary search now consumes the shared, target-aware
  break-value combiner and fragmentation break-opportunity chooser, with the
  active fragmentainer kind passed into the grid planner rather than inferred
  from paged media, so future column fragmentation can reuse the same break
  math with a column fragmentainer kind. Committed grid transitions now preserve
  that active kind before the replay layer applies the shared
  `FragmentainerKind` page-cursor materialization gate. Grid container wrapper
  `break-before`/`break-after` page transitions use the shared standalone box
  break context and the same materialization gate, so column-specific forced
  values remain separate from page fragmentation.
  Fragment records derive visible grid-item fragment metadata and
  gap-decoration item views from the committed source slice, preserving
  original item geometry plus clipped source-space item ranges for the replay
  and decoration layers. Normal-flow replay now consumes committed fragment
  records, applying the committed fragment transition before replaying each
  fragment record; grid items wholly contained by a fragment use normal replay,
  while grid items spanning a fragment boundary replay from the original source item
  layout and clip to the selected page-local slice. Whole-item grid fragment
  replay now attaches committed fragment metadata and updates captured running
  element assignments to that page-local placement. Full grid fragmentation
  remains incomplete until positioned grid children consume fragment slices,
  split-item off-page replay transfers captured assignments back to the target
  fragment, and named-string side-effect replay consumes committed slices.
  Ordinary same-page flow grid items establish an independent formatting
  context for measurement and replay, so descendant block margins do not
  collapse through the grid item.
  Grid child collection shares the blockified formatting-context itemization
  path with flex, including anonymous text item construction, CSS
  document-whitespace-only text suppression even in preserved `white-space`
  modes, order sorting, out-of-flow splitting, and blockified
  inline-source paint normalization.
  `inline-grid` boxes now use the same child collection, intrinsic estimate,
  Taffy geometry, and item replay path inside an atomic inline fragment, with
  a same-page horizontal first exported baseline derived from the first
  occupied grid row rather than paint order. Layout containment suppresses
  both Grid-provided and captured descendant line baselines and uses the
  synthesized border-box fallback consistently.
- Child collection and the Taffy adapter are intentionally narrow first
  passes. Template-area rectangles and generated `area-start`/`area-end`
  line names are passed to Taffy for basic named-area and generated-line
  placement. Same-page auto-placement covers row flow, column flow into
  implicit auto columns, and dense backfill for simple spanning items through
  the Taffy adapter. `grid-auto-rows` and
  `grid-auto-columns` lists cycle across simple same-page implicit tracks.
  Same-page items spanning definite fixed tracks include intervening row and
  column gaps in their replayed border-box geometry. Horizontal `direction: rtl`
  auto-placement starts from the inline-start/rightmost explicit column on the
  same-page path. Fixed-size `repeat(auto-fill, ...)` expands to the number of
  repeated tracks that fit a definite same-page grid inline size, and
  fixed-size `repeat(auto-fit, ...)` collapses empty repeated tracks before
  content alignment. Fixed-size auto-repeat fragments resolve mixed
  length-percentage `calc()` track breadths against a definite grid axis before
  entering Taffy, while retaining its `auto-fit` empty-track collapse.
  Flexible `fr` tracks distribute definite container width for
  basic same-page normal-flow grids. Simple backward named spans from a
  definite end line and negative named line placements can synthesize startward
  implicit tracks in non-repeat and finite numbered-repeat column and row
  grids, including area-created explicit tracks, and in fixed-size definite
  `auto-fill` column and row grids by freezing the pre-adjustment auto-repeat
  count. Simple backward named spans and negative named line placements also
  work before fixed-size definite `auto-fit` column and row grids, including
  empty-track collapse after startward implicit-track expansion. These cases
  use backward-cycled `grid-auto-columns`/`grid-auto-rows` sizes and preserve
  explicit line references and shifted generated area line references after the
  prepended tracks. Positive named implicit line placement after fixed-size
  definite `auto-fill` and `auto-fit` repeats is covered on both axes and uses
  cycled `grid-auto-columns`/`grid-auto-rows` after the frozen repeated tracks,
  including multi-track `auto-fill` repeated fragments and forward named spans
  after single-track `auto-fill` repeated fragments.
  Child collection creates anonymous grid items for
  non-whitespace text runs, ignores CSS document-whitespace-only text runs
  even when `white-space` preserves them, and uses existing box-tree
  `display: contents` flattening so flattened children participate as grid
  items. Non-contiguous anonymous text runs separated by an absolutely
  positioned child become separate grid items; the matching
  `anonymous-grid-item-001.html` WPT passes without changing document-canvas
  body-margin placement. Grid containers keep their
  items' margins contained while their own outer block margins remain eligible
  to collapse with adjacent normal-flow block siblings. Tree-abiding `::before`
  and `::after`
  generated boxes participate as same-page grid items, including
  order-modified auto-placement with real children. Out-of-flow children are
  split out of normal grid item layout, and absolutely positioned children get a first
  same-page static-position path for positive and negative numeric explicit
  lines and positive/negative named explicit-line occurrences over fixed
  explicit tracks, including gaps between those tracks. Intrinsic placement
  replay preserves frozen pseudo child content, so those generated grid items
  retain their inline contents instead of becoming empty placeholders.
  contribution assignment and absolute static-position offsets share the same
  explicit grid line resolver for numeric lines, named occurrences, generated
  line names, crossed fixed-track gaps, and content-aligned final line
  offsets. Numeric/named implicit-line static positions on either side of the
  explicit grid can use definite cycled `grid-auto-columns`/`grid-auto-rows`
  tracks when the final same-page grid has not already created those lines.
  Same-page absolutely positioned grid children resolve explicit physical
  offsets against the grid-area containing block derived from those lines.
  A typed final-geometry grid positioning scope is active while a grid
  container remains an abspos descendant's containing block. In that case,
  direct children and qualifying descendants use the resolved grid-placement
  area, including padding-edge automatic placement lines. Otherwise a direct
  Grid child's static-position rectangle is the Grid content box and ignores
  its grid-placement properties. The selected rectangle remains independent
  of the actual containing block and passes through generic positioned layout
  unchanged, so self-alignment is resolved once after automatic intrinsic
  sizes are known. Orthogonal positioned items resolve an automatic physical
  height from their logical inline measure rather than their physical text
  advance.
  The Taffy-backed non-participating probe path also covers simple
  template-area, flexible-track, intrinsic-track, fixed-size `auto-fill`
  repeated-track, and template-area
  generated-line static positions on the same page. Numeric static-position
  offsets can also use the final same-page grid line geometry for
  start-aligned, positional content-aligned, and distributed content-aligned
  numeric/named `auto-fit` repeated tracks after empty-track collapse. Grid
  leaf nodes now answer Taffy's min-content, max-content, and preferred-size
  measure queries with
  Spindrift-measured intrinsic contributions for basic text, block, flex, table,
  and replaced-element content.
  Parent sizing paths can also query a first grid container intrinsic-width
  estimate for fixed and simple intrinsic explicit column tracks. Simple
  positive/negative one-track column placement, simple positive/negative
  named-line placement, simple positive spans, simple backward spans, simple
  named-line spans with one definite opposite edge, all-auto placement, and
  simple mixed auto/explicit column placement feed per-track item
  contributions into that estimate, including simple row- and column-dense
  auto backfill and simple column auto-flow within explicit tracks, plus
  column auto-flow and row auto-flow constrained by simple numeric or named
  row-line placement. Generated
  `area-start`/`area-end` column lines from `grid-template-areas` participate
  in that intrinsic placement resolution, and fixed-size auto-repeat
  contributes one repeated track list for indefinite intrinsic width queries.
  When there is no explicit column grid, simple all-auto placement and
  positive numeric/named implicit column lines can size implicit columns from
  cycled `grid-auto-columns` track lists. Positive numeric/named placements
  that extend an authored explicit column grid, plus forward named spans from
  definite start lines into the after-explicit implicit grid, also append
  cycled `grid-auto-columns` tracks. Backward named spans from a definite end
  line can synthesize startward implicit columns for intrinsic width estimates,
  using the backward-cycled `grid-auto-columns` pattern.
  Area-created explicit columns from `grid-template-areas` also use cycled
  auto-column sizes and generated area lines when `grid-template-columns` is
  absent or shorter than the area grid.
  In row flow, automatic items create only the implicit columns required by
  their largest span and any definite-row placement; subsequent automatic
  items fill newly created rows rather than adding columns to intrinsic width.
  Column-flow grids with no simple placed items avoid synthesizing an empty
  implicit column.
  Percentage explicit column track breadths are treated as `auto` for grid
  container intrinsic width contributions, and cyclic percentage column gaps
  resolve their percentage components to zero while preserving fixed length
  components.
  Row-axis `min-content` track sizing now receives the grid item's measured
  block-axis intrinsic contribution instead of a synthetic line-height cap, and
  `height:min-content` grids resolve cyclic percentage row gaps against zero
  while preserving fixed row-gap length components.
  Simple same-page fixed-track grids keep those zero-cyclic row/column gap
  contributions for intrinsic container sizing, then resolve final percentage
  gaps against the grid container content box when placing grid contents.
  Simple indefinite percentage grid item block sizes behave as automatic sizes
  instead of resolving against the grid inline size during min-content row
  sizing.
  Standard Grid and inline-grid now run CSS Grid's bounded row-to-column and
  column-to-row intrinsic-contribution feedback sequence. Each corrected pass
  uses the resolved grid area, including crossed gutters, as the item's
  definite percentage containing block; final replay uses the same area while
  preserving cyclic-percentage behavior during intrinsic sizing. Grid Lanes'
  distinct Level 3 packing path remains separate.
  Complete grid intrinsic sizing, grid-aware absolute static positions, full
  inline-grid baseline behavior, and fragmentation are not yet complete for
  grid.
- The Taffy grid adapter maps common CSS Box Alignment values for
  `align-content`, `justify-content`, `align-items`, `justify-items`,
  `align-self`, and `justify-self`. Same-page `align-content: space-evenly`
  and `justify-content: space-evenly` distribute fixed row and column tracks
  through Taffy. Spindrift applies a same-page post-layout pass for simple
  horizontal first- and last-baseline self-alignment between measured text
  items in the same row and for horizontal `justify-self`/`justify-items`
  `self-start`/`self-end` placement so LTR/RTL items align the subject edge
  required by their own inline direction, for horizontal
  `align-self`/`align-items` `self-start`/`self-end` placement so LTR/RTL
  items align the subject edge required by their own block-axis direction, and
  for horizontal
  `justify-self`/`justify-items` `left`/`right` so those keywords remain
  physical in LTR and RTL grids.
  Baseline requests whose Grid-item sizing depends on an intrinsic track are
  permanently excluded from sharing and use their required first/last
  start/end fallback during item replay, so they cannot affect an exported
  Grid baseline.
  Same-page horizontal grid container first and last exported baselines come
  from the first and last occupied grid rows for inline-grid and nested grid
  baseline-sharing cases, and simple same-page spanning items share first/last
  baselines with items on the same row edge. Full baseline shims during
  intrinsic track sizing, orthogonal baseline sharing, broader spanning
  baseline groups, and fragmentation interactions remain incomplete.

## Implementation Plan

- Continue broadening Grid Level 1 parsing, especially remaining `grid` and
  `grid-template` edge cases, full `grid-area` grammar coverage, and full
  grammar validation for unusual named-line cases.
- Build `src/layout/grid/` using the flex integration as the architectural
  model: child collection, Taffy adapter, intrinsic estimates, final layout
  records, item replay, absolute child static positions, and fragmentation
  planning are separate modules. The Taffy adapter now lives in
  `src/layout/grid/taffy_adapter.rs`, child collection now lives in
  `src/layout/grid/children.rs`, grid intrinsic estimates now live in
  `src/layout/grid/intrinsic.rs`, and item replay now lives in
  `src/layout/grid/replay.rs`; absolute child static-position handling now
  lives in `src/layout/grid/static_position.rs`. Fragmentation planning still
  needs further separation as that area matures.
- Keep Taffy isolated behind the grid adapter. CSS computed values and final
  layout records should stay Spindrift-native so PDF side effects, fragmentation,
  future subgrid support, and spec divergence handling remain under Spindrift's
  control.
- Reuse existing Spindrift machinery for anonymous text items, `display: contents`
  flattening, block/inline/table/flex child measurement, paint metadata,
  links, bookmarks, counters, and generated content.

## Tracking Checklist

- [x] CSS display model supports block and inline grid display values.
- [x] Grid computed-value structs model Level 1 track lists, template areas,
  auto tracks, auto-flow, and item placement.
- [ ] Grid declaration parsing is partially covered by parser tests;
  `grid-row`/`grid-column` including omitted `auto` end lines, basic
  `grid-area` including omitted custom-ident end values and invalid overlong
  forms, grid placement custom-ident validation for quoted/function/CSS-wide
  keyword cases, escaped custom-ident decoding for placement and bracketed
  track line names, escaped `grid-template-areas` string decoding,
  non-negative track breadth validation, auto-repeat
  fixed-size/no-nesting/single-auto-repeat validation, bracketed track
  line-name custom-ident and non-empty validation, and practical
  `grid-template` and `grid` shorthand expansion work, while full grammar coverage remains
  incomplete.
- [ ] Box-tree construction creates block grid boxes and atomic inline-grid
  boxes. The same-page inline-grid path reuses grid child collection, intrinsic
  estimates, Taffy layout, and item replay inside an atomic inline fragment;
  fragmented inline-grid and full exported-baseline behavior remain incomplete.
- [x] Grid child collection handles in-flow children, anonymous text items,
  generated content, `display: contents`, ordering, and out-of-flow children.
  Same-page in-flow children, non-whitespace anonymous text items,
  whitespace-only text suppression, `display: contents` flattening,
  order-modified placement, tree-abiding `::before`/`::after` generated grid
  items, and basic out-of-flow splitting work. Generated-content fragmentation
  and side-effect replay are tracked with grid fragmentation, and full
  out-of-flow static-position behavior is tracked with absolute positioning.
- [ ] Taffy adapter maps Spindrift grid values to Taffy and converts unrounded
  layouts back to Spindrift grid item records. Same-page row auto-flow, column
  auto-flow with implicit auto columns, and dense backfill for simple spanning
  items are covered by smoke tests. Implicit `grid-auto-rows` and
  `grid-auto-columns` track lists cycle in simple same-page cases. Same-page
  spanning over definite fixed tracks includes row and column gutters in
  replayed item geometry. Template areas generate `area-start` and `area-end`
  line names for same-page named-line placement. Horizontal RTL
  auto-placement starts at the inline-start/rightmost column. Definite
  same-page flexible `fr` tracks distribute available inline size for ordinary
  in-flow items. Fixed-size same-page `auto-fill` repeats and empty-track
  `auto-fit` collapse work for definite inline sizes. Absolutely positioned
  children can use a same-page fixed-size `auto-fill` repeated track static
  position through the probe path, and start-aligned, positional
  content-aligned, and distributed content-aligned numeric/named `auto-fit`
  static positions use collapsed repeated-track line geometry.
  Explicit `auto auto` rows and columns size to fixed-size item contributions
  in covered definite-size same-page grids. Simple same-page backward named
  spans from a definite end line and negative named line placements can prepend
  startward implicit tracks for non-repeat and finite numbered-repeat column
  and row grids, including area-created explicit tracks, and fixed-size
  definite `auto-fill` column and row grids. Simple backward named spans and
  negative named line placements also work before fixed-size definite
  `auto-fit` column and row grids, including empty-track collapse after
  startward implicit-track expansion. Positive named implicit line placement
  after fixed-size definite `auto-fill` and `auto-fit` repeats works on both
  axes with cycled auto tracks, including multi-track `auto-fill` repeated
  fragments and forward named spans after single-track `auto-fill` repeated
  fragments; writing-mode, broader auto-repeat combinations, and broader
  implicit-grid variants remain incomplete. Same-page
  `align-content: space-evenly` and `justify-content: space-evenly`
  distribute fixed tracks; complex auto-placement, intrinsic/flexible spanning
  effects, broader auto-repeat intrinsic sizing and static positions, broader
  RTL/direction interactions, orthogonal writing modes, and fragmentation
  remain incomplete.
- [ ] Intrinsic measurement reuses existing block, inline, table, flex, and
  replaced-element estimators for basic grid item leaf measurement. Taffy's
  same-page track-sizing queries now receive distinct min-content,
  max-content, and preferred contributions, including covered `min-content`,
  `max-content`, and `fit-content(<length>)` track basics. The grid container
  intrinsic-width estimator applies the `fit-content()` max track clamp as
  `min(max-content, max(auto-minimum, argument))` for covered explicit tracks.
  Per-track contribution assignment for simple non-spanning placed items,
  simple positive/negative numeric lines and named-line column starts/ends,
  simple mixed auto/explicit column placement, simple row- and column-dense
  intrinsic auto backfill, simple column auto-flow within explicit tracks,
  simple numeric and named-line definite-row constrained auto-flow, equal
  distribution of simple
  explicit numeric and named-line spanning-item contributions with one
  definite opposite edge after crossed gaps, and one-copy fixed-size
  auto-repeat contribution for indefinite intrinsic width queries. Simple
  all-auto implicit columns and positive numeric/named implicit
  column-line placements generated from `grid-template-columns:none` use
  cycled `grid-auto-columns` track sizes for intrinsic width contributions;
  positive numeric/named placements that extend authored explicit columns
  and forward named spans from definite start lines into the after-explicit
  implicit grid append the same cycled implicit track sizes. Area-created
  explicit columns from `grid-template-areas` do the same when
  `grid-template-columns` is absent or shorter than the area grid, and expose
  generated area lines to intrinsic placement.
  Percentage explicit column track breadths behave as `auto` for intrinsic
  width contributions, and cyclic percentage column gaps ignore the percentage
  component while preserving fixed length components. Cyclic percentage row
  gaps resolve against zero for simple same-page intrinsic grid height sizing.
  Simple indefinite percentage grid item block sizes behave as automatic sizes for min-content
  row sizing. Cyclic preferred, minimum, and maximum item percentages remain
  automatic for intrinsic track contributions, then resolve against the final
  grid area with final min/max constraints (including mixed `calc()` values).
  Explicit multi-track flexible spans receive Grid's zero automatic minimum,
  while replaced automatic used sizes are retained for final placement rather
  than incorrectly enlarging `minmax(auto, 0)` tracks. Complete grid
  contribution rules, broader implicit-grid placement, broader spanning-item
  effects, broader generated-area interactions, flexible tracks, definite
  auto-repeat intrinsic expansion, and full container intrinsic sizing remain
  incomplete.
- [ ] Same-page grid layout paints normal-flow grid items, backgrounds,
  borders, links, bookmarks, counters, generated content, and nested
  formatting contexts correctly.
- [ ] Inline-grid participates as an atomic inline on the same-page path and
  can export a first baseline from the first occupied grid row. Nested
  same-page horizontal grid containers can export first and last baselines
  from occupied grid rows for parent grid baseline-sharing groups. Full Grid
  baseline synthesis/sharing, writing-mode handling, abspos interactions, and
  fragmentation remain incomplete.
- [ ] Absolutely positioned grid descendants use grid-aware static-position
  inputs. Positive and negative numeric explicit lines and positive/negative
  named explicit-line occurrences over fixed explicit tracks work on the
  same-page path, including gaps. Simple template-area, flexible-track,
  intrinsic-track, fixed-size `auto-fill` repeated-track, template-area
  generated-line static positions, and start-aligned, positional
  content-aligned, and distributed content-aligned numeric/named `auto-fit`
  collapsed repeated-line static positions work on both axes on the same-page
  path. Fixed explicit-line static positions honor content alignment, and numeric/named
  implicit-line static positions on either side of the explicit grid can use
  definite cycled `grid-auto-*` tracks, including after-explicit row and
  column line-edge offsets and covered horizontal static-rectangle end edges
  outside the grid container without including the following implicit gutter.
  Explicit physical offsets on same-page positioned grid children resolve
  against the covered grid-area containing block rather than the whole grid
  container.
  Named explicit lines inside finite numbered
  repeats and named fixed-size `auto-fill` and collapsed `auto-fit` repeated
  lines, including multi-track repeated fragments and before-/after-explicit
  named implicit-line offsets with definite `grid-auto-*` tracks, work on both
  axes; broader implicit-line combinations and fragmented static positions
  remain incomplete.
- [ ] Normal-flow block grid containers fragment in paged media at row-track
  boundaries, with oversized item slices and page-local side-effect metadata.
- [ ] Grid-specific box alignment behavior is partially implemented through
  Taffy's common alignment model. Simple same-page horizontal first- and
  last-baseline self-alignment for text items in the same row is corrected
  with Spindrift-owned baseline metadata, and same-page horizontal
  `justify-self`/`justify-items` `self-start`/`self-end` placement follows
  the grid item's own LTR/RTL inline direction, same-page horizontal
  `align-self`/`align-items` `self-start`/`self-end` follows the grid item's
  own LTR/RTL block-axis direction, while
  `justify-self`/`justify-items` `left`/`right` stays physical in LTR/RTL
  grids; same-page horizontal grid container first/last exported baselines work
  for occupied-row baseline-sharing cases, and simple spanning items share
  baselines by start/end row edge. Cyclic intrinsic-track baseline requests use
  their required start/end fallback and are excluded from sharing and exported
  baselines. Full baseline shims during intrinsic track sizing, broader
  spanning baseline groups, orthogonal baseline sharing, and fragmentation
  interactions remain incomplete and should also be tracked in
  `docs/CSS_BOX_ALIGNMENT_PARITY.md`.
- [ ] `SPEC_DIVERGENCES.md` accurately lists all remaining grid divergences.

## Known Divergences

- Grid layout is not yet implemented end to end. `layout_grid` handles a
  first same-page normal-flow path, but incomplete adapter coverage can still
  fall back to block layout and many Grid Level 1 behaviors are missing.
- Grid intrinsic sizing is incomplete. Taffy receives Spindrift-measured
  min-content, max-content, and preferred leaf contributions for basic grid
  items, which covers simple `min-content`, `max-content`, and
  `fit-content(<length>)` explicit tracks, including the covered
  `fit-content()` intrinsic max-size clamp. It also covers per-track
  contributions from simple non-spanning placed items, simple positive
  named-line column starts/ends, simple mixed auto/explicit column placement,
  simple row- and column-dense intrinsic auto backfill, simple column
  auto-flow within explicit tracks, simple numeric and named-line
  definite-row constrained auto-flow, and equal
  distribution of simple explicit numeric and named-line spanning-item
  contributions with one definite opposite edge after crossed gaps, including
  covered generated `area-start`/`area-end` column lines from
  `grid-template-areas`. Row `min-content` tracks use measured
  block-axis item contributions for simple same-page items. Percentage
  explicit column tracks behave as `auto`, and cyclic percentage column gaps
  resolve their percentage component to zero, for intrinsic width
  contributions. Simple all-auto implicit columns and positive numeric/named
  implicit column-line placements can contribute cycled `grid-auto-columns`
  track sizes, including positive numeric/named placements that extend an
  authored explicit column grid and forward named spans from definite start
  lines into the after-explicit implicit grid, plus backward named spans from a
  definite end line into the startward implicit grid; area-created explicit
  columns from `grid-template-areas` also use cycled `grid-auto-columns` sizes and
  generated area lines in this path when explicit column tracks are absent or
  shorter than the area grid. Broader implicit-grid placement, spanning-item
  effects, automatic minimum sizing, flexible tracks, and full grid container
  intrinsic contributions can still diverge from CSS Grid. A first container
  intrinsic-width path exists for fixed and simple intrinsic explicit column
  tracks, plus one fixed-size auto-repeat copy for indefinite intrinsic width
  queries.
- Grid shorthand parsing is only partially implemented. `grid-row` and
  `grid-column` work for explicit and omitted-end forms, `grid-area` works for
  one- through four-value forms including omitted custom-ident end values,
  invalid grid placement line-name tokens are rejected in covered longhand and
  shorthand paths, escaped grid custom idents are decoded before computed
  placement/line-name storage, bracketed track line-name tokens are validated
  as custom-ident values and empty bracketed line-name lists are rejected.
  Negative track breadth length-percentages are rejected in covered bare track,
  `minmax()`, `fit-content()`, and auto-repeat paths. Escaped
  `grid-template-areas` strings are decoded before area tokenization,
  `auto-fill`/`auto-fit` repeat syntax rejects
  nested repeats, flexible auto-repeat tracks, and multiple auto-repeats, and
  practical `grid-template` and `grid` forms work; broader edge cases remain.
- Named-line placement syntax is only partially covered by tests. Named-line
  occurrences and template-area generated `area-start`/`area-end` lines work
  in same-page placement, including escaped names that reference digit-starting
  template areas, but unusual invalid cases need broader parser coverage.
- `subgrid` is not implemented.
- Masonry layout is not implemented.
- Complete grid fragmentation, including committed-slice item replay,
  forced/avoid break precedence, cloned decorations, side-effect replay, and
  column-axis fragment metadata, is not implemented.
- Grid baseline alignment and exported baselines are only partially
  implemented. Same-page simple horizontal first- and last-baseline
  self-alignment for text items in the same row uses measured item baselines,
  same-page horizontal `justify-self`/`justify-items` `self-start`/`self-end`
  follows the grid item's own LTR/RTL inline direction, same-page horizontal
  `align-self`/`align-items` `self-start`/`self-end` follows the grid item's
  own LTR/RTL block-axis direction, same-page horizontal
  `justify-self`/`justify-items` `left`/`right` stays physical in LTR/RTL
  grids, and same-page horizontal grid containers export first/last baselines
  from occupied rows for inline-grid and nested grid baseline-sharing cases.
  Simple spanning items share baselines with same-start-row or same-end-row
  peers, but full Grid baseline synthesis, broader spanning baseline groups,
  orthogonal writing-mode cases, and fragmentation are incomplete.
- Grid-aware static positions for absolutely positioned descendants are only
  partially implemented for same-page positive/negative numeric explicit-line
  and positive/negative named explicit-line placement over fixed explicit
  tracks, including gaps and content alignment; this includes descendants
  whose effective containing block is the grid container. Positive numeric
  implicit lines can use definite cycled `grid-auto-*` tracks. Same-page template-area,
  simple flexible-track, intrinsic-track, fixed-size auto-repeat, and
  template-area generated-line placements are covered through Taffy
  non-participating probe layout.

## Test Backlog

- CSS parser tests for display values, track lists, `repeat()`, `minmax()`,
  `fit-content()`, `fr`, template areas, broader placement edge cases, and
  invalid declarations. `grid-area` one- through four-value expansion,
  omitted custom-ident end values, invalid overlong shorthand rejection, and
  invalid grid placement custom-ident tokens now have parser coverage.
  Track line-name parser coverage includes invalid reserved, CSS-wide, and
  function-like custom-ident tokens, plus escaped custom-ident decoding for
  grid placement and bracketed track line names, and empty bracketed
  line-name lists. Track breadth parser coverage includes invalid definitely
  negative bare tracks, `minmax()` breadths, `fit-content()` arguments, and
  auto-repeat track fragments. Escaped `grid-template-areas` string decoding
  is covered for longhand and `grid-template` shorthand parsing. Auto-repeat
  parser coverage includes valid fixed-size `auto-fill`/`auto-fit` forms and
  invalid nested, flexible-track, negative fixed-track, and
  multiple-auto-repeat forms.
- Layout smoke tests now cover explicit `grid-template-columns: auto auto` and
  `grid-template-rows: auto auto` sizing in definite grids, plus same-page
  grid `align-content: space-evenly` and `justify-content: space-evenly`
  distribution for fixed tracks. Fixed-size `repeat(auto-fill, ...)` expansion
  and `repeat(auto-fit, ...)` empty-track collapse in definite same-page grids
  are also covered, as is same-page fixed-size auto-fill abspos static
  positioning and start-aligned, positional content-aligned, and distributed
  content-aligned numeric/named auto-fit collapsed repeated-line abspos static
  positioning. Abspos static-position smoke tests also cover content-aligned
  fixed explicit lines, explicit physical offsets resolved against grid-area
  containing blocks, and numeric/named implicit lines on either side of the
  explicit grid sized by definite cycled `grid-auto-columns`.
- Layout smoke tests now cover `width:min-content` grid containers using the
  CSS Grid indefinite-size fallback of one fixed-size `auto-fill` repetition.
  `width:min-content` grid containers also cover per-track intrinsic
  contributions for simple non-spanning items placed in different intrinsic
  columns, simple mixed auto/explicit column placement, simple row- and
  column-dense intrinsic auto backfill, simple column auto-flow within
  explicit tracks, simple numeric and named-line definite-row constrained
  auto-flow, simple
  positive/negative numeric and named-line column starts, and simple explicit
  numeric and named-line spanning-item contribution distribution with one definite
  opposite edge, including generated `area-start`/`area-end` column lines from
  `grid-template-areas` and area-created explicit columns sized by
  `grid-auto-columns` when `grid-template-columns` is absent or shorter than
  the area grid, plus positive numeric/named implicit columns that extend an
  authored explicit column grid, forward named implicit column spans, and
  backward named spans that synthesize startward implicit columns from a
  definite end line for intrinsic width estimates and simple same-page
  non-repeat and finite numbered-repeat column and row grids, including
  area-created explicit tracks and shifted generated area lines, plus
  fixed-size definite `auto-fill` column and row grids, plus simple fixed-size
  definite `auto-fit` column and row grids with empty-track collapse after
  startward implicit-track expansion. Positive named implicit line placement
  after fixed-size definite `auto-fill` and `auto-fit` repeats is covered on
  both axes with cycled auto tracks, including multi-track `auto-fill` repeated
  fragments and forward named spans after single-track `auto-fill` repeated
  fragments. Broader same-page rendered placement for backward named implicit
  spans remains incomplete with writing modes, broader auto-repeat
  combinations, and more complex implicit-grid combinations.
- Layout smoke tests now cover `grid-template-rows: min-content` using a
  measured block-axis item contribution larger than the item's line-height, and
  an indefinite percentage grid item block-size falling back to automatic
  sizing for min-content row contribution. They also cover a min-content item
  spanning two intrinsic rows, and `height:min-content` grids resolving cyclic
  percentage row gaps against zero. Definite pure percentage grid-item sizes
  now remain percentages through the Grid adapter so Taffy resolves them
  against the final grid area rather than eagerly against the grid container.
- Layout smoke tests for more spanning-item combinations, intrinsic/flexible
  spanning effects, intrinsic tracks, auto-track list cycling, named areas,
  broader RTL/direction cases, vertical writing modes, fragmented
  generated-content grid items, inline-grid baselines, and nested
  grid/flex/table cases. Same-page fixed-track spanning across row and column
  gaps, definite-width `fr` distribution, simple `min-content`/`max-content`/
  `fit-content(<length>)` track sizing, `fit-content()` intrinsic max-size
  clamping, implicit `grid-auto-rows` and `grid-auto-columns` track-list
  cycling, horizontal RTL auto-placement, auto-placement row/column/dense,
  fixed-size auto-repeat expansion and empty-track collapse, fixed-size
  auto-fill intrinsic one-repeat fallback, simple per-track intrinsic
  contribution assignment, mixed auto/explicit intrinsic contribution
  assignment, simple row- and column-dense intrinsic contribution backfill,
  simple column-auto-flow intrinsic contribution assignment, simple spanning
  intrinsic contribution distribution, intrinsic contribution assignment
  through template-area generated named column lines, same-page fixed-size
  auto-fill and two-axis start-aligned, positional content-aligned, and
  distributed content-aligned numeric/named auto-fit repeated-track abspos
  static positioning, content-aligned fixed-line, finite numbered-repeat named-line,
  named fixed-size auto-fill and collapsed auto-fit including multi-track
  repeated fragments and before-/after-explicit named implicit-line offsets,
  and numeric/named implicit-line abspos static positioning on either side of
  the explicit grid, including after-explicit row and column lines outside the grid
  container, positive numeric/named
  implicit-row abspos static
  positioning from final same-page grid offsets, negative numeric/named
  implicit-row abspos static positioning with normal-flow peers in the same
  startward implicit row, template-area generated named lines, escaped
  generated line names for digit-starting
  template areas with escaped template-area string tokens, simple same-page
  backward named startward implicit column and row placement through
  area-created explicit grids, finite numbered repeats, fixed-size definite
  `auto-fill`, and simple fixed-size definite `auto-fit` including negative
  named line starts and collapsed trailing empty repeated tracks after
  startward implicit-track expansion, anonymous text items,
  CSS document-whitespace-only text suppression, `display: contents`,
  order sorting, same-page abspos fixed/named/flexible/intrinsic track and
  template-area generated-line static positions, and tree-abiding generated
  pseudo grid item basics now have smoke coverage.
- Paged-media smoke tests for grid containers across pages, forced breaks
  between row bands, oversized item clipping, positioned descendants, links,
  bookmarks, counters, and generated content inside grid items.
- WPT imports grouped by placement, track sizing, intrinsic sizing, alignment,
  writing modes, absolute positioning, and fragmentation.

## Architecture Notes

- Prefer reusing flex's adapter pattern over introducing a second layout
  abstraction: estimate children in Spindrift, compute item geometry with Taffy,
  then replay children through Spindrift layout and paint paths.
- Keep the Taffy conversion layer in `src/layout/grid/taffy_adapter.rs` so
  Grid computed values, item collection, replay, side effects, and future
  fragmentation metadata stay Spindrift-owned.
- Keep grid child collection in `src/layout/grid/children.rs` so anonymous
  text items, `display: contents`, order sorting, out-of-flow splitting, and
  static-position probes share one code path across block grid and
  `inline-grid`.
- Keep grid intrinsic measurement in `src/layout/grid/intrinsic.rs` so Taffy
  leaf measurement, container shrink-to-fit sizing, and future track-sizing
  contributions use the same Spindrift-owned estimator.
- Keep grid item replay in `src/layout/grid/replay.rs` so block grid and
  `inline-grid` use the same style normalization, side-effect, and child
  formatting-context path after Taffy has produced item geometry.
- Scope same-page grid item paint through `StackingContextPolicy::for_grid_item`
  after replay. This preserves CSS Grid's rule that a static grid item with a
  non-auto `z-index` establishes a stacking context, and keeps its ordering
  consistent with fragmented Grid and Flex item replay.
- Keep grid-aware absolute static-position handling in
  `src/layout/grid/static_position.rs` so explicit-line offsets,
  non-participating probe layouts, and future fragmented static-position
  behavior can evolve without crowding normal in-flow grid layout.
- Keep explicit grid line resolution in `src/layout/grid/line_resolution.rs`
  so intrinsic contribution assignment and absolute static-position offsets
  cannot drift for numeric lines, named-line occurrences, generated names, or
  crossed gaps.
- Keep Grid Lanes placement in `src/layout/grid/lanes.rs`: it consumes the
  resolved grid-axis track geometry, measures items after their lane span is
  known, and owns the independent stacking-axis cursor, direction reversal,
  content alignment, grid-axis track-gap distribution, and dense-backfill
  state. Level 3 track sizing and the remaining alignment behavior must extend
  this boundary rather than feeding lane items back through ordinary
  two-dimensional auto-placement.
- The all-auto column-lane path derives one min-content hypothetical
  contribution for each eligible auto track and applies positive free-space
  stretching before positional alignment. Nested grids still use the shared
  Grid pass until subgrid contribution propagation is represented explicitly.
- The single-track intrinsic auto-repeat path builds a hypothetical
  max/min-content contribution before selecting the repetition count. It keeps
  percentage-sized items indefinite during that phase, includes simple spans,
  and resolves those item percentages only after their final lane area is
  known.
- Grid item percentage sizing has two deliberate phases shared by ordinary
  Grid and Grid Lanes: intrinsic track sizing treats cyclic percentages as
  automatic, while final area placement resolves preferred/minimum/maximum
  constraints and replay retains those physical used bounds.
- Keep grid placement and track data in logical grid terms until the adapter
  maps through writing mode and direction into physical container coordinates.
- Use explicit grid layout records rather than recovering structure from paint
  output. Fragmentation, alignment, links, bookmarks, and future tagged PDF
  support need durable layout metadata.
- When Taffy differs from CSS Grid or lacks a required paged-media behavior,
  correct the result in Spindrift-owned post-processing and record the divergence
  until it is fully resolved.
