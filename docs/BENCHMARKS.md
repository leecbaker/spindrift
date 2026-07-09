# Benchmarks

Criterion benchmarks live in `benches/weasyprint_samples.rs` and are run
through Cargo's standard bench command.

Run the full sample suite:

```sh
cargo bench --bench weasyprint_samples
```

Run only the empty-document initialization benchmark:

```sh
cargo bench --bench weasyprint_samples -- render_pdf/empty_document
```

Run one WeasyPrint sample benchmark:

```sh
cargo bench --bench weasyprint_samples -- render_pdf/sample_invoice
cargo bench --bench weasyprint_samples -- render_pdf/sample_letter
cargo bench --bench weasyprint_samples -- render_pdf/sample_poster
cargo bench --bench weasyprint_samples -- render_pdf/sample_report
cargo bench --bench weasyprint_samples -- render_pdf/sample_ticket
```

The `book` sample is included as `render_pdf/sample_book`, but it is currently
much slower than the other samples. Prefer running it explicitly when
investigating book-layout performance:

```sh
cargo bench --bench weasyprint_samples -- render_pdf/sample_book
```

Each benchmark measures the full `Html` input to PDF byte path, including file
loading, HTML/CSS parsing, resource/font loading, layout, drawing, and PDF
serialization. The empty-document benchmark intentionally exercises the same
path with a minimal HTML string so it captures the renderer's one-time setup
costs, especially font-system initialization.

The benchmark uses the sample project's authoring configuration: `book` loads
`book.css` and `poster` loads `poster.css` as explicit author stylesheets.
Those sample HTML files do not link their companion stylesheets themselves.
