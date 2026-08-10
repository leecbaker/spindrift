# WeasyPrint Sample Parity

This document records the current visual comparison state for the configured
`weasyprint-samples` inputs. The baseline is each checked-in sample reference
PDF; page count and page dimensions are checked before raster inspection using
the workflow in `AGENTS/pdf_comparison.md`. The local WeasyPrint version can
be a useful comparison point, but is not the pagination oracle when it differs
from the checked-in sample.

## Current Snapshot

| Sample | Reference pages | Quire pages | Page size | Current state |
| --- | ---: | ---: | --- | --- |
| `book` | 56 | 58 | A5 | The generated contents page resolves each chapter's number, title, and target page through normal-flow `target-counter()` / `target-text()` layout. The chapter `Poire` caption now contributes to its auto-height row-flex line and no longer overlaps the following paragraph. French U+202F punctuation pairs remain intact through inline layout. The remaining text-flow/font-metric divergence moves chapters 3 and 4 to pages 25 and 41 (reference: 23 and 39), producing two extra pages; raster parity needs a new comparison after that pagination difference is resolved. Root-initial-containing-block chapter/outro artwork remains a WeasyPrint compatibility-only difference. |
| `invoice` | 1 | 1 | A4 | The absolutely positioned totals table remains on one aligned row. |
| `letter` | 1 | 1 | A4 | The collapsed first-content margin matches CSS. Quire intentionally suppresses generated pseudo-content on `input` and `textarea`, so WeasyPrint's dotted form leaders remain a compatibility-only visual difference. |
| `poster` | 1 | 2 | 278 mm x 388 mm | The absolute logo is out of flow and the top-line/main-title margins collapse correctly. The in-flow sponsor row leaves less than one address line in the page area, so Quire takes the unforced break before the address; the checked-in WeasyPrint one-page result is retained as a diagnostic compatibility difference. |
| `report` | 8 | 8 | A4 | The generated contents page now resolves linked section titles and page numbers through normal-flow target references. The full-height wrapped flex cover preserves `align-content: space-between`: its two address panels share the cover's block-end. Fresh output retains all five feature-list `::before` float SVG backgrounds as vector paint, matching their reference placement and color on page 5. Forced wrapped-row flex breaks restart packing on their destination page, and the Typography nested flex rows now keep their distinct block positions. Default Fira Sans ligatures retain their authored PDF extraction text through `/ActualText`; their raster difference from the non-ligated reference is intentional. Remaining page-8 visual differences are body-text metrics, wrapping, and glyph rasterization. |
| `ticket` | 1 | 1 | A4 landscape | Fresh local comparison: destination heading and all three later definition-list dividers paint correctly; both PDFs are one A4-landscape page. At 144 dpi, 26,776 of 2,005,644 pixels differ, primarily in glyph/emoji rasterization. |

## Remaining Work

- Keep the letter leader difference classified as intentional: HTML `input` and
  `textarea` controls suppress generated `::before` and `::after` content, so
  compatibility emulation is not planned.
- Re-run the complete six-sample raster comparison after each of those changes
  and update this table from fresh PDFs only.
