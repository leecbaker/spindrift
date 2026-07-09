# WeasyPrint Sample Parity

This document records the current visual comparison state for the configured
`weasyprint-samples` inputs. The baseline is a fresh local WeasyPrint render;
page count and page dimensions are checked before raster inspection using the
workflow in `AGENTS/pdf_comparison.md`.

## Current Snapshot

| Sample | Reference pages | Quire pages | Page size | Current state |
| --- | ---: | ---: | --- | --- |
| `book` | 56 | 56 | A5 | Named-page/right-break sequence and cover/chapter image paint match the required pagination. |
| `invoice` | 1 | 1 | A4 | The absolutely positioned totals table remains on one aligned row. |
| `letter` | 1 | 1 | A4 | Form fields and leader rules remain on one page; some generated-leader line-box spacing still differs visually. |
| `poster` | 1 | 2 | 278 mm x 388 mm | Inline box-edge line sizing is aligned and the sponsor row now stays on page 1. The address alone still overflows; Pango strut inspection shows this is not caused by Quire using a taller `line-height: normal` metric. |
| `report` | 8 | 8 | A4 | Column continuation now preserves reading order without cross-page text overlap. Generated SVG icon background paint still needs review. |
| `ticket` | 1 | 1 | A4 landscape | The explicit 25 cm x 8 cm body is centered and the flex-section divider is placed correctly. |

## Remaining Work

- Trace the root/page-area flow transition after the sponsor flex row. The
  final address must fit the authored page area without changing its 278 mm x
  388 mm size; Pango strut inspection rules out normal line-height metrics as
  the cause of its extra page.
- Audit generated `leader(dotted)` block line boxes for form controls against
  CSS Generated Content and CSS Inline Layout; the letter labels and leaders
  should occupy distinct stable lines.
- Restore generated SVG background-image paint in the report feature-list
  floats, then re-run the raster comparison for pages 5 and 6.
- Re-run the complete six-sample raster comparison after each of those changes
  and update this table from fresh PDFs only.
