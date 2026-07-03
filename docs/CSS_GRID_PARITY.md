# CSS Grid Parity

Last updated: 2026-07-08

This document tracks Quire's CSS Grid implementation. It is a living
tracking document: update it whenever grid behavior is added, narrowed,
deferred, or found to diverge from the specs. The normative references are
CSS Grid Layout Level 1, CSS Grid Layout Level 2, CSS Box Alignment Level 3,
CSS Display Level 3, CSS Writing Modes Level 4, CSS Sizing Level 3, and CSS
Fragmentation Level 3. WeasyPrint behavior is useful compatibility context,
but spec conformance is the priority when they disagree.

## Current Implementation

- Quire has grid-specific geometry scaffolding in `src/layout/grid.rs`,
  including typed grid coordinates and projection into page-top paint space.
- Taffy 0.11.0 is already a dependency and its default feature set includes
  CSS Grid support. Grid integration should use Taffy for the core Grid Level
  1 placement and track-sizing algorithm where its public model matches the
  CSS model.
- CSS display parsing now accepts `grid`, `inline-grid`, `block grid`,
  `inline grid`, and `run-in grid`. Grid boxes are recognized as independent
  formatting contexts.
- Computed values now carry Grid Level 1 track lists, template areas, auto
  tracks, auto-flow, and item placement longhands. The first parser pass
  covers practical explicit tracks, `repeat()`, `auto-fill`/`auto-fit`
  auto-repeat with fixed-size validation, `minmax()`, `fit-content()`, `fr`,
  template areas, auto tracks, auto-flow, and basic line/span placement.
  `grid-row` and `grid-column` shorthand expansion is also
  implemented, including omitted-end forms that expand to `auto`, and
  `grid-area` shorthand expansion works for basic placement values. Named line
  occurrence syntax such as `main 2` and `span main 2` parses and is preserved
  through Taffy placement. `grid-area` shorthand expansion covers one- through
  four-value forms, omitted custom-ident end values, and invalid overlong
  shorthand rejection. Grid placement line names now reject non-identifier
  tokens and CSS-wide/custom-reserved keywords in both longhands and
  shorthands, and bracketed track line names use the same validation.
  `grid-template` shorthand expansion supports `none`,
  `<track-list> / <track-list>`, and string-based template rows with optional
  row sizes and column tracks. `grid` shorthand expansion supports explicit
  template forms and both row/column `auto-flow` branches.
  `grid-template-areas` rejects non-rectangular named areas and invalid cell
  tokens.
- Layout dispatch has a dedicated grid entrypoint. The first same-page path
  collects normal-flow grid items, runs a Taffy-backed Grid Level 1 layout,
  and replays each item through Quire's existing block, inline, table, flex,
  replaced, paint, and side-effect machinery. Ordinary same-page flow grid
  items establish an independent formatting context for measurement and
  replay, so descendant block margins do not collapse through the grid item.
  Grid child collection shares the blockified formatting-context itemization
  path with flex, including anonymous text item construction, whitespace-only
  text suppression, order sorting, out-of-flow splitting, and blockified
  inline-source paint normalization.
  `inline-grid` boxes now use the same child collection, intrinsic estimate,
  Taffy geometry, and item replay path inside an atomic inline fragment, with
  a first exported baseline from rendered grid item text on the same-page path.
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
  content alignment. Flexible `fr` tracks distribute definite container width for
  basic same-page normal-flow grids. Child collection creates anonymous grid items for
  non-whitespace text runs, ignores whitespace-only text runs, and uses existing box-tree
  `display: contents` flattening so flattened children participate as grid
  items. Tree-abiding `::before` and `::after`
  generated boxes participate as same-page grid items, including
  order-modified auto-placement with real children. Out-of-flow children are
  split out of normal grid item layout, and absolutely positioned children get a first
  same-page static-position path for positive and negative numeric explicit
  lines and positive/negative named explicit-line occurrences over fixed
  explicit tracks, including gaps between those tracks. Intrinsic placement
  contribution assignment and absolute static-position offsets share the same
  explicit grid line resolver for numeric lines, named occurrences, generated
  line names, and crossed fixed-track gaps. The Taffy-backed
  non-participating probe path also covers simple template-area,
  flexible-track, intrinsic-track, fixed-size `auto-fill` repeated-track, and
  template-area generated-line static positions on the same page. Grid leaf
  nodes now answer Taffy's
  min-content, max-content, and preferred-size measure queries with
  Quire-measured intrinsic contributions for basic text, block, flex, table,
  and replaced-element content.
  Parent sizing paths can also query a first grid container intrinsic-width
  estimate for fixed and simple intrinsic explicit column tracks. Simple
  positive/negative one-track column placement, simple positive/negative
  named-line placement, simple positive spans, and all-auto placement feed
  per-track item contributions into that estimate, and fixed-size auto-repeat
  contributes one repeated track list for indefinite intrinsic width queries. Complete grid
  intrinsic sizing, grid-aware absolute static positions, full inline-grid
  baseline behavior, and fragmentation are
  not yet complete for grid.
