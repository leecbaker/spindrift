# Spindrift PDF renderer

`spindrift` is a from-scratch Rust HTML/CSS to PDF renderer inspired by
WeasyPrint. It exposes a crate API and a CLI, parses HTML with `html5ever`,
parses explicit/XML-declared XML input with `xml5ever`, parses CSS syntax with
`cssparser`, matches selectors with Servo's `selectors` crate, performs
paged layout, and emits deterministic PDF files with Parley-measured text,
embedded system fonts, and Type 0 Unicode text resources.

## Examples

Convert an HTML file to a PDF file:

```sh
spindrift document.html document.pdf
```

## CLI

The invocation mirrors WeasyPrint's basic syntax:

```sh
cargo run -- example.html example.pdf
```

Once installed as a binary:

```sh
spindrift example.html example.pdf
```

Enable debug logs, including PDF generation timing, with:

```sh
cargo run -- --debug example.html example.pdf
```

Print host information useful in bug reports without rendering a document:

```sh
spindrift --info
```

Use `--full-fonts` to embed complete font programs when PDF embedding permits
it, matching WeasyPrint's opt-out of font subsetting:

```sh
cargo run -- --full-fonts input.html /tmp/full-fonts.pdf
```

Use `--pdf-profile` to select Spindrift's PDF writer policy. The default is
`pdf`; pass `--pdf-profile pdf/a-1b` to opt into the current PDF/A-oriented
writer behavior. Supported values are `pdf` and `pdf/a-1b`. These profiles select the implemented header,
metadata, and font-planning behavior, but do not yet guarantee PDF/A
conformance. `--pdf-variant` and `--pdf-type` remain aliases.

Use `--uncompressed-pdf` to omit `/FlateDecode` from every generated PDF
stream. This is primarily useful when inspecting PDF syntax while debugging;
it substantially increases output size:

```sh
cargo run -- --uncompressed-pdf input.html /tmp/debug.pdf
```

Spindrift follows HTTP redirects by default and treats every external resource
failure as fatal. Use `--no-http-redirects` to reject HTTP redirects, or
`--allow-fetch-errors` to skip failed optional subresources and continue:

```sh
cargo run -- --no-http-redirects --allow-fetch-errors input.html /tmp/output.pdf
```

External stylesheets are supported:

```sh
cargo run -- -s style.css input.html /tmp/hello.pdf
```

Command-line stylesheets are user-origin stylesheets, so they provide print
preferences without overriding an explicit author `@page` declaration. The
library API has the same behavior when a stylesheet is marked with
`Css::with_user_origin()`; `Html::with_stylesheet()` remains the author-origin
API for document styles.

Set document page geometry in CSS rather than command-line or crate options:

```css
@page { size: A4; margin: 18mm }
```

Generate shell completions with:

```sh
cargo run -- --generate-completion bash > spindrift.bash
```

## Library

```rust
use spindrift::{Html, PdfOptions, RenderOptions};

let mut pdf = Vec::new();
Html::from_string("<p>Hello, world</p>")
    .write_pdf(&mut pdf, &RenderOptions::default(), &PdfOptions::default())
    .await?;
# Ok::<(), spindrift::Error>(())
```

Rendering an HTML file from Rust uses the same library path:

```rust
use spindrift::{Html, PdfOptions, RenderOptions};
use std::fs::File;

let mut output = File::create("/tmp/example.pdf")?;
Html::from_file("example.html")
    .await?
    .write_pdf(
        &mut output,
        &RenderOptions::default(),
        &PdfOptions::default(),
    )
    .await?;
# Ok::<(), spindrift::Error>(())
```

## Benchmarking

`cargo bench` has a few useful benchmarks.

When profiling using instruments, you may need to add an entitlement to allow Instruments to inspect the process:

```bash
codesign --force  --sign - --timestamp=none --entitlements debug.entitlements target/release/spindrift
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

There is an external runner for web platform tests in `../spindrift-wpt`; there is a checkout of the tests there at `../spindrift-wpt/third_party/wpt`.
