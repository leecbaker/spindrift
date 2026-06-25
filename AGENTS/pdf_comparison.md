# PDF Visual Comparison

The comparison treats PDFs as visual output. It does not compare PDF structure directly.

## Inputs

For one engine and one reftest:

- `actual.pdf`: PDF rendered from the test file.
- `reference.pdf`: PDF rendered from the reference file.
- Raster settings:
  - `dpi`
  - ImageMagick executable path, usually `magick`
  - Ghostscript executable path, used by ImageMagick as the PDF delegate
  - `fuzz`, for small pixel tolerance
  - `max_diff_ratio`, pass/fail threshold
  - command timeout
- Reftest operator:
  - `==`: actual should visually match reference
  - `!=`: actual should visually differ from reference

## Step 1: Rasterize PDFs

Each PDF is converted into one PNG per page.

```bash
magick \
  -density <dpi> \
  input.pdf \
  -background white \
  -alpha remove \
  -alpha off \
  page-%04d.png
```

Example outputs:
actual-pages/page-0000.png
actual-pages/page-0001.png
reference-pages/page-0000.png
reference-pages/page-0001.png
Alpha is removed against white so transparent PDF backgrounds do not create misleading diffs.

## Step 2: Check Page Count

If the actual and reference PDFs rasterize to different numbers of pages, the comparison fails.
Example error:
page count mismatch: actual=2 reference=1

## Step 3: Check Page Dimensions

For each page pair, get the image dimensions:

```bash
magick identify -format "%w %h" page.png
```

If corresponding pages have different pixel dimensions, the comparison fails.

## Step 4: Compare Pixels

For each page pair:

```bash
magick compare \
  -metric AE \
  -fuzz <fuzz> \
  actual.png \
  expected.png \
  diff.png
```

AE means absolute error: the number of pixels that differ after applying the fuzz tolerance. ImageMagick writes this metric to stderr.

## Compute metrics

diff_ratio = absolute_error / (width * height)
The comparison result uses the maximum diff_ratio across all pages.
Pass/Fail Rules
For == reftests:
pass if max_diff_ratio <= configured max_diff_ratio
For != reftests:
pass if max_diff_ratio > configured max_diff_ratio
Outputs
For each comparison, record:
status: pass, fail, or error
reference path
operator: == or !=
max_diff_ratio
per-page metrics:page number
absolute error pixel count
total pixels
diff ratio

artifact paths:actual PDF
actual page PNGs
reference PDF
reference page PNGs
diff PNGs
