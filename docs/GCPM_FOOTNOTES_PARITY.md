# GCPM footnotes parity

Quire implements the page-footnote model from CSS Generated Content for
Paged Media Level 3:
<https://www.w3.org/TR/css-gcpm-3/#footnotes>.

## Implemented

- `float: footnote` removes the body from its source formatting flow and
  inserts a generated `::footnote-call` at the source position.
- `::footnote-call` and `::footnote-marker` cascade from their originating
  element and use the GCPM `footnote` counter defaults. Counter snapshots are
  planned in DOM order before pagination, so retry does not renumber calls.
- `@page { @footnote { … } }` is a dedicated page-area rule, independently
  cascaded from the sixteen CSS Paged Media margin boxes.
- Footnote bodies are measured, page assignments are iterated to a fixed
  point, then the final flow reserves the resulting page-local area. A call
  becomes page-owned only when its graph-selected inline line commits, so
  intrinsic sizing and table probes cannot reserve a footnote on the wrong
  page.
- The footnote area's margins, backgrounds, borders, background images, and
  padding are applied once per page area; its bottom margin edge is anchored
  to the page-area bottom. The generated marker is rendered through the
  generated-content evaluator rather than principal-element layout.

## Validation

Focused coverage currently includes CSS parsing for the GCPM properties and
page-area rule plus a box-tree regression for detachment and call/marker
counter events. Local PDF smoke documents verify one call, one marker, and one
body on a single page, a call that crosses to its selected page, and the
Taiwanese-numerals table's one-page footnote layout.

## Remaining divergence

The authoritative list is maintained in `SPEC_DIVERGENCES.md`. In brief,
area margins and sizing constraints, inline/compact body packing, and the
line/block policy remain to be implemented. Oversized bodies currently use
normal block fragmentation as the fallback.
