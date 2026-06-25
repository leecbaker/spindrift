use clap::{CommandFactory, Parser, ValueHint};
use clap_complete::{Shell, generate};
use quire::{Css, Html, RenderOptions, file_url_to_path};
use std::io;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Parser)]
#[command(name = "reasyprint", version, about = "Convert HTML documents to PDF.")]
struct Cli {
    /// Treat the input argument as an HTML string instead of a path or URL.
    #[arg(long = "string")]
    from_string: bool,

    /// Enable HTML presentational hints.
    #[arg(short = 'p', long = "presentational-hints")]
    presentational_hints: bool,

    /// URL fragment target for :target and :target-within selectors.
    #[arg(long = "target-fragment", value_name = "FRAGMENT")]
    target_fragment: Option<String>,

    /// Attach an additional author stylesheet. Can be repeated.
    #[arg(
        short = 's',
        long = "stylesheet",
        value_name = "STYLESHEET",
        value_hint = ValueHint::FilePath
    )]
    stylesheets: Vec<String>,

    /// Base URL/path used to resolve document-relative resources.
    #[arg(
        short = 'u',
        long = "base-url",
        value_name = "BASE_URL",
        value_hint = ValueHint::DirPath
    )]
    base_url: Option<String>,

    /// Generate a shell completion script to standard output.
    #[arg(long = "generate-completion", value_name = "SHELL")]
    generate_completion: Option<Shell>,

    /// Input HTML string, path, file URL, or HTTP URL.
    #[arg(
        value_name = "INPUT",
        required_unless_present = "generate_completion",
        value_hint = ValueHint::AnyPath
    )]
    input: Option<String>,

    /// Output PDF path.
    #[arg(
        value_name = "OUTPUT",
        required_unless_present = "generate_completion",
        value_hint = ValueHint::FilePath
    )]
    output: Option<String>,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let cli_started = Instant::now();
    let result = run().await;
    log::debug!("reasyprint CLI completed in {:.3?}", cli_started.elapsed());
    if let Err(error) = result {
        log::error!("{error}");
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> quire::Result<()> {
    let args = Cli::parse();
    if let Some(shell) = args.generate_completion {
        let mut command = Cli::command();
        generate(shell, &mut command, "reasyprint", &mut io::stdout());
        return Ok(());
    }

    let mut stylesheets = Vec::new();

    for path in &args.stylesheets {
        log::debug!("loading stylesheet from {path}");
        stylesheets.push(
            Css::from_file_async(file_url_to_path(path).unwrap_or_else(|| path.into())).await?,
        );
    }

    let base_url = args.base_url.as_deref().map(|path| {
        file_url_to_path(path)
            .unwrap_or_else(|| path.into())
            .to_string_lossy()
            .to_string()
    });

    let input = args
        .input
        .as_deref()
        .ok_or_else(|| quire::Error::InvalidInput("expected input argument".to_string()))?;
    let output = args
        .output
        .as_deref()
        .ok_or_else(|| quire::Error::InvalidInput("expected output argument".to_string()))?;
    let mut html =
        if !args.from_string && (input.starts_with("file://") || input.starts_with("http://")) {
            log::debug!("loading HTML from URL {input}");
            Html::from_url_async(input).await?
        } else if args.from_string || looks_like_html(input) || !Path::new(input).exists() {
            log::debug!("reading HTML from command-line string");
            Html::from_string(input)
        } else {
            log::debug!("loading HTML from {input}");
            Html::from_file_async(input).await?
        };
    if let Some(base_url) = base_url {
        html = html.with_base_url(base_url);
    }
    for stylesheet in stylesheets {
        html = html.with_stylesheet(stylesheet);
    }

    log::info!("writing PDF to {output}");
    let started = Instant::now();
    let options = RenderOptions {
        presentational_hints: args.presentational_hints,
        target_fragment: args.target_fragment,
        ..RenderOptions::default()
    };
    html.write_pdf_async(output, &options).await?;
    log::debug!("generated PDF in {:.3?}", started.elapsed());
    Ok(())
}

fn looks_like_html(input: &str) -> bool {
    input.contains('<') && input.contains('>')
}
