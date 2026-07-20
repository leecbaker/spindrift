# WeasyPrint and Prince Interface Parity

This document tracks the public CLI and library surfaces of WeasyPrint and
Prince, and the corresponding Quire interface. It is an API/UX compatibility
tracker, not a claim that matching an option also matches either renderer's
layout or PDF output. CSS and PDF conformance gaps remain tracked in
[`SPEC_DIVERGENCES.md`](../SPEC_DIVERGENCES.md).

The intended endpoint is a Rust-native API and a `clap` CLI that can perform
the useful work exposed by both reference renderers. Exact Python call shapes,
Prince wrapper APIs, and Prince's proprietary CSS extensions are explicitly
not compatibility goals.

## Sources and Status

- WeasyPrint documentation: [API reference, stable 69.0](https://doc.courtbouillon.org/weasyprint/stable/api_reference.html)
  and [manual page](https://doc.courtbouillon.org/weasyprint/stable/manpage.html).
- The checked-in `Weasyprint/` source is the local implementation reference.
- Prince documentation: [command-line options](https://www.princexml.com/doc-refs/),
  [input](https://www.princexml.com/doc/prince-input/),
  [PDF output and profiles](https://www.princexml.com/doc/prince-output/), and
  [server integration](https://www.princexml.com/doc/server-integration/).
- “Available” means an equivalent is public and usable now. “Partial” means it
  exists but differs in a material documented behavior. “Missing” means no
  public equivalent exists. “Deliberately different” means Quire offers a
  better Rust-oriented shape rather than copying a Python convenience.

The status below reflects the current working tree, not a released API.

## Target Shape in Rust

WeasyPrint concentrates source loading, resource policy, layout configuration,
and PDF serialization options in dynamically typed keyword arguments. Rust
should make those boundaries explicit instead:

```rust,no_run
use quire::{Css, FetchErrorPolicy, Html, PdfOptions, RenderOptions, ResourcePolicy};

let resources = ResourcePolicy {
    follow_http_redirects: false,
    // The default is FetchErrorPolicy::Fail. Allow is opt-in recovery for
    // failed optional subresources.
    error_policy: FetchErrorPolicy::Allow,
};

let stylesheet = Css::from_file("print.css").await?.with_user_origin();
let html = Html::from_file("report.html")
    .await?
    .with_resource_policy(resources)
    .with_stylesheet(stylesheet);

let document = html.render(&RenderOptions::default()).await?;
let pdf = document.write_pdf_bytes(&PdfOptions::default())?;
```

`ResourcePolicy` is a current public type. The example shows the ownership
split:

- `Html` and `Css` have explicit constructors for string, path, URL, and
  reader/byte input. Rust should not reproduce Python's ambiguous positional
  input guessing.
- `ResourcePolicy` controls built-in fetching and cache behavior. A
  trait-backed custom fetcher remains the Rust equivalent of WeasyPrint's
  `URLFetcher`.
- `RenderOptions` owns layout and cascade inputs only.
- `PdfOptions` owns the implemented serialization controls: profile, font
  embedding, stream compression, and producer metadata. Future PDF controls
  belong at this boundary.
- `Document` is the rendered intermediate result. It should support selected
  page output and structured inspection without exposing mutable PDF internals.

This split keeps percentage/layout configuration separate from PDF emission,
and lets a `Document` be rendered once and serialized in more than one way.

## CLI Parity

WeasyPrint uses `weasyprint [options] <input> <output>`. Quire's binary target
and clap command name are both `quire`. The compatible long options below
should be preferred; short aliases should follow WeasyPrint when they do not
conflict with a stronger Quire convention.

### WeasyPrint Positional Arguments and General Flags

| WeasyPrint surface | Quire equivalent | Status | Work remaining |
| --- | --- | --- | --- |
| `<input>` URL, filename, or `-` for stdin | Path, `file:`, `http:`, or `https:` URL | Partial | Accept `-` as byte input. Literal HTML is intentionally a Rust-library-only input mode. |
| `<output>` filename or `-` for stdout | Canonicalized filesystem path | Partial | Accept `-` and stream PDF bytes to stdout without corrupting diagnostics. |
| `--version` | clap-generated version | Available | Keep the binary and clap command names consistent. |
| `-i`, `--info` | None | Missing | Add a deterministic system/dependency report suitable for bug reports. |
| `-v`, `--verbose`; `-d`, `--debug`; `-q`, `--quiet` | Mutually exclusive `env_logger` CLI flags | Available | No flag uses `warn`; verbose, debug, and quiet map to `info`, `debug`, and `off`. Explicit flags override `RUST_LOG`; without one, `RUST_LOG` remains available. |

### WeasyPrint Rendering and PDF Options

| WeasyPrint option | Quire equivalent | Status | Work remaining |
| --- | --- | --- | --- |
| `-s`, `--stylesheet` (repeatable user stylesheet) | `-s`, `--stylesheet` (repeatable) | Partial | The CLI currently loads these as author-origin `Css`; apply `Css::with_user_origin()` so cascade precedence matches WeasyPrint. |
| `-a`, `--attachment` (repeatable) | None | Missing | Add `Attachment` input and PDF embedded-file serialization. |
| `--attachment-relationship` (repeatable) | None | Missing | Model the ISO 32000 associated-file relationship alongside each attachment, with cardinality validation. |
| `--pdf-identifier` | None | Missing | Add an explicit trailer `/ID` option; document deterministic/default identifier behavior. |
| `--pdf-variant` | `--pdf-profile`; `--pdf-variant` / `--pdf-type` aliases | Partial | `PdfProfile` accepts `pdf`, PDF/A-1b, -2b, -3b, -2u, and -3u, selecting current header, metadata, and font-planning behavior rather than claiming PDF/A conformance. Add A/4, PDF/UA, PDF/X, and debug only with their required writer behavior and validation. |
| `--pdf-version` | Profile selects an internal version | Partial | Expose an explicit compatible version option and reject combinations forbidden by the selected conformance target. |
| `--pdf-forms` | None | Missing | Preserve form semantics through layout and emit AcroForm fields/appearances. |
| `--pdf-tags` | None | Missing | Add tagged-PDF structure trees, roles, annotation linkage, and validation; this is also needed for PDF/UA. |
| `--uncompressed-pdf` | `--uncompressed-pdf`; `PdfOptions::compression: PdfCompression` | Available | `PdfCompression::Compressed` is the default; `Uncompressed` omits `/FlateDecode` from every generated stream for debugging. |
| `--xmp-metadata` (repeatable) | None | Missing | Accept repo/user-provided XMP packet fragments with clear merge/validation rules. |
| `--custom-metadata` | None | Missing | Extract and serialize custom HTML metadata rather than only title/author/creator. |
| `--output-intent` | None | Missing | Add ICC/profile resolution and PDF output-intent serialization. |
| `-p`, `--presentational-hints` | `-p`, `--presentational-hints` | Available | Continue the element-by-element conformance audit in `SPEC_DIVERGENCES.md`. |
| `--optimize-images` | None | Missing | Add lossless image optimization that preserves required PDF/A/PDF/UA behavior. |
| `-j`, `--jpeg-quality` | None | Missing | Add bounded JPEG re-encoding as an opt-in output step. |
| `-D`, `--dpi` | None | Missing | Downsample raster images at a maximum effective DPI while retaining correct CSS intrinsic geometry. |
| `--full-fonts` | `--full-fonts`; `PdfOptions::font_embedding: FontEmbeddingMode` | Available | `FontEmbeddingMode::Subset` is the default; `Full` embeds complete programs where valid PDF Type 0 embedding permits it. TTC/OTC faces and OpenType CFF still require PDF-compatible extraction. |
| `--hinting` | None | Missing | Preserve hinting tables as an explicit font-subsetting policy. |
| `-c`, `--cache-folder` | In-memory internal `ResourceCache` | Partial | Expose an owned disk-cache directory, cleanup policy, and cache limits. |

### WeasyPrint HTML and Resource-fetching Options

| WeasyPrint option | Quire equivalent | Status | Work remaining |
| --- | --- | --- | --- |
| `-e`, `--encoding` | None; HTML/CSS source loading requires UTF-8 | Missing | Support explicit input encoding and HTTP charset handling before parsing. |
| `-m`, `--media-type` | `--media-type print\|screen` | Partial | Support the CSS media type accepted by the API rather than only this two-value subset, while retaining typed well-known values. |
| `-u`, `--base-url` | `-u`, `--base-url` | Available | Test stdin/base-url behavior when stdin is added. |
| `-t`, `--timeout` | None | Missing | Put HTTP timeout in `ResourcePolicy` and pass it to the HTTP client. |
| `--allowed-protocols` | Fixed `file`, `http`, and `https` allowlist | Partial | Make the allowlist public and configurable; keep a safe default. |
| `--no-http-redirects` | `--no-http-redirects`; `ResourcePolicy::follow_http_redirects` | Available | Quire follows redirects by default and retains the final response URL as the base for HTML and CSS resources. |
| `--fail-on-http-errors` | Deliberately different: strict fetching is the default; `--allow-fetch-errors` / `FetchErrorPolicy::Allow` opt into recovery | Deliberately different | Quire treats HTTP, filesystem, and other external-resource failures as fatal unless recovery is explicitly enabled. |

### Quire-only CLI Features

These are useful extensions, but should not block WeasyPrint-compatible use:

| Quire option | Purpose |
| --- | --- |
| `--input-syntax auto\|html\|xml` | Selects HTML/XML parsing, including auto-detection of XML/XHTML. |
| `--target-fragment` | Supplies the static rendering target for `:target` and `:target-within`. |
| `--page-size WIDTH HEIGHT` and `--page-margin` | Set initial paged-media defaults before document `@page` rules. |
| `--generate-completion SHELL` | Emits shell completion scripts through `clap_complete`. |

### Prince CLI and PDF Comparison

Prince is a second behavioral reference, especially for print-production PDF
controls. Its CSS extensions and language-specific wrappers are not proposed
as Quire API compatibility requirements; this table identifies the portable
capabilities that should inform Quire's public surface.

| Prince surface | Quire equivalent | Status | Work remaining |
| --- | --- | --- | --- |
| `prince [options] INPUT... -o OUTPUT`; `-` for stdin/stdout; multiple inputs combine into one PDF | One input path/URL and one output path | Partial | Add byte stdin/stdout and an explicit multi-document composition API without conflating it with ordinary HTML parsing. |
| `--input=auto\|xml\|html`, `--baseurl`, repeatable `-s` stylesheets | `--input-syntax`, `--base-url`, repeatable `-s` stylesheets | Partial | Rename or alias the compatible input values where useful, accept stdin/base-URL combinations, and load CLI stylesheets at user origin. |
| `--no-network`, `--no-local-files`, `--no-redirects`, `--remap`, and explicit HTTP authentication | `ResourcePolicy` with redirect and fetch-error controls | Partial | Expose a typed resource allowlist, local-file policy, URL remapping, request credentials, and safe custom headers. |
| `--javascript`, external scripts, and CSS script functions | None | Missing | Decide whether deterministic static rendering should support a sandboxed scripting phase before exposing any script API. |
| `--capture` / `--replay` and the control protocol | None | Missing | Add a reproducible, repo-safe render capture format before designing process-control compatibility. |
| `--pdf-profile` for PDF/A, PDF/UA, and PDF/X; `--tagged-pdf` | `--pdf-profile`; `PdfProfile` | Partial | Quire accepts only its current plain-PDF and PDF/A profile modes. Do not accept PDF/UA or PDF/X names until their structures, output intents, and validation are implemented. |
| `--no-compress` | `--uncompressed-pdf`; `PdfCompression::Uncompressed` | Available | Quire disables `/FlateDecode` for every generated stream; keep this debugging behavior separate from PDF optimization policy. |
| `--no-embed-fonts` and `--no-subset-fonts` | `--full-fonts`; `FontEmbeddingMode` | Partial | Preserve Quire's safe embedded-font default; consider an explicit no-subsetting alias, but do not add no-embedding output without a compatibility and conformance policy. |
| PDF metadata, attachments, forms, encryption, output intents, and printer settings | Basic document metadata and selected PDF profile hooks | Missing | Add each feature with its PDF conformance and security constraints rather than mirroring flags independently. |
| Java, .NET, PHP, and other process wrappers | Rust crate API plus CLI | Deliberately different | Keep `Html`/`Document` ownership native to Rust; offer a stable streaming or process protocol only if integration needs justify it. |

## Library API Parity

### WeasyPrint Source, Stylesheet, and Resource API

| WeasyPrint public API | Quire equivalent | Status | Work remaining |
| --- | --- | --- | --- |
| `HTML(filename=...)` | `Html::from_file(path).await` | Available | Preserve this explicit source mode. |
| `HTML(url=...)` | `Html::from_url(url).await` | Partial | Add configurable resource policy and all desired URL schemes. |
| `HTML(string=...)` | `Html::from_string(source)` | Available | Keep strings synchronous and allocation-transparent where practical. |
| `HTML(file_obj=...)` | None | Missing | Add an async byte/reader constructor with an optional source URL/base URL. |
| positional input guessing | Separate constructors | Deliberately different | Do not add ambiguous guessing to the Rust API. A CLI-only compatibility parser is sufficient. |
| `HTML(encoding=...)` | None | Missing | Share decoding configuration with `Css` and resource responses. |
| `HTML(base_url=...)` | `Html::with_base_url` / `with_base_path` | Available | Preserve file and URL forms with explicit error handling. |
| `HTML(url_fetcher=...)` | `ResourcePolicy` controls built-in fetching | Partial | Define a public custom fetcher trait and response type; the built-in policy already controls redirects and strict error handling. |
| `HTML(media_type=...)` | `RenderOptions::media_type` | Partial | Widen media type support and consider a builder to keep per-render options clear. |
| `CSS(filename/url/string/file_obj, font_config=...)` | `Css::from_file`, `from_url`, `from_string` | Partial | Add reader/byte input, decoding, resource policy, and a reusable public font context. |
| user stylesheet origin | `Css::with_user_origin()` | Available | Make CLI and rendering-option stylesheet inputs consistently use it. |
| `Attachment(...)` | None | Missing | Introduce a public attachment value with source, name, description, dates, and relationship. |
| `URLFetcher`, response, fatal error | `ResourcePolicy`, `FetchErrorPolicy` | Partial | Publish a custom async fetcher interface without exposing `reqwest` types. |
| `FontConfiguration` | Internal font-system load | Missing | Introduce a reusable font context only when it can correctly own font discovery and `@font-face` lifetime. |
| `CounterStyle` | Internal parsed counter styles | Missing | Expose a reusable counter-style registry or deliberately keep this renderer-owned and document why. |

### WeasyPrint Rendering and Serialization API

| WeasyPrint public API | Quire equivalent | Status | Work remaining |
| --- | --- | --- | --- |
| `HTML.render(**options)` | `Html::render(&RenderOptions).await` | Partial | Add the missing render options listed above. |
| `HTML.write_pdf(target=None, zoom=1, finisher=..., **options)` | `Html::write_pdf_bytes(&RenderOptions, &PdfOptions).await` and `Html::write_pdf(path, &RenderOptions, &PdfOptions).await` | Partial | Add writer targets, a typed zoom/scaling decision, and a safe finisher extension point. |
| `DEFAULT_OPTIONS` | `RenderOptions::default()` and `PdfOptions::default()` | Partial | Fully document layout and PDF serialization defaults. |
| bytes returned for `target=None` | `write_pdf_bytes` | Available | Keep this direct byte-returning convenience. |
| path or file-object target | Path-only `write_pdf`; bytes can be written by caller | Partial | Add an ergonomic `Write`/async-writer method without forcing a file path. |
| PDF finisher callback | None | Missing | If added, expose a narrowly scoped post-layout/PDF-builder hook that cannot invalidate internal ownership invariants. |
| `zoom` | None | Missing | Decide whether to support WeasyPrint-compatible physical-unit scaling or a clearer PDF page transform API; document that the former changes CSS physical units. |

### WeasyPrint Rendered Document Inspection

| WeasyPrint public API | Quire equivalent | Status | Work remaining |
| --- | --- | --- | --- |
| `Document.pages` | `Document::pages()` | Available | Pages are immutable semantic inspection values. |
| `Document.metadata` | `Document::metadata()` | Partial | Extend source metadata beyond title, one author, and creator. |
| `Document.fonts` | Internal font planning and embedding records | Deliberately different | Font records are renderer implementation details, not a stable crate API. |
| `Document.copy(pages=...)` | None | Missing | Add immutable page selection/concatenation that preserves document-level metadata and resource ownership. |
| `Document.make_bookmark_tree(...)` | Flat `Document::bookmarks()` | Partial | Add a tree view and explicit coordinate-space conversion rather than making callers reconstruct hierarchy. |
| `Document.write_pdf(...)` | `write_pdf_bytes(&PdfOptions)` and `write_pdf(path, &PdfOptions)` | Partial | Add stream targets. |
| `Page.width`, `Page.height` | `Page::width()`, `Page::height()` | Available | Document Quire's unit (PDF points) versus WeasyPrint's CSS pixels. |
| `Page.links` | `Page::links()` returning `LinkAnnotation` values | Partial | Raw targets and PDF-point rectangles are stable; distinguish external, internal, and attachment links later. |
| `Page.anchors` | None | Missing | Retain page-local anchor identifiers and coordinates in the public document model. |
| `Page.bookmarks` | Document-global `bookmarks` | Partial | Offer page-local views if useful after the tree API is established. |
| `Page.bleed` | None | Missing | Retain resolved page bleed widths. |
| `Page.forms` | None | Missing | Retain form geometry/attributes as the prerequisite for PDF forms. |
| detailed paint operations | Internal paint tree and PDF writer records | Deliberately different | Keep renderer graphics, text, images, and fonts crate-private; the public model is semantic inspection only. |

## Prioritized Work Queue

The following order maximizes useful interoperability while keeping architecture
coherent. It is a checklist, not an authorization to weaken PDF/A or CSS
conformance requirements.

1. **Normalize the existing surface.** Rename the clap command/help/examples to
   `quire` (or intentionally retain an alias), make CLI `--stylesheet`
   user-origin, and document public units and defaults.
2. **Create resource boundaries.** Add typed `ResourcePolicy` and an async
   fetcher trait before adding timeout, protocol, redirect, encoding, stdin,
   reader, or cache flags. Every HTML/CSS/font/image fetch must use the same
   policy.
3. **Complete common I/O.** Support stdin/stdout, reader input, writer output,
   explicit encodings, and the compatibility logging flags.
4. **Split render from PDF options.** Keep layout options free of serialization
   concerns, then add compression, identifiers, metadata/XMP, cache, and
   font/image output choices.
5. **Expand the document model.** Add immutable page selection, anchor/link
   types, bookmark-tree construction, and complete metadata extraction before
   exposing a stable inspection API.
6. **Implement semantic PDF features.** Attachments, forms, tagging, output
   intents, PDF/UA, and PDF/X must be implemented together with their PDF
   conformance obligations; do not expose flags that merely write a label.
7. **Broaden PDF profiles last.** Accept a WeasyPrint profile only once Quire
   can emit the profile's required header, metadata, structures, font/image
   restrictions, and validation evidence.

## Maintaining This Tracker

When an interface item changes:

1. Update the row status and the specific remaining work in this file.
2. Update `SPEC_DIVERGENCES.md` if the change reveals or resolves a CSS, HTML,
   SVG, or PDF conformance divergence.
3. Add a local test for new CLI/API behavior, especially source selection,
   cascade origin, and resource-policy behavior.
4. Do not mark a PDF conformance option “Available” solely because the CLI
   accepts it; the resulting PDF must meet the option's required semantics.
5. When changing a Prince comparison row, use current Prince documentation and
   distinguish portable output behavior from Prince-specific CSS extensions or
   wrapper conventions.
