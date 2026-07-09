# Concurrency and Parallelism

This document tracks where Quire currently uses async/concurrent execution, and
where the rendering and PDF output pipelines can safely gain parallelism later.
It is intentionally a working design note: add new sections as the architecture
changes or as profiling identifies better targets.

## Current Rendering Pipeline

Production async usage is currently concentrated in resource loading, HTML
orchestration, font loading, and one layout worker thread.

- The CLI runs on Tokio via `#[tokio::main]` in `src/main.rs`.
- `Html::from_file_async`, `Html::from_url_async`, `Html::render_async`, and
  `Html::write_pdf_async` orchestrate the async public pipeline in
  `src/html.rs`.
- CSS file and `@import` loading are async in `src/css/types/source.rs`, but
  imports are resolved depth-first and sequentially.
- Resource reads use `reqwest` for HTTP(S) URLs and `tokio::fs::read` for
  local `file:` URLs in `src/resource.rs`.
- Font loading starts the Parley/fontique context on `spawn_blocking`, loads
  `@font-face` rules with Tokio tasks, and registers loaded font faces on
  `spawn_blocking` in `src/text/system/font_loading.rs`.
- Layout waits for font loading, then runs the page-box build and flow on one
  scoped `quire-layout` thread with a larger stack in `src/layout/split_1.rs`.
- PDF serialization is synchronous once a `Document` exists; only the final file
  write in `Html::write_pdf_async` uses `tokio::fs::write`.

Important current sequential points:

- linked stylesheets load one by one;
- stylesheet imports load one by one;
- `ResourceCache::preload` reads one resource at a time;
- layout, cascade-dependent box construction, page flow, and fragmentation are
  effectively serial;
- PDF object emission through `pdf_writer::Pdf` is serial.

```mermaid
flowchart TD
  A["CLI / API calls Html::write_pdf_async"] --> B["Read input HTML async"]
  B --> C["render_async starts"]

  C --> F0["spawn_blocking: load Parley/fontique context"]
  C --> D["Parse DOM synchronously"]
  D --> E["Collect embedded styles synchronously"]
  E --> L["Load linked stylesheets async, sequential"]
  L --> I["Resolve CSS @imports async, sequential"]
  I --> P["Parse stylesheets synchronously"]

  P --> FF["spawn: load all @font-face rules"]
  FF --> FFN["spawn one task per font face"]
  FFN --> FFS["Within each face: try src list sequentially"]

  P --> R["Build resource path list"]
  R --> RP["ResourceCache::preload async, sequential"]

  F0 --> JOIN["layout_dom_async waits for font system finish"]
  FFS --> JOIN
  RP --> JOIN

  JOIN --> REG["spawn_blocking: register loaded font faces"]
  REG --> PG["Resolve page direction and page counter seeds"]
  PG --> LT["spawn scoped quire-layout thread"]
  LT --> BX["Build formatting box tree"]
  BX --> FM["Resolve font-metric lengths"]
  FM --> FLOW["Flow page box content"]
  FLOW --> DOC["Document"]

  DOC --> META["Extract metadata"]
  META --> PDF["write_pdf_bytes with PdfOptions synchronously"]
  PDF --> V["Validate paint operations"]
  V --> SH["Shape document text for PDF"]
  SH --> FP["Plan font embedding"]
  FP --> PC["Build page content streams, sequential per page"]
  PC --> OBJ["Write PDF objects"]
  OBJ --> W["tokio::fs::write output file async"]
```

## PDF Output Opportunities

The PDF writer currently performs most work serially in `src/pdf/writer.rs`:

1. shape document text for PDF;
2. collect used glyphs and build embedded font plans;
3. deduplicate images;
4. build page content streams;
5. collect page graphics-state resources;
6. plan annotations and outlines;
7. write PDF objects into one `pdf_writer::Pdf`.

Font subsetting is one of the best near-term concurrency targets. The current
font planning step combines cheap global planning with expensive per-font work:
collecting used glyphs, deduplicating document fonts, assigning resource names,
auditing font programs, subsetting font files, building descriptor metrics,
building ToUnicode data, and optionally building PDF/A CIDSet data.

Page content rendering needs the PDF font resource mapping, not the subset font
bytes. Text content in `src/pdf/content.rs` maps `document_font_id` to an
embedded font resource name such as `RF1`; it does not need the final embedded
font stream. That means font planning can be split:

1. collect used glyphs and deduplicate document fonts;
2. assign stable embedded font indexes, resource names, and object IDs;
3. run heavy per-font audit/subsetting concurrently.

After step 2, page content stream generation can run while font files are being
subset.

```mermaid
flowchart TD
  A["Document"] --> B["Shape document text"]
  B --> C["Collect used glyphs"]
  C --> D["Plan embedded font identity map<br/>document font -> RF name + object IDs"]

  D --> F1["Subset/audit font 1"]
  D --> F2["Subset/audit font 2"]
  D --> F3["Subset/audit font N"]

  D --> P0["Pre-count per-page dynamic form XObjects"]
  P0 --> PIDs["Assign page-local object ID ranges"]
  PIDs --> P1["Render page stream 1"]
  PIDs --> P2["Render page stream 2"]
  PIDs --> P3["Render page stream N"]

  A --> I0["Prepare image resource data per image"]
  I0 --> I1["Stable image dedupe + image object IDs"]

  A --> G1["Plan ExtGState resources per page"]
  A --> O["Plan outlines and annotations"]

  F1 --> JOIN["Join prepared artifacts"]
  F2 --> JOIN
  F3 --> JOIN
  P1 --> JOIN
  P2 --> JOIN
  P3 --> JOIN
  I1 --> JOIN
  G1 --> JOIN
  O --> JOIN

  JOIN --> W["Serial pdf_writer object emission"]
  W --> Z["pdf.finish"]
```

Good PDF-side candidates for parallel work:

- audit and subset each unique embedded font independently;
- generate page content streams independently after stable font resource names
  and object ID ranges are known;
- prepare image resource data, especially cropped image RGB/alpha buffers, then
  perform stable deduplication;
- collect page-local ExtGState resources per page;
- shape PDF text per page, although this is mostly copying already-shaped
  layout glyph data today.

Work that should probably remain serial or have a serial planning phase:

- global object ID allocation;
- stable font and image resource naming;
- outline object ID planning;
- final mutation of `pdf_writer::Pdf`;
- final deterministic file identifier hashing.

## Open Questions

- Should Quire use Tokio tasks, scoped threads, or a CPU pool for CPU-bound PDF
  work? Tokio is already present for async I/O, but font subsetting and page
  stream generation are CPU work.
- Can page content rendering be split into a pure pre-count pass and a render
  pass without duplicating too much paint-tree traversal logic?
- Should image deduplication hash cropped image data after parallel preparation,
  or should we introduce a cheaper structural key before materializing cropped
  buffers?
- How much determinism do we require from log ordering and warning ordering when
  font audits run in parallel?

## Future Notes

Add profiling results, implementation sketches, and rejected approaches here as
parallelism work lands.