- The Taffy grid adapter maps common CSS Box Alignment values for
  `align-content`, `justify-content`, `align-items`, `justify-items`,
  `align-self`, and `justify-self`. Same-page `align-content: space-evenly`
  and `justify-content: space-evenly` distribute fixed row and column tracks
  through Taffy. Baseline alignment, `self-start`/`self-end` writing-mode
  details, and fragmentation interactions remain incomplete.

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
  layout records should stay Quire-native so PDF side effects, fragmentation,
  future subgrid support, and spec divergence handling remain under Quire's
  control.
- Reuse existing Quire machinery for anonymous text items, `display: contents`
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
  keyword cases, auto-repeat fixed-size/no-nesting/single-auto-repeat
  validation, bracketed track line-name custom-ident validation, and practical
  `grid-template` and `grid` shorthand expansion work, while full grammar
  coverage remains incomplete.
- [ ] Box-tree construction creates block grid boxes and atomic inline-grid
  boxes. The same-page inline-grid path reuses grid child collection, intrinsic
  estimates, Taffy layout, and item replay inside an atomic inline fragment;
  fragmented inline-grid and full exported-baseline behavior remain incomplete.
- [ ] Grid child collection handles in-flow children, anonymous text items,
  generated content, `display: contents`, ordering, and out-of-flow children.
  Same-page in-flow children, non-whitespace anonymous text items,
  whitespace-only text suppression, `display: contents` flattening,
  order-modified placement, tree-abiding `::before`/`::after` generated grid
  items, and basic out-of-flow splitting work; generated-content fragmentation
  and side-effect edge cases, full static-position behavior, and fragmentation
  behavior remain incomplete.
- [ ] Taffy adapter maps Quire grid values to Taffy and converts unrounded
  layouts back to Quire grid item records. Same-page row auto-flow, column
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
  position through the probe path. Explicit `auto auto` rows and columns size
  to fixed-size item contributions in covered definite-size same-page grids. Same-page
  `align-content: space-evenly` and `justify-content: space-evenly`
  distribute fixed tracks; complex auto-placement, intrinsic/flexible spanning
  effects, broader auto-repeat intrinsic sizing and static positions, broader
  RTL/direction interactions, orthogonal writing modes, and fragmentation
  remain incomplete.
- [ ] Intrinsic measurement reuses existing block, inline, table, flex, and
  replaced-element estimators for basic grid item leaf measurement. Taffy's
  same-page track-sizing queries now receive distinct min-content,
  max-content, and preferred contributions, including covered `min-content`,
  `max-content`, and `fit-content(<length>)` track basics, per-track
  contribution assignment for simple non-spanning placed items, simple
  positive/negative numeric lines and named-line column starts/ends, equal
  distribution of simple explicit positive spanning-item contributions after
  crossed gaps, and one-copy fixed-size auto-repeat contribution for
  indefinite intrinsic width queries; complete grid contribution rules,
  broader spanning-item effects, named-line spans/generated area-line
  contributions, flexible tracks, automatic minimum sizing, definite
  auto-repeat intrinsic expansion, and full container intrinsic sizing remain
  incomplete.
