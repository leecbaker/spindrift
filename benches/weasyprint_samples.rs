//! Benchmarks for Spindrift's bundled WeasyPrint-compatible sample documents.

use std::hint::black_box;
use std::path::Path;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use spindrift::{Css, Html, PdfOptions, RenderOptions};
use tokio::runtime::Runtime;

struct Sample {
    name: &'static str,
    path: &'static str,
    stylesheets: &'static [&'static str],
}

struct GridDiagnosticWorkload {
    name: &'static str,
    family: GridDiagnosticFamily,
    baseline: bool,
    floating: bool,
    orthogonal: bool,
}

#[derive(Clone, Copy)]
enum GridDiagnosticFamily {
    MixedBaseline,
    ContentBaseline,
}

const GRID_DIAGNOSTIC_WORKLOADS: &[GridDiagnosticWorkload] = &[
    GridDiagnosticWorkload {
        name: "mixed_baseline_floating_orthogonal",
        family: GridDiagnosticFamily::MixedBaseline,
        baseline: true,
        floating: true,
        orthogonal: true,
    },
    GridDiagnosticWorkload {
        name: "mixed_baseline_neutral_floating_orthogonal",
        family: GridDiagnosticFamily::MixedBaseline,
        baseline: false,
        floating: true,
        orthogonal: true,
    },
    GridDiagnosticWorkload {
        name: "mixed_baseline_nonfloating_orthogonal",
        family: GridDiagnosticFamily::MixedBaseline,
        baseline: true,
        floating: false,
        orthogonal: true,
    },
    GridDiagnosticWorkload {
        name: "mixed_baseline_floating_horizontal",
        family: GridDiagnosticFamily::MixedBaseline,
        baseline: true,
        floating: true,
        orthogonal: false,
    },
    GridDiagnosticWorkload {
        name: "content_baseline_floating_orthogonal",
        family: GridDiagnosticFamily::ContentBaseline,
        baseline: true,
        floating: true,
        orthogonal: true,
    },
    GridDiagnosticWorkload {
        name: "content_baseline_neutral_floating_orthogonal",
        family: GridDiagnosticFamily::ContentBaseline,
        baseline: false,
        floating: true,
        orthogonal: true,
    },
];

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

    let fragmented_grid_document = format!(
        "<!doctype html><meta charset=\"utf-8\"><style>@page {{ size: 120pt 100pt; margin: 10pt }} body, div {{ margin: 0 }} .grid {{ display: grid; grid-template-columns: 100pt; grid-auto-rows: 70pt }} .item {{ height: 210pt; background: #ddd; font: 10pt/10pt sans-serif }}</style><div class=\"grid\">{}</div>",
        "<div class=\"item\">fragmented grid replay</div>".repeat(48),
    );
    group.bench_function("fragmented_grid_replay", |b| {
        b.iter(|| {
            let bytes = runtime.block_on(render_inline_html_pdf(
                &fragmented_grid_document,
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

    let mut group = c.benchmark_group("grid_float_baseline");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    for workload in GRID_DIAGNOSTIC_WORKLOADS {
        let document = grid_diagnostic_document(workload);
        group.bench_function(workload.name, |b| {
            b.iter(|| {
                let bytes = runtime.block_on(render_inline_html_pdf(
                    black_box(&document),
                    &options,
                    &pdf_options,
                ));
                black_box(bytes.len())
            });
        });
    }
    group.finish();
}

/// Build local workloads with the container and item counts of the slow Grid
/// WPT families, without relying on a sibling WPT checkout.
fn grid_diagnostic_document(workload: &GridDiagnosticWorkload) -> String {
    let item_counts: Vec<usize> = match workload.family {
        // 30 grids / 116 items, matching the mixed-baseline cases.
        GridDiagnosticFamily::MixedBaseline => {
            (0..30).map(|index| if index < 4 { 3 } else { 4 }).collect()
        }
        // 36 grids / 90 items, matching the content-baseline case.
        GridDiagnosticFamily::ContentBaseline => (0..36)
            .map(|index| if index < 18 { 3 } else { 2 })
            .collect(),
    };
    let baseline_class = if workload.baseline {
        "baseline"
    } else {
        "neutral"
    };
    let floating_class = if workload.floating {
        "floating"
    } else {
        "nonfloating"
    };
    let mut grids = String::new();
    for (grid_index, item_count) in item_counts.into_iter().enumerate() {
        let grid_writing_mode = if workload.orthogonal && grid_index % 2 == 1 {
            "vertical-rl"
        } else {
            "horizontal-tb"
        };
        grids.push_str(&format!(
            "<div class=\"grid\" style=\"writing-mode:{grid_writing_mode}\">"
        ));
        for item_index in 0..item_count {
            let item_writing_mode = if workload.orthogonal && (grid_index + item_index) % 3 == 0 {
                "vertical-lr"
            } else {
                "horizontal-tb"
            };
            grids.push_str(&format!(
                "<span class=\"item\" style=\"writing-mode:{item_writing_mode}\">{grid_index}<br>{item_index}</span>"
            ));
        }
        grids.push_str("</div>");
    }
    format!(
        "<!doctype html><meta charset=\"utf-8\"><style>
            @page {{ size: 600pt 800pt; margin: 8pt }}
            body {{ margin: 0; font: 14pt/1 monospace }}
            .grid {{ display: grid; grid-template-columns: auto auto; gap: 1pt; margin: 1pt; border: 1pt solid #777; padding: 1pt }}
            .item {{ border: .5pt solid #aaa; padding: 1pt }}
            .floating .grid {{ float: left }}
            .nonfloating .grid {{ float: none }}
            .baseline .grid {{ align-content: baseline; align-items: baseline }}
            .baseline .item:nth-child(even) {{ align-self: last baseline }}
            .neutral .grid, .neutral .item {{ align-content: start; align-items: start; align-self: start }}
        </style><main class=\"{baseline_class} {floating_class}\">{grids}</main>"
    )
}

async fn render_empty_pdf(options: &RenderOptions, pdf_options: &PdfOptions) -> Vec<u8> {
    let mut bytes = Vec::new();
    Html::from_string("<!doctype html><meta charset=\"utf-8\"><title>empty</title>")
        .write_pdf(&mut bytes, options, pdf_options)
        .await
        .expect("render empty document to PDF");
    bytes
}

async fn render_inline_break_all_pdf(
    source: &str,
    options: &RenderOptions,
    pdf_options: &PdfOptions,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    Html::from_string(source)
        .write_pdf(&mut bytes, options, pdf_options)
        .await
        .expect("render long break-all benchmark document to PDF");
    bytes
}

async fn render_inline_html_pdf(
    source: &str,
    options: &RenderOptions,
    pdf_options: &PdfOptions,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    Html::from_string(source)
        .write_pdf(&mut bytes, options, pdf_options)
        .await
        .expect("render inline benchmark document to PDF");
    bytes
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
    let mut bytes = Vec::new();
    html.write_pdf(&mut bytes, options, pdf_options)
        .await
        .unwrap_or_else(|error| panic!("render sample {} to PDF: {error}", sample.path));
    bytes
}

criterion_group!(benches, benchmark_weasyprint_samples);
criterion_main!(benches);
