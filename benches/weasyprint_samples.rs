use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use quire::{Html, RenderOptions};
use std::hint::black_box;
use std::path::Path;
use std::time::Duration;
use tokio::runtime::Runtime;

struct Sample {
    name: &'static str,
    path: &'static str,
}

const SAMPLES: &[Sample] = &[
    Sample {
        name: "book",
        path: "weasyprint-samples/book/book.html",
    },
    Sample {
        name: "invoice",
        path: "weasyprint-samples/invoice/invoice.html",
    },
    Sample {
        name: "letter",
        path: "weasyprint-samples/letter/letter.html",
    },
    Sample {
        name: "poster",
        path: "weasyprint-samples/poster/poster.html",
    },
    Sample {
        name: "report",
        path: "weasyprint-samples/report/report.html",
    },
    Sample {
        name: "ticket",
        path: "weasyprint-samples/ticket/ticket.html",
    },
];

fn benchmark_weasyprint_samples(c: &mut Criterion) {
    let runtime = Runtime::new().expect("create Tokio runtime for Criterion benchmarks");
    let options = RenderOptions::default();

    let mut group = c.benchmark_group("render_pdf");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("empty_document", |b| {
        b.iter(|| {
            let bytes = runtime.block_on(render_empty_pdf(&options));
            black_box(bytes.len())
        });
    });

    for sample in SAMPLES {
        let benchmark_id = BenchmarkId::from_parameter(format!("sample_{}", sample.name));
        group.bench_with_input(benchmark_id, sample, |b, sample| {
            b.iter(|| {
                let bytes = runtime.block_on(render_sample_pdf(sample.path, &options));
                black_box(bytes.len())
            });
        });
    }

    group.finish();
}

async fn render_empty_pdf(options: &RenderOptions) -> Vec<u8> {
    Html::from_string("<!doctype html><meta charset=\"utf-8\"><title>empty</title>")
        .write_pdf_bytes_async(options)
        .await
        .expect("render empty document to PDF")
}

async fn render_sample_pdf(path: &str, options: &RenderOptions) -> Vec<u8> {
    Html::from_file_async(Path::new(path))
        .await
        .unwrap_or_else(|error| panic!("load sample {path}: {error}"))
        .write_pdf_bytes_async(options)
        .await
        .unwrap_or_else(|error| panic!("render sample {path} to PDF: {error}"))
}

criterion_group!(benches, benchmark_weasyprint_samples);
criterion_main!(benches);