- [ ] Same-page grid layout paints normal-flow grid items, backgrounds,
  borders, links, bookmarks, counters, generated content, and nested
  formatting contexts correctly.
- [ ] Inline-grid participates as an atomic inline on the same-page path and
  can export a baseline from rendered grid item text. Full Grid baseline
  synthesis/sharing, writing-mode handling, abspos interactions, and
  fragmentation remain incomplete.
- [ ] Absolutely positioned grid descendants use grid-aware static-position
  inputs. Positive and negative numeric explicit lines and positive/negative
  named explicit-line occurrences over fixed explicit tracks work on the
  same-page path, including gaps. Simple template-area, flexible-track,
  intrinsic-track, fixed-size `auto-fill` repeated-track, and template-area
  generated-line static positions work through a Taffy non-participating probe
  layout; broader auto-fit/named-repeat, implicit-line edge cases and
  fragmented static positions remain incomplete.
- [ ] Normal-flow block grid containers fragment in paged media at row-track
  boundaries, with oversized item slices and page-local side-effect metadata.
- [ ] Grid-specific box alignment behavior is partially implemented through
  Taffy's common alignment model; baseline alignment, self-start/self-end
  writing-mode details, and fragmentation interactions remain incomplete and
  should also be tracked in `docs/CSS_BOX_ALIGNMENT_PARITY.md`.
- [ ] `SPEC_DIVERGENCES.md` accurately lists all remaining grid divergences.

## Known Divergences

- Grid layout is not yet implemented end to end. `layout_grid` handles a
  first same-page normal-flow path, but incomplete adapter coverage can still
  fall back to block layout and many Grid Level 1 behaviors are missing.
- Grid intrinsic sizing is incomplete. Taffy receives Quire-measured
  min-content, max-content, and preferred leaf contributions for basic grid
  items, which covers simple `min-content`, `max-content`, and
  `fit-content(<length>)` explicit tracks, per-track contributions from simple
  non-spanning placed items, simple positive named-line column starts/ends,
  and equal distribution of simple explicit positive spanning-item
  contributions after crossed gaps. Broader spanning-item effects, automatic
  minimum sizing, flexible tracks, and full grid container intrinsic
  contributions can still diverge from CSS Grid. A first container
  intrinsic-width path exists for fixed and simple intrinsic explicit column
  tracks, plus one fixed-size auto-repeat copy for indefinite intrinsic width
  queries.
- Grid shorthand parsing is only partially implemented. `grid-row` and
  `grid-column` work for explicit and omitted-end forms, `grid-area` works for
  one- through four-value forms including omitted custom-ident end values,
  invalid grid placement line-name tokens are rejected in covered longhand and
  shorthand paths, bracketed track line-name tokens are validated as
  custom-ident values, `auto-fill`/`auto-fit` repeat syntax rejects nested
  repeats, flexible auto-repeat tracks, and multiple auto-repeats, and
  practical `grid-template` and `grid` forms work; broader edge cases remain.
- Named-line placement syntax is only partially covered by tests. Named-line
  occurrences and template-area generated `area-start`/`area-end` lines work
  in same-page placement, but unusual invalid and escaped-name cases need
  broader parser coverage.
- `subgrid` is not implemented.
- Masonry layout is not implemented.
- Complete grid fragmentation, including forced/avoid break precedence,
  cloned decorations, side-effect replay, and row/column fragment metadata,
  is not implemented.
- Grid baseline alignment and exported baselines are only partially
  implemented. Same-page `inline-grid` can export a baseline from rendered grid
  item text, but full Grid baseline synthesis, first/last baseline sets,
  writing-mode cases, and fragmentation are incomplete.
- Grid-aware static positions for absolutely positioned descendants are only
  partially implemented for same-page positive/negative numeric explicit-line
  and positive/negative named explicit-line placement over fixed explicit
  tracks, including gaps. Same-page template-area, simple flexible-track,
  intrinsic-track, and template-area generated-line placements are covered
  through Taffy non-participating probe layout.

## Test Backlog

