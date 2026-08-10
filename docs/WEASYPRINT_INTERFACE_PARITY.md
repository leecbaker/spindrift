# WeasyPrint Interface Parity

This document compares the public command-line and library interfaces of
WeasyPrint with Quire's current public API. It is an API/UX compatibility
tracker, not a claim that equivalent options produce equivalent layout or PDF
output. Rendering and PDF conformance gaps belong in
[`SPEC_DIVERGENCES.md`](../SPEC_DIVERGENCES.md).

Quire is intentionally Rust-native. Matching WeasyPrint's useful operations is
valuable, but matching Python call shapes, dynamically typed keyword arguments,
or pydyf internals is not a goal.

## Sources and Status

Last verified: 2026-08-08, against the current working tree and WeasyPrint
stable 69.0.

- [WeasyPrint API reference, stable 69.0](https://doc.courtbouillon.org/weasyprint/stable/api_reference.html)
- [WeasyPrint manual page, stable 69.0](https://doc.courtbouillon.org/weasyprint/stable/manpage.html)
- The checked-in `Weasyprint/` source for implementation research.

“Available” means an equivalent public operation is usable now. “Partial”
means the public operation exists but has a material behavioral or capability
difference. “Missing” means there is no public equivalent. “Deliberately
different” means Quire provides a more appropriate Rust boundary instead of
emulating a Python convenience.

## Quire API Shape

Quire separates source loading, resource policy, render configuration, and PDF
serialization. In contrast, WeasyPrint passes most of these as keyword
arguments to `HTML.render()` and `HTML.write_pdf()`.

```rust,no_run
use quire::{Css, FetchErrorPolicy, Html, HttpRequestTimeout, PdfOptions, RenderOptions, ResourcePolicy};

# async fn render() -> quire::Result<Vec<u8>> {
let resource_policy = ResourcePolicy {
    follow_http_redirects: false,
    http_timeout: HttpRequestTimeout::try_from(std::time::Duration::from_secs(5))
        .expect("five seconds is non-zero"),
    error_policy: FetchErrorPolicy::Allow,
};
let stylesheet = Css::from_file("print.css")
    .await?
    .with_user_origin()
    .with_resource_policy(resource_policy);
let html = Html::from_file("report.html")
    .await?
    .with_resource_policy(resource_policy)
    .with_stylesheet(stylesheet);

let document = html.render(&RenderOptions::default()).await?;
let mut pdf = Vec::new();
document.write_pdf(&mut pdf, &PdfOptions::default())?;
# }
```

- `Html` and `Css` offer explicit string, local-path, and URL constructors.
  They avoid WeasyPrint's positional input guessing.
- `ResourcePolicy` controls redirect handling, a non-zero per-request HTTP(S)
  timeout, and whether optional stylesheet/font failures may be recovered from.
  It does not yet expose a custom fetcher, protocol list, headers, TLS context,
  or disk cache.
- `RenderOptions` owns parsing/cascade/layout inputs. Its public settings
  include media type, colour-scheme preference, forced-colours mode, initial
  page margins, presentational hints, fragment target, and device resolution.
  Page size is author CSS (`@page size`), not an API input.
- `PdfOptions` owns PDF-profile selection, font embedding, compression, and
  producer metadata. A rendered `Document` can be serialized more than once
  with different PDF options.
- `Document` exposes metadata and bookmarks. Pages, link annotations, paint,
  and PDF-writer internals remain private.

## CLI Parity

WeasyPrint uses `weasyprint [options] <input> <output>`. Quire's binary and
clap command name are `quire`.

### Inputs, Outputs, and Diagnostics

| WeasyPrint surface | Quire equivalent | Status | Difference / remaining work |
| --- | --- | --- | --- |
| `<input>` filename, URL, or `-` for stdin | Local path, `file:`, `http:`, or `https:` URL | Partial | Add `-` byte input. The CLI does not accept `data:` even though the library URL constructors do. Literal HTML remains library-only. |
| `<output>` filename or `-` for stdout | Canonicalized filesystem path | Partial | Add `-` and write PDF bytes to stdout while keeping diagnostics on stderr. |
| `--version` | clap-generated version | Available | Both report a version and exit. |
| `-i`, `--info` | `-i`, `--info` | Available | Reports the runtime OS type, version, and architecture plus Quire's version. Unlike WeasyPrint, it omits Rust dependency versions because Quire compiles its rendering stack into the binary. |
| `-v`, `--verbose`; `-d`, `--debug`; `-q`, `--quiet` | Mutually exclusive flags using `env_logger` | Available | The flags map to `info`, `debug`, and `off`; without one, `RUST_LOG` remains available. |

### Rendering and PDF Options

| WeasyPrint option | Quire equivalent | Status | Difference / remaining work |
| --- | --- | --- | --- |
| `-s`, `--stylesheet` (repeatable user stylesheet) | `-s`, `--stylesheet` (repeatable) | Partial | CLI stylesheets are currently loaded as author-origin `Css`. Call `Css::with_user_origin()` in the CLI to match WeasyPrint cascade precedence. |
| `-a`, `--attachment`; `--attachment-relationship` | None | Missing | Add a typed attachment input and embedded-file serialization, including ISO 32000 associated-file relationships. |
| `--pdf-identifier` | None | Missing | Add an explicit trailer `/ID` policy and document deterministic/default behaviour. |
| `--pdf-variant` | `--pdf-profile`, with `--pdf-variant` and `--pdf-type` aliases | Partial | Quire accepts `pdf`, PDF/A-1b, -2b, -3b, -2u, and -3u. These choose writer behaviour and identification metadata, but do not yet establish PDF/A conformance. Do not accept WeasyPrint's additional A/4, PDF/UA, PDF/X, or `debug` values until their required structures and validation exist. |
| `--pdf-version` | Profile-selected PDF version | Partial | `PdfProfile` selects 1.4 or 1.7. Expose an explicit version only with validation of profile/version combinations. |
| `--pdf-forms` | None | Missing | Preserve form semantics through layout and emit AcroForm fields and appearances. |
| `--pdf-tags` | None | Missing | Add the tagged-PDF structure tree, roles, annotation linkage, and validation required for PDF/UA. |
| `--uncompressed-pdf` | `--uncompressed-pdf`; `PdfOptions::compression` | Available | `PdfCompression::Uncompressed` omits `/FlateDecode` from generated streams. |
| `--xmp-metadata`; `--custom-metadata` | None | Missing | Add XMP fragment handling and complete HTML metadata extraction with merge/validation rules. |
| `--output-intent` | None | Missing | Add ICC/profile resolution and PDF output-intent serialization. |
| `-p`, `--presentational-hints` | Always enabled for HTML documents | Deliberately different | Quire follows HTML's rendering model; XML documents do not receive HTML hint mappings. |
| `--optimize-images`; `--jpeg-quality`; `--dpi` | None | Missing | Add opt-in image optimization, re-encoding, and downsampling without changing CSS intrinsic geometry or violating conformance targets. |
| `--full-fonts` | `--full-fonts`; `PdfOptions::font_embedding` | Available | `FontEmbeddingMode::Subset` is default; `Full` keeps complete programs where PDF embedding permits it. |
| `--hinting` | None | Missing | Model hint preservation as an explicit font-subsetting policy. |
| `-c`, `--cache-folder` | Per-render in-memory resource cache | Partial | The cache is not public and has no disk-backed configuration, lifecycle, or limits. |

### HTML and Resource Fetching

| WeasyPrint option | Quire equivalent | Status | Difference / remaining work |
| --- | --- | --- | --- |
| `-e`, `--encoding` | UTF-8 input only | Missing | Add explicit input decoding and HTTP charset handling for HTML and CSS. |
| `-m`, `--media-type` | `--media-type print\|screen` | Partial | The crate has typed `MediaType`; the CLI only exposes `print` and `screen`. Widen it if arbitrary media types are intended to be supported. |
| `-u`, `--base-url` | `-u`, `--base-url` | Available | Quire accepts a URL or local path. Add and test the stdin default when stdin support exists. |
| `-t`, `--timeout` | `-t`, `--timeout`; `ResourcePolicy::http_timeout` | Available | Quire uses a typed non-zero `HttpRequestTimeout` and defaults to 10 seconds. |
| `--allowed-protocols` | Fixed `data`, `file`, `http`, and `https` set in the library | Partial | Make the allowlist public and configurable while retaining a safe default. |
| `--no-http-redirects` | `--no-http-redirects`; `ResourcePolicy::follow_http_redirects` | Available | The final response URL becomes the base URL for URL-loaded HTML and CSS. |
| `--fail-on-http-errors` | Strict primary/explicit sources by default; `--allow-fetch-errors` / `FetchErrorPolicy::Allow` recover from optional stylesheet/font failures | Deliberately different | Failed visual-asset preloads remain unavailable replaced elements so layout can proceed; this is not a blanket “ignore all failures” mode. |

### Quire-only CLI Features

| Quire option | Purpose |
| --- | --- |
| `--target-fragment` | Supplies the static rendering target for `:target` and `:target-within`. |
| `--forced-colors none\|light\|dark` | Sets the CSS forced-colours environment. |
| `--generate-completion SHELL` | Emits shell completions through `clap_complete`. |

## Library API Parity

### Source, Stylesheet, and Resource API

| WeasyPrint public API | Quire equivalent | Status | Difference / remaining work |
| --- | --- | --- | --- |
| `HTML(filename=...)` | `Html::from_file(path).await` | Available | The source path establishes the base URL. |
| `HTML(url=...)` | `Html::from_url(url).await`; `from_url_with_resource_policy` | Partial | Supports `data`, `file`, `http`, and `https`; expose the remaining fetch controls through a public custom fetcher/policy. |
| `HTML(string=...)` | `Html::from_string(source)` | Available | Source strings are synchronous. `from_xml_string` explicitly selects XML/XHTML. |
| `HTML(file_obj=...)` | None | Missing | Add an async byte/reader constructor with optional source/base URL. |
| positional input guessing | Separate constructors | Deliberately different | Do not add ambiguous guessing to the Rust API. |
| `HTML(encoding=...)` | None | Missing | Share a decoding policy with CSS and fetched resource responses. |
| `HTML(base_url=...)` | `Html::with_base_url`; `with_base_path` | Available | The explicit methods make URL and local-directory bases distinct. |
| `HTML(url_fetcher=...)` | `ResourcePolicy` | Partial | Quire controls redirects and error recovery, but has no public custom async fetcher or response type. |
| `HTML(media_type=...)` | `RenderOptions::media_type` | Partial | Quire's typed public media choices are currently `print` and `screen`; WeasyPrint accepts a string. |
| `CSS(filename/url/string/file_obj, font_config=...)` | `Css::from_file`, `from_url`, `from_url_with_resource_policy`, `from_string` | Partial | URL constructors support the same four built-in schemes. Add reader/bytes, decoding, a public URL base setter, and reusable font context if those boundaries are needed. |
| user stylesheet origin | `Css::with_user_origin()` | Available | The library supports it; the CLI must apply it to `--stylesheet`. |
| `Attachment(...)` | None | Missing | Introduce a public attachment value with source, name, description, timestamps, and relationship. |
| `URLFetcher` / response / fatal error | `ResourcePolicy`, `FetchErrorPolicy` | Partial | Define a public async fetch trait and response type without leaking `reqwest` types. |
| `FontConfiguration` | Internal font loading | Missing | Expose a reusable font context only when it can own font discovery and `@font-face` lifetime correctly. |
| `CounterStyle` | Internal parsed counter styles | Missing | Expose a reusable registry or deliberately keep it renderer-owned. |

### Rendering and Serialization API

| WeasyPrint public API | Quire equivalent | Status | Difference / remaining work |
| --- | --- | --- | --- |
| `HTML.render(font_config=..., counter_style=..., color_profiles=..., **options)` | `Html::render(&RenderOptions).await` | Partial | Render options cover several CSS environments, but no public font configuration, counter-style registry, colour-profile context, stylesheet list, or cache input exists. |
| `HTML.write_pdf(target=None, zoom=1, finisher=..., **options)` | `Html::write_pdf(&mut impl Write, &RenderOptions, &PdfOptions).await` | Partial | Split render and PDF policy is deliberate. A writer target supports files, buffers, and adapters; scaling and a safe post-processing extension point remain. |
| `DEFAULT_OPTIONS` | `RenderOptions::default()`; `PdfOptions::default()` | Partial | The split is clearer than one dynamic map; finish documenting all public defaults. |
| bytes returned for `target=None` | Caller supplies `Vec<u8>` to `write_pdf` | Available | This avoids a separate byte-returning convenience API. |
| path or file-object target | `&mut impl Write` | Available | Files, in-memory buffers, and writer adapters share one serialization API. |
| PDF finisher callback | None | Missing | A future hook must not expose mutable writer internals or bypass PDF conformance checks. |
| `zoom` | None | Missing | Decide whether to offer WeasyPrint-compatible CSS-unit scaling or an explicit PDF transform; WeasyPrint's zoom also changes physical CSS units. |

### Rendered Document Inspection

| WeasyPrint public API | Quire equivalent | Status | Difference / remaining work |
| --- | --- | --- | --- |
| `Document.pages` and all `Page` inspection data | None | Deliberately different | Quire keeps page geometry, page-local bookmarks, links, anchors, bleed, forms, and paint state renderer-private. Add a public page model only if a stable semantic use case requires one. |
| `Document.metadata` | `Document::metadata()` | Partial | Quire exposes title, one author, and creator. See [Document Metadata Parity](#document-metadata-parity) for the field-by-field comparison and target surface. |
| `Document.fonts` | Internal font planning/embedding records | Deliberately different | Font records are renderer implementation details, not a stable public model. |
| `Document.copy(pages=...)` | None | Deliberately different | Quire has no public page model, so it does not expose page selection or concatenation. Reconsider only with a stable semantic document-composition API. |
| `Document.make_bookmark_tree(...)` | Flat `Document::bookmarks()` | Partial | Add a typed tree and an explicit coordinate-space conversion rather than requiring callers to reconstruct hierarchy. |
| `Document.write_pdf(...)` | `write_pdf(&mut impl Write, &PdfOptions)` | Available | Callers own output allocation and destination selection. |
| detailed paint operations | Internal paint tree and PDF writer records | Deliberately different | Keep graphics, text, images, and fonts private; the public model is semantic inspection. |

### Document Metadata Parity

WeasyPrint exposes document-wide metadata through `Document.metadata` and
uses it for PDF information and XMP. The [WeasyPrint API reference](https://doc.courtbouillon.org/weasyprint/stable/api_reference.html#weasyprint.document.DocumentMetadata)
lists the current public attributes and their HTML sources. Quire should keep
this data document-level and independent from page/paint inspection.

| WeasyPrint public metadata | Quire today | Parity-oriented Quire surface |
| --- | --- | --- |
| `title`: first `<title>`; PDF `/Title` | `DocumentMetadata::title() -> Option<&str>`; extracts the first `<title>` and writes `/Title` plus `dc:title` XMP | Retain `title()`. |
| `authors`: every `<meta name=author>` in source order; PDF `/Author` | `author() -> Option<&str>`; extracts only the first author and writes one `/Author` value plus a one-item `dc:creator` sequence | Replace the singular stored value with `authors() -> &[String]`. Preserve source order; match WeasyPrint by joining authors with `", "` for the single PDF `/Author` string, while emitting one ordered `rdf:li` per author in the XMP `dc:creator` sequence. |
| `description`: first `<meta name=description>`; PDF `/Subject` | `description() -> Option<&str>`; writes `/Subject` and `dc:description` XMP | Available. |
| `keywords`: comma-separated `<meta name=keywords>` values; PDF `/Keywords` | `keywords() -> &[String]`; splits every keywords meta value, trims HTML whitespace, omits empty values, and preserves first occurrence order; writes comma-joined `/Keywords` and `pdf:Keywords` XMP | Available. |
| `generator`: first `<meta name=generator>`; PDF `/Creator` | `creator() -> Option<&str>` extracts this value and writes `/Creator` plus `xmp:CreatorTool` | Keep the source semantics but rename the public accessor to `generator()` (or add it as the preferred spelling). `creator` is ambiguous next to the PDF `/Creator` field. |
| `created` and `modified`: `<meta name=dcterms.created>` / `dcterms.modified` using W3C's ISO 8601 profile; PDF `/CreationDate` / `/ModDate` | `created()` / `modified()` return validated `DocumentDate` values; writes matching information fields and `xmp:CreateDate` / `xmp:ModifyDate` | Available for the six W3C profile forms. Fractional seconds are retained in XMP; PDF information dates use their whole-second precision. |
| `lang`: `<html lang>` as a BCP 47 tag | `DocumentMetadata::language() -> Option<&str>`; extracts a non-empty root `lang` value, writes the PDF catalog `/Lang`, and emits one `dc:language` XMP value | Partial: preserve the source value today; validate BCP 47 before asserting full conformance. |
| `attachments`: `<link rel=attachment>` values and explicit attachment options | No public attachment model or PDF embedded-file output | Add `attachments() -> &[Attachment]` and a typed `Attachment` input shared by HTML extraction and `PdfOptions`; include name, description, dates, MIME type, and associated-file relationship. |
| `custom`: other named HTML meta values | Not extracted or exposed | Add a deterministic custom-metadata collection. Define case handling, duplicate behavior, and whether unsupported values are rejected or retained before mapping to PDF/XMP extension properties. |
| `xmp_metadata`: caller-provided XML packet fragments | Quire generates a private XMP packet from title, author, creator, producer, and PDF/A identification, but callers cannot inspect or add to it | Add explicit XMP input/merge rules at the PDF-serialization boundary. Preserve Quire-owned conformance and mirrored information fields as authoritative. |
| `generate_rdf_metadata()` and `include_in_pdf()` helpers | No public equivalent; the writer emits information and XMP internally | Do not expose a mutable PDF-writer handle for call-shape parity. If applications need preflight, expose an immutable metadata/XMP generation API with validation rather than an `include_in_pdf` escape hatch. |
| Writer producer (not a WeasyPrint `DocumentMetadata` attribute) | `PdfOptions::producer`, written to PDF `/Producer` and `pdf:Producer` XMP | Retain this Quire extension. Keep it separate from source `generator`: the former identifies the PDF writer, while the latter describes source-generating software. |

The target data model should make repeated and typed values explicit rather
than reproducing WeasyPrint's mutable Python attributes. In particular,
authors, keywords, attachments, custom values, dates, and language must not be
collapsed into a single `String`. Source extraction, public inspection, PDF
information, and XMP serialization should consume the same immutable metadata
record so PDF/A mirrored fields cannot drift.

## Prioritized Interface Work

1. **Correct existing compatibility behaviour.** Load CLI `--stylesheet`
   inputs at user origin and document PDF-point versus CSS-pixel inspection
   units prominently.
2. **Complete common I/O.** Add stdin/stdout, reader/byte sources, writer
   targets, and an encoding policy with locally testable behaviour.
3. **Finish the resource boundary.** Add a typed custom fetcher before adding
   protocol, headers, TLS, and disk-cache controls. Every HTML, CSS, font, and
   image fetch must use the same policy.
4. **Expand document-level inspection.** Add complete metadata and
   bookmark-tree construction before making broader inspection guarantees. Keep
   pages, links, and paint data private unless a stable semantic API requires
   them.
5. **Implement semantic PDF features together.** Attachments, forms, tagging,
   output intents, PDF/UA, and PDF/X require their supporting document model,
   writer structures, and validation evidence—not just flags.
6. **Broaden profiles last.** Accept an additional WeasyPrint profile only when
   Quire emits all required header, metadata, structure, font/image, and
   validation behaviour.

## Maintaining This Tracker

When an interface item changes:

1. Update its status and the specific remaining work here.
2. Update `SPEC_DIVERGENCES.md` if the change reveals or resolves a CSS, HTML,
   SVG, or PDF conformance divergence.
3. Add a local test for new CLI/API behaviour, especially source selection,
   cascade origin, and resource-policy behaviour.
4. Do not mark a PDF conformance option “Available” merely because its CLI
   spelling parses; the emitted PDF must meet its required semantics.
