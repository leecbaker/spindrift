//! Benchmarks for Quire's bundled WeasyPrint-compatible sample documents.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use quire::{Css, Html, PdfOptions, RenderOptions};
use std::hint::black_box;
use std::path::Path;
use std::time::Duration;
use tokio::runtime::Runtime;

struct Sample {
    name: &'static str,
    path: &'static str,
    stylesheets: &'static [&'static str],
}

const SAMPLES: &[Sample] = &[
    Sample {
        name: "book",
        path: "weasyprint-samples/book/book.html",
        stylesheets: &["weasyprint-samples/book/book.css"],
    },
    Sample {
        name: "invoice",
        path: "weasyprint-samples/invoice/invoice.html",
        stylesheets: &[],
    },
    Sample {
        name: "letter",
        path: "weasyprint-samples/letter/letter.html",
        stylesheets: &[],
    },
    Sample {
        name: "poster",
        path: "weasyprint-samples/poster/poster.html",
        stylesheets: &["weasyprint-samples/poster/poster.css"],
    },
    Sample {
        name: "report",
        path: "weasyprint-samples/report/report.html",
        stylesheets: &[],
    },
    Sample {
        name: "ticket",
        path: "weasyprint-samples/ticket/ticket.html",
        stylesheets: &[],
    },
];

fn benchmark_weasyprint_samples(c: &mut Criterion) {
    let runtime = Runtime::new().expect("create Tokio runtime for Criterion benchmarks");
    let options = RenderOptions::default();
    let pdf_options = PdfOptions::default();

    let mut group = c.benchmark_group("render_pdf");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("empty_document", |b| {
        b.iter(|| {
            let bytes = runtime.block_on(render_empty_pdf(&options, &pdf_options));
            black_box(bytes.len())
        });
    });

    let break_all_document = format!(
        "<!doctype html><meta charset=\"utf-8\"><style>@page {{ size: 80pt 120pt; margin: 8pt }} p {{ margin: 0; width: 20pt; font: 10pt/10pt sans-serif; word-break: break-all }}</style><p>{}</p>",
        "abcdefghijklmnopqrstuvwxyz0123456789".repeat(512),
    );
    group.bench_function("inline_break_all_long", |b| {
        b.iter(|| {
            let bytes = runtime.block_on(render_inline_break_all_pdf(
                &break_all_document,
                &options,
                &pdf_options,
            ));
            black_box(bytes.len())
        });
    });

    for sample in SAMPLES {
        let benchmark_id = BenchmarkId::from_parameter(format!("sample_{}", sample.name));
        group.bench_with_input(benchmark_id, sample, |b, sample| {
            b.iter(|| {
                let bytes = runtime.block_on(render_sample_pdf(sample, &options, &pdf_options));
                black_box(bytes.len())
            });
        });
    }

    group.finish();
}

async fn render_empty_pdf(options: &RenderOptions, pdf_options: &PdfOptions) -> Vec<u8> {
    Html::from_string("<!doctype html><meta charset=\"utf-8\"><title>empty</title>")
        .write_pdf_bytes(options, pdf_options)
        .await
        .expect("render empty document to PDF")
}

async fn render_inline_break_all_pdf(
    source: &str,
    options: &RenderOptions,
    pdf_options: &PdfOptions,
) -> Vec<u8> {
    Html::from_string(source)
        .write_pdf_bytes(options, pdf_options)
        .await
        .expect("render long break-all benchmark document to PDF")
}

async fn render_sample_pdf(
    sample: &Sample,
    options: &RenderOptions,
    pdf_options: &PdfOptions,
) -> Vec<u8> {
    let mut html = Html::from_file(Path::new(sample.path))
        .await
        .unwrap_or_else(|error| panic!("load sample {}: {error}", sample.path));
    for stylesheet in sample.stylesheets {
        let stylesheet = Css::from_file(Path::new(stylesheet))
            .await
            .unwrap_or_else(|error| panic!("load stylesheet {stylesheet}: {error}"));
        html = html.with_stylesheet(stylesheet);
    }
    html.write_pdf_bytes(options, pdf_options)
        .await
        .unwrap_or_else(|error| panic!("render sample {} to PDF: {error}", sample.path))
}

criterion_group!(benches, benchmark_weasyprint_samples);
criterion_main!(benches);