- CSS parser tests for display values, track lists, `repeat()`, `minmax()`,
  `fit-content()`, `fr`, template areas, broader placement edge cases, and
  invalid declarations. `grid-area` one- through four-value expansion,
  omitted custom-ident end values, invalid overlong shorthand rejection, and
  invalid grid placement custom-ident tokens now have parser coverage.
  Track line-name parser coverage includes invalid reserved, CSS-wide, and
  function-like custom-ident tokens. Auto-repeat parser coverage includes valid
  fixed-size `auto-fill`/`auto-fit` forms and invalid nested, flexible-track,
  and multiple-auto-repeat forms.
- Layout smoke tests now cover explicit `grid-template-columns: auto auto` and
  `grid-template-rows: auto auto` sizing in definite grids, plus same-page
  grid `align-content: space-evenly` and `justify-content: space-evenly`
  distribution for fixed tracks. Fixed-size `repeat(auto-fill, ...)` expansion
  and `repeat(auto-fit, ...)` empty-track collapse in definite same-page grids
  are also covered, as is same-page fixed-size auto-fill abspos static
  positioning.
- Layout smoke tests now cover `width:min-content` grid containers using the
  CSS Grid indefinite-size fallback of one fixed-size `auto-fill` repetition.
  `width:min-content` grid containers also cover per-track intrinsic
  contributions for simple non-spanning items placed in different intrinsic
  columns, simple positive/negative numeric and named-line column starts, and
  simple explicit positive spanning-item contribution distribution.
- Layout smoke tests for more spanning-item combinations, intrinsic/flexible
  spanning effects, intrinsic tracks, auto-track list cycling, named areas,
  broader RTL/direction cases, vertical writing modes, fragmented
  generated-content grid items, inline-grid baselines, and nested
  grid/flex/table cases. Same-page fixed-track spanning across row and column
  gaps, definite-width `fr` distribution, simple `min-content`/`max-content`/
  `fit-content(<length>)` track sizing, implicit `grid-auto-rows` and
  `grid-auto-columns` track-list cycling, horizontal RTL auto-placement,
  auto-placement row/column/dense, fixed-size auto-repeat expansion and
  empty-track collapse, fixed-size auto-fill intrinsic one-repeat fallback,
  simple per-track intrinsic contribution assignment, simple spanning
  intrinsic contribution distribution,
  same-page fixed-size auto-fill abspos static positioning, template-area generated named lines,
  anonymous text items, whitespace-only text suppression, `display: contents`,
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
  abstraction: estimate children in Quire, compute item geometry with Taffy,
  then replay children through Quire layout and paint paths.
- Keep the Taffy conversion layer in `src/layout/grid/taffy_adapter.rs` so
  Grid computed values, item collection, replay, side effects, and future
  fragmentation metadata stay Quire-owned.
- Keep grid child collection in `src/layout/grid/children.rs` so anonymous
  text items, `display: contents`, order sorting, out-of-flow splitting, and
  static-position probes share one code path across block grid and
  `inline-grid`.
- Keep grid intrinsic measurement in `src/layout/grid/intrinsic.rs` so Taffy
  leaf measurement, container shrink-to-fit sizing, and future track-sizing
  contributions use the same Quire-owned estimator.
- Keep grid item replay in `src/layout/grid/replay.rs` so block grid and
  `inline-grid` use the same style normalization, side-effect, and child
  formatting-context path after Taffy has produced item geometry.
- Keep grid-aware absolute static-position handling in
  `src/layout/grid/static_position.rs` so explicit-line offsets,
  non-participating probe layouts, and future fragmented static-position
  behavior can evolve without crowding normal in-flow grid layout.
- Keep explicit grid line resolution in `src/layout/grid/line_resolution.rs`
  so intrinsic contribution assignment and absolute static-position offsets
  cannot drift for numeric lines, named-line occurrences, generated names, or
  crossed gaps.
- Keep grid placement and track data in logical grid terms until the adapter
  maps through writing mode and direction into physical container coordinates.
- Use explicit grid layout records rather than recovering structure from paint
  output. Fragmentation, alignment, links, bookmarks, and future tagged PDF
  support need durable layout metadata.
- When Taffy differs from CSS Grid or lacks a required paged-media behavior,
  correct the result in Quire-owned post-processing and record the divergence
  until it is fully resolved.
