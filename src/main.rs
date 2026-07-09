//! Command-line interface for rendering HTML and XML documents as PDFs.

use clap::{ArgGroup, CommandFactory, Parser, ValueEnum, ValueHint};
use clap_complete::{Shell, generate};
use log::LevelFilter;
use quire::{
    Css, FetchErrorPolicy, FontEmbeddingMode, Html, InputSyntax, MediaType, PageMargins, PageSize,
    PdfCompression, PdfOptions, PdfProfile, RenderOptions, ResourcePolicy, Url,
};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

// Layout recursively traverses CSS box and inline trees. Keep command-line
// rendering off the platform main thread so a valid deeply nested document is
// not limited by the comparatively small process-main stack.
const CLI_RENDER_STACK_SIZE: usize = 32 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "quire",
    version,
    about = "Convert HTML documents to PDF.",
    group(ArgGroup::new("logging").args(["verbose", "debug", "quiet"]).multiple(false))
)]
struct Cli {
    /// Show warnings and information messages.
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    /// Show debugging messages.
    #[arg(short = 'd', long = "debug")]
    debug: bool,

    /// Hide logging messages.
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,

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

    /// Input syntax to use for document parsing.
    #[arg(
        long = "input-syntax",
        value_name = "SYNTAX",
        default_value = "auto",
        value_parser = parse_input_syntax
    )]
    input_syntax: InputSyntax,

    /// PDF output profile to generate.
    #[arg(
        long = "pdf-profile",
        visible_aliases = ["pdf-variant", "pdf-type"],
        value_name = "PROFILE",
        default_value_t = PdfProfile::default(),
        value_parser = clap::value_parser!(PdfProfile)
    )]
    pdf_profile: PdfProfile,

    /// Embed complete font programs when possible instead of subsetting them.
    #[arg(long = "full-fonts")]
    full_fonts: bool,

    /// Do not compress PDF streams, primarily for debugging generated PDF syntax.
    #[arg(long = "uncompressed-pdf")]
    uncompressed_pdf: bool,

    /// Do not follow HTTP(S) redirects while fetching document resources.
    #[arg(long = "no-http-redirects")]
    no_http_redirects: bool,

    /// Continue rendering when optional external resource fetches fail.
    #[arg(long = "allow-fetch-errors")]
    allow_fetch_errors: bool,

    /// Output medium used to evaluate CSS Media Queries.
    #[arg(long = "media-type", value_enum, default_value_t = CliMediaType::Print)]
    media_type: CliMediaType,

    /// Initial page-box width and height, as CSS absolute lengths.
    ///
    /// This is the initial page box used by `@page size: auto` and viewport
    /// units in page rules. Document `@page size` declarations may override it.
    #[arg(
        long = "page-size",
        value_names = ["WIDTH", "HEIGHT"],
        num_args = 2,
        value_parser = parse_absolute_page_length
    )]
    page_size: Option<Vec<f32>>,

    /// Initial page margins in CSS shorthand order, as absolute lengths.
    ///
    /// One to four values map to all, block/inline, top/inline/bottom, or
    /// top/right/bottom/left respectively. Document `@page` declarations may
    /// override these initial margins.
    #[arg(
        long = "page-margin",
        value_name = "LENGTH",
        num_args = 1..=4,
        value_delimiter = ',',
        require_equals = true,
        value_parser = parse_absolute_page_margin_length
    )]
    page_margin: Option<Vec<f32>>,

    /// Generate a shell completion script to standard output.
    #[arg(long = "generate-completion", value_name = "SHELL")]
    generate_completion: Option<Shell>,

    /// Input HTML path, file URL, or HTTP(S) URL.
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
        value_hint = ValueHint::FilePath,
        value_parser = canonicalize_output_path
    )]
    output: Option<PathBuf>,
}

impl Cli {
    /// Map WeasyPrint-compatible command-line logging flags to `log` filters.
    fn log_level(&self) -> LevelFilter {
        if self.quiet {
            LevelFilter::Off
        } else if self.debug {
            LevelFilter::Debug
        } else if self.verbose {
            LevelFilter::Info
        } else {
            LevelFilter::Warn
        }
    }

