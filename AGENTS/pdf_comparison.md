# PDF Visual Comparison

The WPT runner compares rendered PDFs as visual output. It has two exact
fast paths before rasterization: byte-identical PDF SHA-256 hashes, and a
canonical reachable-PDF graph match that ignores only the document title in
the Info dictionary and XMP `dc:title`. The latter preserves page content,
resources, ICC profiles, annotations, text mappings, and all other metadata.

Use the runner in the sibling `spindrift-wpt` checkout as the source of truth. Do
not invoke `magick compare` or `magick identify` directly when evaluating a
test: the runner's in-process comparator is the authoritative metric and
avoids repeatedly starting ImageMagick.

## Inputs and configuration

For an engine and one reftest the runner uses:

- `actual.pdf`: the PDF rendered from the test file.
- `reference.pdf`: the PDF rendered from the reference file.
- The reftest operator: `==` means the images should match; `!=` means they
  should differ.
- Raster settings from `spindrift-wpt.toml`. The normal pass is 96 DPI and the
  retry pass is 288 DPI. Raster output is canonical 8-bit RGB PNG (`PNG24`).
- `max_diff_ratio`: the normalized-MAE pass/fail threshold. Its default is
  `0.0004` for every engine. Exact per-test engine overrides remain in effect
  and must not be generalized or overwritten.

The field is historically named `max_diff_ratio`, but it is **not** a count of
different pixels divided by the image size. It is normalized RGB mean absolute
error (MAE): the mean of the absolute difference of every red, green, and blue
channel, divided by 255. It ranges from `0.0` (identical raster pixels) to
`1.0` (every RGB channel differs by the full range).

## Evaluation process

1. Render the test and reference PDFs.
2. If their PDF SHA-256 hashes match, record `PdfIdentical`. Otherwise compare
   their title-insensitive canonical PDF graphs. A graph match records
   `PdfTitleInsensitiveIdentical`. For either exact fast path, a `==` reftest
   passes and a `!=` reftest fails; no PNGs need be generated.
3. Rasterize both PDFs at 96 DPI when neither exact fast path applies. Alpha is composited against white and the
   output is forced to 8-bit RGB PNG.
4. If page counts or corresponding pixel dimensions differ, the page images
   are incompatible. A `==` reftest fails and a `!=` reftest passes.
5. For every corresponding page pair, load both PNG files concurrently and
   decode them concurrently. Once both are decoded, calculate normalized RGB
   MAE in-process. The runner does not start a child comparison process.
6. Record the maximum page MAE as `max_diff_ratio`. `absolute_error` in old
   artifact metadata is only an equivalent number of fully-different pixels;
   use `max_diff_ratio` for decisions and diagnosis.
7. Write an absolute-RGB-difference PNG only if a page is non-identical. Do
   not expect a diff artifact for exact pages.
8. For `==`, pass when the measured `max_diff_ratio` is at or below the
   effective raster threshold. For `!=`, pass for any nonzero MAE; it is not
   gated by the match tolerance.
9. If a `==` result fails at 96 DPI, rerasterize the already-rendered PDFs at
   288 DPI and repeat the same comparison. A passing retry is reported as a
   high-DPI-resolved result. Rendering the HTML/PDF engine is not repeated.

## Inspecting two existing PNG artifacts

Use the runner subcommand rather than ImageMagick:

```bash
cd /Users/lee/projects/spindrift-wpt
target/release/spindrift-wpt compare-images actual.png reference.png --diff diff.png
```

It prints `max_diff_ratio`, pixel count, and whether the input pixels are
identical. `--diff` writes an 8-bit RGB absolute-difference image only when
the inputs differ. Different image dimensions are an error rather than a
metric result.

## Evaluating one WPT test

Use the exact-path command to render and evaluate one test across all
configured engines, producing the usual PDFs, PNG artifacts, diffs, and HTML
report:

```bash
cd /Users/lee/projects/spindrift-wpt
target/release/spindrift-wpt evaluate-test css/css-color/deprecated-sameas-003.html
```

The test path is relative to the WPT tests root. This is exact selection—not a
directory or prefix filter—so use it instead of `run <filter> --limit 1` when
investigating a single WPT. Pass `--include-scripts` only for a script-driven
test that must be included.

When evaluating a marginal failure, inspect both the 96-DPI and the recorded
288-DPI result and artifacts. Do not relax a global tolerance merely to make a
single visual defect pass; first confirm the metric and the actual/reference
artifacts being compared.
