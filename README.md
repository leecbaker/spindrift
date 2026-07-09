# Quire PDF renderer

`quire` is a from-scratch Rust HTML/CSS to PDF renderer inspired by
WeasyPrint. It exposes a crate API and a CLI, parses HTML with `html5ever`,
parses explicit/XML-declared XML input with `xml5ever`, parses CSS syntax with
`cssparser`, matches selectors with Servo's `selectors` crate, performs simple
paged layout, and emits deterministic PDF files with Parley-measured text,
embedded system fonts, and Type 0 Unicode text resources.

The long-term goal is feature parity with WeasyPrint's print-oriented pipeline:
HTML parsing, CSS cascade, formatting boxes, paged layout, drawing, and PDF
metadata. The checked-in `WeasyPrint/` directory is the upstream reference used
for parity research and test migration; it is not a runtime dependency.

Current implementation status and the resume checklist live in
[`PROGRESS.md`](PROGRESS.md).

Design notes on current async usage and future PDF/rendering parallelism live in
[`docs/CONCURRENCY_AND_PARALLELISM.md`](docs/CONCURRENCY_AND_PARALLELISM.md).

## Examples

Convert an HTML file to a PDF file:

```sh
quire document.html document.pdf
```

Set the initial page size while converting a file. Values use CSS absolute
length units; an `@page` rule in the document can override this size:

```sh
quire --page-size 8.5in 11in document.html document.pdf
```

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
- deterministic PDF output with embedded system TrueType fonts for normal text;
  generated streams use Flate compression by default

## CLI

For the checked-in KinSNP report fixture, the invocation mirrors WeasyPrint's
basic syntax:

```sh
cargo run -- KinSNP_example.html /tmp/kinsnp.pdf
```

Once installed as a binary:

```sh
quire KinSNP_example.html /tmp/kinsnp.pdf
```

Enable Quire debug logs, including PDF generation timing, with:

```sh
cargo run -- --debug KinSNP_example.html /tmp/kinsnp.pdf
```

Without a logging flag, Quire shows warnings and errors by default. Set
`RUST_LOG` to customize `env_logger` filtering; `--verbose`, `--debug`, and
`--quiet` override `RUST_LOG` for WeasyPrint-compatible CLI behavior.

Use `--full-fonts` to embed complete font programs when PDF embedding permits
it, matching WeasyPrint's opt-out of font subsetting:

```sh
cargo run -- --full-fonts input.html /tmp/full-fonts.pdf
```

Use `--pdf-profile` to select Quire's PDF writer policy. The default is
`pdf/a-2b`; supported values are `pdf`, `pdf/a-1b`, `pdf/a-2b`, `pdf/a-3b`,
`pdf/a-2u`, and `pdf/a-3u`. These profiles select the implemented header,
metadata, and font-planning behavior, but do not yet guarantee PDF/A
conformance. `--pdf-variant` and `--pdf-type` remain aliases.

Use `--uncompressed-pdf` to omit `/FlateDecode` from every generated PDF
stream. This is primarily useful when inspecting PDF syntax while debugging;
it substantially increases output size:

```sh
cargo run -- --uncompressed-pdf input.html /tmp/debug.pdf
```

Quire follows HTTP redirects by default and treats every external resource
failure as fatal. Use `--no-http-redirects` to reject HTTP redirects, or
`--allow-fetch-errors` to skip failed optional subresources and continue:

```sh
cargo run -- --no-http-redirects --allow-fetch-errors input.html /tmp/output.pdf
```

Use `--input-syntax xml` to force XML/XHTML parsing. The default
`--input-syntax auto` keeps HTML parsing unless the source begins with an XML
declaration such as `<?xml version="1.0"?>`.

External stylesheets are supported:

```sh
cargo run -- -s style.css input.html /tmp/hello.pdf
```

Generate shell completions with:

```sh
cargo run -- --generate-completion bash > quire.bash
```

## Library

```rust
use quire::{Html, PdfOptions, RenderOptions};

let pdf = Html::from_string("<p>Hello, world</p>")
    .write_pdf_bytes(&RenderOptions::default(), &PdfOptions::default())?;
# Ok::<(), quire::Error>(())
```

Rendering the KinSNP fixture from Rust uses the same library path:

```rust
use quire::{Html, PdfOptions, RenderOptions};

Html::from_file("KinSNP_example.html")?
    .write_pdf(
        "/tmp/kinsnp.pdf",
        &RenderOptions::default(),
        &PdfOptions::default(),
    )?;
# Ok::<(), quire::Error>(())
```

## Benchmarking

`cargo bench` has a few useful benchmarks.

When profiling using instruments, you may need to add an entitlement to allow Instruments to inspect the process:

```bash
codesign --force  --sign - --timestamp=none --entitlements debug.entitlements target/release/quire
```

```xml
<!-- debug.entitlements -->
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.get-task-allow</key>
  <true/>
</dict>
</plist>
```

## Web Platform Tests

There is an external runner for web platform tests in `../quire-wpt`; there is a checkout of the tests there at `../quire-wpt/third_party/wpt`.