    fn has_explicit_log_level(&self) -> bool {
        self.verbose || self.debug || self.quiet
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliMediaType {
    Print,
    Screen,
}

impl From<CliMediaType> for MediaType {
    fn from(value: CliMediaType) -> Self {
        match value {
            CliMediaType::Print => Self::Print,
            CliMediaType::Screen => Self::Screen,
        }
    }
}

fn main() {
    let args = Cli::parse();
    initialize_logger(&args);
    let cli_started = Instant::now();
    let result = match thread::Builder::new()
        .name("quire-cli-render".to_string())
        .stack_size(CLI_RENDER_STACK_SIZE)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("CLI runtime construction should succeed")
                .block_on(run(args))
        }) {
        Ok(thread) => match thread.join() {
            Ok(result) => result,
            Err(_) => Err(quire::Error::InvalidInput(
                "CLI rendering thread panicked".to_string(),
            )),
        },
        Err(error) => Err(error.into()),
    };
    log::debug!("quire CLI completed in {:.3?}", cli_started.elapsed());
    if let Err(error) = result {
        log::error!("{error}");
        std::process::exit(1);
    }
}

fn initialize_logger(args: &Cli) {
    let mut logger = if args.has_explicit_log_level() {
        let mut logger = env_logger::Builder::new();
        logger.filter_level(args.log_level());
        logger
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
    };
    logger.init();
}

async fn run(args: Cli) -> quire::Result<()> {
    if let Some(shell) = args.generate_completion {
        let mut command = Cli::command();
        generate(shell, &mut command, "quire", &mut io::stdout());
        return Ok(());
    }

    let resource_policy = ResourcePolicy {
        follow_http_redirects: !args.no_http_redirects,
        error_policy: if args.allow_fetch_errors {
            FetchErrorPolicy::Allow
        } else {
            FetchErrorPolicy::Fail
        },
    };

    let mut stylesheets = Vec::new();

    for location in &args.stylesheets {
        log::debug!("loading stylesheet from {location}");
        stylesheets.push(
            if let Some(url) = parse_resource_url(location) {
                Css::from_url_async_with_resource_policy(url, resource_policy).await?
            } else {
                Css::from_file(location).await?
            }
            .with_resource_policy(resource_policy),
        );
    }

    let input = args
        .input
        .as_deref()
        .ok_or_else(|| quire::Error::InvalidInput("expected input argument".to_string()))?;
    let output = args
        .output
        .as_deref()
        .ok_or_else(|| quire::Error::InvalidInput("expected output argument".to_string()))?;
    let mut html = if let Some(url) = parse_resource_url(input) {
        log::debug!("loading HTML from URL {input}");
        Html::from_url_with_resource_policy(url, resource_policy).await?
    } else {
        log::debug!("loading HTML from {input}");
        Html::from_file(input).await?
    };
    if let Some(base_url) = args.base_url.as_deref() {
        html = if let Ok(url) = Url::parse(base_url) {
            html.with_base_url(url)
        } else {
            html.with_base_path(base_url)?
        };
    }
    html = html
        .with_input_syntax(args.input_syntax)
        .with_resource_policy(resource_policy);
    for stylesheet in stylesheets {
        html = html.with_stylesheet(stylesheet);
    }

    log::info!("writing PDF to {}", output.display());
    let started = Instant::now();
    let mut options = RenderOptions::default();
    options.media_type = args.media_type.into();
    options.presentational_hints = args.presentational_hints;
    options.target_fragment = args.target_fragment;
    let pdf_options = PdfOptions {
        profile: args.pdf_profile,
        font_embedding: if args.full_fonts {
            FontEmbeddingMode::Full
        } else {
            FontEmbeddingMode::Subset
        },
        compression: if args.uncompressed_pdf {
            PdfCompression::Uncompressed
        } else {
            PdfCompression::Compressed
        },
        ..PdfOptions::default()
    };
    if let Some(page_size) = args.page_size {
        // Clap enforces exactly two arguments for `--page-size` above.
        options.page_size = PageSize::from_points(page_size[0], page_size[1]);
    }
    if let Some(page_margin) = args.page_margin {
        options.set_page_margins(page_margins_from_shorthand(&page_margin));
    }
    log::debug!("initial page margins: {:?}", options.page_margins);
    html.write_pdf(output, &options, &pdf_options).await?;
    log::debug!("generated PDF in {:.3?}", started.elapsed());
    Ok(())
}

fn parse_input_syntax(value: &str) -> Result<InputSyntax, String> {
    match value {
        "auto" => Ok(InputSyntax::Auto),
        "html" => Ok(InputSyntax::Html),
        "xml" => Ok(InputSyntax::Xml),
        _ => Err(format!("unsupported input syntax: {value}")),
    }
}

/// Parses CSS absolute lengths for the command-line initial page box.
///
/// CSS Values and Units defines the absolute-length conversions:
/// <https://www.w3.org/TR/css-values-4/#absolute-lengths>.
fn parse_absolute_page_length(value: &str) -> Result<f32, String> {
    let points = parse_absolute_length(value)?;
    if points < 0.0 {
        return Err(format!(
            "page size must be a finite non-negative length, got {value:?}"
        ));
    }
    Ok(points)
}

