# reasyprint

`reasyprint` is a from-scratch Rust HTML/CSS to PDF renderer inspired by
WeasyPrint. It exposes a crate API and a CLI, parses HTML with `html5ever`,
parses CSS syntax with `cssparser`, matches selectors with Servo's `selectors`
crate, performs simple paged layout, and emits deterministic PDF files with
Parley-measured text, embedded system fonts, and Type 0 Unicode text resources.

The long-term goal is feature parity with WeasyPrint's print-oriented pipeline:
HTML parsing, CSS cascade, formatting boxes, paged layout, drawing, and PDF
metadata. The checked-in `WeasyPrint/` directory is the upstream reference used
for parity research and test migration; it is not a runtime dependency.

Current implementation status and the resume checklist live in
[`PROGRESS.md`](PROGRESS.md).

Implemented now:

- crate API and CLI
- simple DOM parsing with text, block elements, inline spans, and `<br>`
- embedded, linked, and API-provided stylesheets
- CSS rules parsed with `cssparser`, selector parsing/matching through
  Servo's `selectors`, inline style, `@media print`/`@media all`, specificity,
  and source order
- page size/margin from `@page`, including a small named-page-size subset and
  orientation
- block layout with margin, padding, border, width, height, min/max dimensions,
  `box-sizing`, background, simple forced page breaks, and basic positioned
  blocks with `left`/`top`/`right`/`bottom`
- text color, font size, line height, text alignment, text decoration,
  `text-transform`, `visibility`, CSS generic font-family mapping, numeric
  `font-weight`, `font-style`, `font-width`/`font-stretch`, system font
  discovery through Fontique, text measurement, paragraph wrapping, and render
  glyph runs through `parley`, CSS `@font-face` loading for local/data-URI
  TrueType fonts, and embedded TrueType Type 0 PDF fonts with ToUnicode maps
- body/page background painting
- unordered and ordered lists with a small `list-style-type` subset
- basic tables with simple `colspan`
- title, author, and creator metadata
- CSS bookmarks and PDF outlines for heading defaults and authored
  `bookmark-level`, `bookmark-label`, and `bookmark-state`
- URI link annotations
- deterministic uncompressed PDF output with embedded system TrueType fonts for
  normal text

## CLI

For the checked-in KinSNP report fixture, the invocation mirrors WeasyPrint's
basic syntax:

```sh
cargo run -- KinSNP_example.html /tmp/kinsnp.pdf
```

Once installed as a binary:

```sh
reasyprint KinSNP_example.html /tmp/kinsnp.pdf
```

Enable ReasyPrint debug logs, including PDF generation timing, with:

```sh
RUST_LOG=reasyprint=debug cargo run -- KinSNP_example.html /tmp/kinsnp.pdf
```

```sh
cargo run -- '<p>Hello, world</p>' /tmp/hello.pdf
```

Use `--string` to force the first argument to be treated as HTML source:

```sh
cargo run -- --string '<h1>Hello</h1><p>From Rust.</p>' /tmp/hello.pdf
```

External stylesheets are supported:

```sh
cargo run -- --string -s style.css '<p class="lead">Hello</p>' /tmp/hello.pdf
```

Generate shell completions with:

```sh
cargo run -- --generate-completion bash > reasyprint.bash
```

## Library

```rust
use quire::{Html, RenderOptions};

let pdf = Html::from_string("<p>Hello, world</p>")
    .write_pdf_bytes(&RenderOptions::default())?;
# Ok::<(), quire::Error>(())
```

Rendering the KinSNP fixture from Rust uses the same library path:

```rust
use quire::{Html, RenderOptions};

Html::from_file("KinSNP_example.html")?
    .write_pdf("/tmp/kinsnp.pdf", &RenderOptions::default())?;
# Ok::<(), quire::Error>(())
```
