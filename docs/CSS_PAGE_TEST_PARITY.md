# CSS Page Test Parity

This note tracks renderer parity for the CSS Page WPT reftests. It records
current behavior rather than treating harness thresholds as renderer support.

## Current baseline

On 2026-07-24, the local `quire-wpt` raw run of `css/CSS2/pagination/`
rendered 43 reftests:

- 42 passed;
- 1 failed.

The remaining raw mismatch, `row-page-break-inside-avoid-2-print.html`, is a
legacy reference incompatibility rather than a renderer target. Its source
table has no header row, but its assigned reference introduces a `<thead>`
whose `page-break-after: always` creates different page content. The same
case fails in all four configured engines. Quire deliberately preserves the
source table's CSS Fragmentation behavior instead of adding a test-specific
header/repetition exception; see `SPEC_DIVERGENCES.md`.

On 2026-07-18, the local `quire-wpt` raw run of `css/css-page/` rendered 226
reftests with the development binary and an equivalent author `@page` rule:

- 175 passed;
- 51 failed.

The run artifacts are kept under
`/private/tmp/quire-css-page-logical-page-basis/` for this working session.
This is three renderer matches above the preceding 172/226 baseline:
`monolithic-overflow-012-print.html` now retains the final fragmented float
slice, and `monolithic-overflow-013-print.html` now retains background-only
absolute-positioned fragments before its first text line.
`margin-boxes/alignment-001-print.html` also now matches after the current
margin-box alignment work.

On 2026-07-30, an isolated fresh `quire-wpt` run exercised all fourteen CSS
Page fixtures from the root-cause group with the release binary:

- 14 passed;
- 0 failed.

The run artifacts are under `/private/tmp/quire-wpt-css-page-pictured-final/`
for this working session. HTML presentational hints are now unconditional
renderer behavior rather than a CLI option.

The corresponding fresh full `css/css-page/` reftest run exercised 226
non-script tests with the same isolated Quire configuration:

- 116 passed;
- 110 failed.

This run is a fresh harness baseline, not directly comparable with the
2026-07-18 multi-engine/development setup above. Its artifacts are under
`/private/tmp/quire-wpt-css-page-full-final/` for this working session.

On 2026-08-04, fresh exact `quire-wpt evaluate-test` runs rendered
`fixedpos-001-print.html`, `fixedpos-002-print.html`, and
`fixedpos-004-print.html` as raster-exact matches.

On 2026-08-01, fresh exact `quire-wpt evaluate-test` runs rendered the six
CSS2 `page-break-inside: avoid` pagination fixtures as matches:
`float-page-break-inside-avoid-3-print.html` and
`rowgroup-page-break-inside-avoid-{1,2,3,4,5}-print.html`. The table cases
cover separated-border row-group fit decisions and repeated header/footer
fragment chrome; broader cloned-decoration and complex nested-fragmentation
limitations remain tracked in `SPEC_DIVERGENCES.md`.

On 2026-08-03, fresh exact `quire-wpt evaluate-test` runs rendered
`monolithic-overflow-005-print.html` and `monolithic-overflow-006-print.html`
as matches. Size-contained definite-height flex items now retain a single
monolithic source canvas across page continuations, including the wrapped
structure in `006`.

## Implemented foundations

- Page-rule resolution keeps the caller-supplied initial page box immutable;
  each materialized page resolves its own `@page` geometry. Propagated
  root/body backgrounds now paint the used page area rather than page margins.
  Transparent absolute paint retains its resolved destination-page ownership,
  while a positive absolute span is retained for viewport-fixed replay after
  the complete document has been laid out, independent of source order. This
  matches `basic-pagination-003`, both `page-left-right` cases,
  `page-margin-auto`, `page-size-001/006/009`, and
  `fixedpos-001/002/004` in direct local rendering. `page-size-009` verifies
  that `vw`/`vh` retain the immutable initial viewport after a named-page
  transition, while used page geometry remains destination-specific.
- Root page continuations now transition on the principal writing mode's
  logical block axis rather than assuming physical Y. This resolves the
  vertical root progression exercised by `page-margin-003`.
- Flex final-fragment handoff rebases following normal flow to the finalized
  destination page context. A flex item's visual fragment span no longer
  rounds its used block size up to a full final page.
- On 2026-07-28, a fresh local run of all 53
  `css/css-page/page-name-*-print.html` reftests rendered 53 matches and no
  mismatches. Named-page selection now resolves used start/end values before
  comparing class-A boundaries and re-enters destination page contexts with
  normalized continuation offsets.
- Page-name scopes retain their lexical specified value independently from the
  currently selected page, so nested and flex-item page groups do not leak
  into their parent scope.
- Formatting-tree propagation resolves each descendant's `page:auto` against
  its nearest non-auto ancestor before its start/end values are compared at a
  class-A boundary. Table wrappers derive those endpoints from their durable
  caption/row fragment in visual order, while repeated headers and footers do
  not create source boundaries; output-page selection is never used as the
  ancestor lookup.
- Inline `page` declarations do not create page groups. Leading direct inline
  content remains in the initial `page:auto` group when a later block
  descendant selects a named page, preserving the class-A page break between
  those groups.
- Page-local canvas insets are tracked while root/body fragments are laid out,
  preventing a page transition from carrying the first page's canvas offset
  into later named pages.
- Page contexts distinguish physical area dimensions from the logical inline
  and block extents of the active writing mode. Child available-space setup
  now takes its fallback block basis from that logical page extent.
- Deferred float paint materializes every destination page and marks a page
  containing deferred paint as a real fragmentainer.
- An auto-height independent formatting context now materializes every
  page/column fragment needed to contain its internal floats. Paintless float
  continuations retain page-local exclusion geometry, so later cleared floats
  and the BFC's final used height advance through the same fragmentainers.
- A cleared, `break-inside: avoid` float now freezes its definite percentage
  block size before isolated replay and defers its destination-page paint.
  Its following in-flow siblings consequently remain in their source
  fragmentainer while the new page receives the float's page-local exclusion.
- Oversized table rows select shared in-flow child boundaries before committing
  a row piece. A child that fits on a fresh fragmentainer is therefore moved
  intact rather than replayed as a zero-height source slice; table captions
  remain table-wrapper edge content across those continuations.
- Absolutely positioned replay retains every captured painted fragment. A
  background-only slice must not be discarded merely because it contains no
  text baseline.
- Forced `left`/`right`/`recto`/`verso` breaks retain any intervening blank
  page needed to place a following fragment on the requested spread side, but
  trailing blank pages with no following fragment are omitted during document
  finalization.

## Remaining renderer clusters

The outstanding raw failures are architectural clusters, not per-test
exceptions:

- Remaining vertical/sideways root-flow work is limited to complex nested
  body-background and orthogonal named-page combinations; ordinary root
  continuation now transitions through the logical block fragmentainer.
- Remaining page-geometry work concerns complex orientation, specificity, and
  print-media-query combinations. Initial viewport units and destination-page
  used geometry are now distinct.
- Canvas/page background propagation still needs page-fragment-local image
  positioning and physical writing-mode mapping.
- Complex monolithic flex/table replay still needs exact continuation-edge
  ownership for nested inline, transformed, and mixed-writing-mode content.
  Positioned/fixed page-span materialization now uses destination-page-aware
  paths.

The three documented CSS Page margin-box `writing-mode` compatibility ratios
remain runner-only reporting. They are not renderer parity and no additional
thresholds should be added.