fn parse_absolute_page_margin_length(value: &str) -> Result<f32, String> {
    parse_absolute_length(value)
}

fn parse_absolute_length(value: &str) -> Result<f32, String> {
    let value = value.trim();
    let (number, factor) = if let Some(number) = value.strip_suffix("px") {
        (number, 72.0 / 96.0)
    } else if let Some(number) = value.strip_suffix("in") {
        (number, 72.0)
    } else if let Some(number) = value.strip_suffix("cm") {
        (number, 72.0 / 2.54)
    } else if let Some(number) = value.strip_suffix("mm") {
        (number, 72.0 / 25.4)
    } else if let Some(number) = value.strip_suffix("q") {
        (number, 72.0 / 101.6)
    } else if let Some(number) = value.strip_suffix("pt") {
        (number, 1.0)
    } else {
        return Err(format!(
            "expected a CSS absolute length such as 5in or 210mm, got {value:?}"
        ));
    };
    let number = number
        .parse::<f32>()
        .map_err(|_| format!("expected a number before the unit in {value:?}"))?;
    let points = number * factor;
    if !points.is_finite() {
        return Err(format!(
            "expected a finite CSS absolute length, got {value:?}"
        ));
    }
    Ok(points)
}

fn page_margins_from_shorthand(values: &[f32]) -> PageMargins {
    let [top, right, bottom, left] = match values {
        [all] => [*all; 4],
        [block, inline] => [*block, *inline, *block, *inline],
        [top, inline, bottom] => [*top, *inline, *bottom, *inline],
        [top, right, bottom, left] => [*top, *right, *bottom, *left],
        _ => unreachable!("clap constrains --page-margin to one through four values"),
    };
    PageMargins::from_points(top, right, bottom, left)
}

fn canonicalize_output_path(value: &str) -> Result<PathBuf, String> {
    let path = Url::parse(value)
        .ok()
        .filter(|url| url.scheme() == "file")
        .and_then(|url| url.to_file_path().ok())
        .unwrap_or_else(|| PathBuf::from(value));
    if path.as_os_str().is_empty() {
        return Err("output path must not be empty".to_string());
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| "output path must include a file name".to_string())?;
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|error| format!("failed to canonicalize output path {value}: {error}"));
    }

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize output directory {parent:?}: {error}"))?;
    Ok(parent.join(file_name))
}

