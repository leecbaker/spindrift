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
reftests with the development binary and `--page-margin=0px`:

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

## Implemented foundations

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
  class-A boundary. The same immutable used-value model is used for ordinary
  block flow and table rows; output-page selection is no longer used as that
  ancestor lookup.
- Leading direct inline content remains in the initial `page:auto` group when
  a later block descendant selects a named page, preserving the class-A page
  break between those two groups.
- Page-local canvas insets are tracked while root/body fragments are laid out,
  preventing a page transition from carrying the first page's canvas offset
  into later named pages.
- Page contexts distinguish physical area dimensions from the logical inline
  and block extents of the active writing mode. Child available-space setup
  now takes its fallback block basis from that logical page extent.
- Deferred float paint materializes every destination page and marks a page
  containing deferred paint as a real fragmentainer.
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

## Remaining renderer clusters

The outstanding raw failures are architectural clusters, not per-test
exceptions:

- Vertical and sideways root flows still paginate through the physical Y axis.
  Their logical block progression must select a new physical page fragment,
  which affects the four body-background writing-mode cases and orthogonal
  named-page cases.
- Page geometry is not yet fully staged as immutable initial viewport geometry
  followed by each destination page's used box. This affects page size,
  orientation, page-rule specificity, logical page margins, and print media
  queries.
- Canvas/page background propagation still needs page-fragment-local image
  positioning and physical writing-mode mapping.
- Monolithic table, inline, and positioned/fixed overflow need the common
  page-fragment disposition model. The positioned cases must preserve the
  renderer's A4 default while their WPT viewport assumptions remain explicit
  harness policy, rather than adding test-specific renderer behavior.

The three documented CSS Page margin-box `writing-mode` compatibility ratios
remain runner-only reporting. They are not renderer parity and no additional
thresholds should be added.