fn parse_resource_url(value: &str) -> Option<Url> {
    Url::parse(value)
        .ok()
        .filter(|url| matches!(url.scheme(), "file" | "http" | "https"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_to_pdfa_2b() {
        let cli = Cli::try_parse_from(["quire", "input.html", "output.pdf"]).unwrap();

        assert_eq!(cli.pdf_profile, PdfProfile::PdfA2B);
        assert!(!cli.verbose);
        assert!(!cli.debug);
        assert!(!cli.quiet);
        assert_eq!(cli.log_level(), LevelFilter::Warn);
        assert!(!cli.full_fonts);
        assert!(!cli.uncompressed_pdf);
        assert!(!cli.no_http_redirects);
        assert!(!cli.allow_fetch_errors);
        assert_eq!(
            cli.output.as_deref().and_then(Path::file_name),
            Some("output.pdf".as_ref())
        );
    }

    #[test]
    fn cli_logging_flags_map_to_weasyprint_levels() {
        let verbose = Cli::try_parse_from(["quire", "-v", "input.html", "output.pdf"]).unwrap();
        assert!(verbose.verbose);
        assert_eq!(verbose.log_level(), LevelFilter::Info);

        let debug = Cli::try_parse_from(["quire", "-d", "input.html", "output.pdf"]).unwrap();
        assert!(debug.debug);
        assert_eq!(debug.log_level(), LevelFilter::Debug);

        let quiet = Cli::try_parse_from(["quire", "-q", "input.html", "output.pdf"]).unwrap();
        assert!(quiet.quiet);
        assert_eq!(quiet.log_level(), LevelFilter::Off);
    }

    #[test]
    fn cli_rejects_combined_logging_flags() {
        for flags in [["-v", "-d"], ["-v", "-q"], ["-d", "-q"]] {
            assert!(
                Cli::try_parse_from(["quire", flags[0], flags[1], "input.html", "output.pdf"])
                    .is_err()
            );
        }
    }

    #[test]
    fn cli_rejects_removed_html_string_flags() {
        for flag in ["--string", "--strings"] {
            assert!(Cli::try_parse_from(["quire", flag, "<p>test</p>", "output.pdf"]).is_err());
        }
    }

    #[test]
    fn cli_accepts_pdf_profile_and_legacy_aliases() {
        let cli = Cli::try_parse_from([
            "quire",
            "--pdf-profile",
            "pdf/a-2u",
            "input.html",
            "output.pdf",
        ])
        .unwrap();

        assert_eq!(cli.pdf_profile, PdfProfile::PdfA2U);

        let variant =
            Cli::try_parse_from(["quire", "--pdf-variant", "pdf", "input.html", "output.pdf"])
                .unwrap();
        assert_eq!(variant.pdf_profile, PdfProfile::Pdf);

        let type_alias =
            Cli::try_parse_from(["quire", "--pdf-type", "pdf", "input.html", "output.pdf"])
                .unwrap();
        assert_eq!(type_alias.pdf_profile, PdfProfile::Pdf);
    }

    #[test]
    fn cli_accepts_full_fonts() {
        let cli =
            Cli::try_parse_from(["quire", "--full-fonts", "input.html", "output.pdf"]).unwrap();

        assert!(cli.full_fonts);
        assert_eq!(FontEmbeddingMode::default(), FontEmbeddingMode::Subset);
        assert_eq!(
            PdfOptions::default().font_embedding,
            FontEmbeddingMode::Subset
        );
    }

    #[test]
    fn cli_accepts_uncompressed_pdf() {
        let cli = Cli::try_parse_from(["quire", "--uncompressed-pdf", "input.html", "output.pdf"])
            .unwrap();

        assert!(cli.uncompressed_pdf);
        assert_eq!(PdfCompression::default(), PdfCompression::Compressed);
        assert_eq!(
            PdfOptions::default().compression,
            PdfCompression::Compressed
        );
    }

    #[test]
    fn cli_accepts_resource_recovery_controls() {
        let cli = Cli::try_parse_from([
            "quire",
            "--no-http-redirects",
            "--allow-fetch-errors",
            "input.html",
            "output.pdf",
        ])
        .unwrap();

        assert!(cli.no_http_redirects);
        assert!(cli.allow_fetch_errors);
        assert!(
            Cli::try_parse_from(["quire", "--fail-on-http-errors", "input.html", "output.pdf",])
                .is_err()
        );
    }

    #[test]
    fn cli_accepts_initial_page_size() {
        let cli = Cli::try_parse_from([
            "quire",
            "--page-size",
            "5in",
            "3in",
            "input.html",
            "output.pdf",
        ])
        .unwrap();

        assert_eq!(cli.page_size, Some(vec![360.0, 216.0]));
    }

    #[test]
    fn cli_rejects_relative_initial_page_size() {
        let error = Cli::try_parse_from([
            "quire",
            "--page-size",
            "100vw",
            "3in",
            "input.html",
            "output.pdf",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("CSS absolute length"));
    }

    #[test]
    fn cli_accepts_page_margin_shorthand() {
        let cli = Cli::try_parse_from([
            "quire",
            "--page-margin=0.5in,12px",
            "input.html",
            "output.pdf",
        ])
        .unwrap();

        assert_eq!(cli.page_margin, Some(vec![36.0, 9.0]));
        assert_eq!(
            page_margins_from_shorthand(cli.page_margin.as_deref().unwrap()),
            PageMargins::from_points(36.0, 9.0, 36.0, 9.0)
        );
    }

    #[test]
    fn cli_accepts_input_syntax() {
        let cli = Cli::try_parse_from([
            "quire",
            "--input-syntax",
            "xml",
            "input.xhtml",
            "output.pdf",
        ])
        .unwrap();

        assert_eq!(cli.input_syntax, InputSyntax::Xml);
    }

    #[test]
    fn cli_rejects_unsupported_pdf_profile() {
        for profile in ["pdf/a-4f", "pdf/ua-1", "pdf/x-4", "debug"] {
            let error = Cli::try_parse_from([
                "quire",
                "--pdf-profile",
                profile,
                "input.html",
                "output.pdf",
            ])
            .unwrap_err();

            assert!(error.to_string().contains("unsupported PDF profile"));
            assert!(error.to_string().contains("pdf/a-3u"));
        }
    }

    #[test]
    fn cli_canonicalizes_output_path() {
        let cli = Cli::try_parse_from(["quire", "input.html", "output.pdf"]).unwrap();
        let output = cli.output.unwrap();

        assert!(output.is_absolute());
        assert_eq!(output.file_name(), Some("output.pdf".as_ref()));
    }

    #[test]
    fn cli_rejects_output_with_missing_parent() {
        let missing_parent = format!("quire-missing-output-parent-{}", std::process::id());
        let output = format!("{missing_parent}/output.pdf");
        let error = Cli::try_parse_from(["quire", "input.html", output.as_str()]).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("failed to canonicalize output directory")
        );
    }
}
