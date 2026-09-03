//! Command-line interface for rendering HTML and XML documents as PDFs.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use clap::{ArgGroup, CommandFactory, Parser, ValueEnum, ValueHint};
use clap_complete::{Shell, generate};
use log::LevelFilter;
use quire::{
    Css, FetchErrorPolicy, FontEmbeddingMode, ForcedColorPalette, ForcedColorsMode, Html,
    HttpRequestTimeout, MediaType, PdfCompression, PdfOptions, PdfProfile, RenderOptions,
    ResourcePolicy, Url,
};

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

    /// URL fragment target for :target and :target-within selectors.
    #[arg(long = "target-fragment", value_name = "FRAGMENT")]
    target_fragment: Option<String>,

    /// Attach an additional user stylesheet. Can be repeated.
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

    /// Time limit in whole seconds for each HTTP(S) resource request.
    #[arg(
        short = 't',
        long = "timeout",
        value_name = "SECONDS",
        value_parser = parse_http_request_timeout
    )]
    http_timeout: Option<HttpRequestTimeout>,

    /// Continue rendering when optional external resource fetches fail.
    #[arg(long = "allow-fetch-errors")]
    allow_fetch_errors: bool,

    /// Output medium used to evaluate CSS Media Queries.
    #[arg(long = "media-type", value_enum, default_value_t = CliMediaType::Print)]
    media_type: CliMediaType,

    /// Initial CSS-pixel viewport used for media queries and viewport-relative
    /// page descriptors before document `@page` rules choose a page size.
    #[arg(
        long = "initial-viewport-size",
        value_names = ["WIDTH", "HEIGHT"],
        num_args = 2
    )]
    initial_viewport_size: Option<Vec<f32>>,

    /// Forced-colors palette used for CSS CssColor Adjustment.
    #[arg(long = "forced-colors", value_enum, default_value_t = CliForcedColors::None)]
    forced_colors: CliForcedColors,

    /// Generate a shell completion script to standard output.
    #[arg(
        long = "generate-completion",
        value_name = "SHELL",
        conflicts_with = "info"
    )]
    generate_completion: Option<Shell>,

    /// Print system information useful in bug reports and exit.
    #[arg(short = 'i', long = "info", conflicts_with = "generate_completion")]
    info: bool,

    /// Input HTML path, file URL, or HTTP(S) URL.
    #[arg(
        value_name = "INPUT",
        required_unless_present_any = ["generate_completion", "info"],
        value_hint = ValueHint::AnyPath
    )]
    input: Option<String>,

    /// Output PDF path.
    #[arg(
        value_name = "OUTPUT",
        required_unless_present_any = ["generate_completion", "info"],
        value_hint = ValueHint::FilePath
    )]
    output: Option<String>,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliForcedColors {
    None,
    Light,
    Dark,
}

impl From<CliForcedColors> for ForcedColorsMode {
    fn from(value: CliForcedColors) -> Self {
        match value {
            CliForcedColors::None => Self::Inactive,
            CliForcedColors::Light => Self::Active(ForcedColorPalette::LIGHT),
            CliForcedColors::Dark => Self::Active(ForcedColorPalette::DARK),
        }
    }
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
    if args.info {
        if let Err(error) = print_info_report(&mut io::stdout()) {
            eprintln!("failed to write info report: {error}");
            std::process::exit(1);
        }
        return;
    }

    initialize_logger(&args);
    let cli_started = Instant::now();
    let result = match thread::Builder::new()
        .name("quire-cli-render".to_string())
        // Recursive CSS layout and paint traversal can exceed macOS's small
        // default spawned-thread stack for deeply nested reports. This is a
        // bounded rendering worker, so reserve a stable 32 MiB stack rather
        // than depending on the caller's `RUST_MIN_STACK` environment.
        .stack_size(32 * 1024 * 1024)
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
    // ICU4X's CJK fallback is expected with the data embedded by its line
    // segmenter. Do not flood the CLI with one dependency warning per run.
    logger.filter_module("icu_provider", LevelFilter::Off);
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
        http_timeout: args.http_timeout.unwrap_or_default(),
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
                Css::from_url_with_resource_policy(url, resource_policy).await?
            } else {
                Css::from_file(location).await?
            }
            .with_resource_policy(resource_policy)
            .with_user_origin(),
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
    let output = canonicalize_output_path(output).map_err(quire::Error::InvalidInput)?;
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
    html = html.with_resource_policy(resource_policy);
    for stylesheet in stylesheets {
        html = html.with_stylesheet(stylesheet);
    }

    log::info!("writing PDF to {}", output.display());
    let started = Instant::now();
    let mut options = RenderOptions::default();
    options.media_type = args.media_type.into();
    if let Some(viewport) = args.initial_viewport_size {
        let [width, height]: [f32; 2] = viewport
            .try_into()
            .expect("clap enforces exactly two initial viewport dimensions");
        options.set_initial_viewport_size(quire::CssViewportSize::new(width, height))?;
    }
    options.forced_colors = args.forced_colors.into();
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
    let mut output_file = std::fs::File::create(&output)?;
    html.write_pdf(&mut output_file, &options, &pdf_options)
        .await?;
    log::debug!("generated PDF in {:.3?}", started.elapsed());
    Ok(())
}

/// Write diagnostic host and binary information without initializing rendering.
fn print_info_report(output: &mut impl Write) -> io::Result<()> {
    let system = os_info::get();
    write_info_report(output, &system)
}

/// Format the stable CLI information report.
fn write_info_report(output: &mut impl Write, system: &os_info::Info) -> io::Result<()> {
    let architecture = system.architecture().unwrap_or(std::env::consts::ARCH);

    writeln!(output, "System: {}", system.os_type())?;
    writeln!(output, "Machine: {architecture}")?;
    writeln!(output, "Version: {}", system.version())?;
    writeln!(output)?;
    writeln!(output, "Quire version: {}", env!("CARGO_PKG_VERSION"))
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

fn parse_http_request_timeout(value: &str) -> Result<HttpRequestTimeout, String> {
    let seconds = value
        .parse::<u64>()
        .map_err(|_| "timeout must be a positive whole number of seconds".to_string())?;
    HttpRequestTimeout::try_from(Duration::from_secs(seconds)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_defaults_to_regular_pdf() {
        let cli = Cli::try_parse_from(["quire", "input.html", "output.pdf"]).unwrap();

        assert_eq!(cli.pdf_profile, PdfProfile::Pdf);
        assert!(!cli.verbose);
        assert!(!cli.debug);
        assert!(!cli.quiet);
        assert_eq!(cli.log_level(), LevelFilter::Warn);
        assert_eq!(cli.initial_viewport_size, None);
        assert!(!cli.full_fonts);
        assert!(!cli.uncompressed_pdf);
        assert!(!cli.no_http_redirects);
        assert_eq!(cli.http_timeout, None);
        assert!(!cli.allow_fetch_errors);
        assert_eq!(cli.output.as_deref(), Some("output.pdf"));
    }

    #[test]
    fn cli_accepts_initial_viewport_size() {
        let cli = Cli::try_parse_from([
            "quire",
            "--initial-viewport-size",
            "800",
            "600",
            "input.html",
            "output.pdf",
        ])
        .unwrap();
        assert_eq!(cli.initial_viewport_size, Some(vec![800.0, 600.0]));
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
    fn cli_rejects_removed_non_weasyprint_flags() {
        for flag in [
            "--string",
            "--strings",
            "--input-syntax",
            "--page-size",
            "--page-margin",
            "--presentational-hints",
            "--no-presentational-hints",
        ] {
            assert!(Cli::try_parse_from(["quire", flag, "<p>test</p>", "output.pdf"]).is_err());
        }
    }

    #[test]
    fn cli_info_requires_no_rendering_positionals() {
        for flag in ["-i", "--info"] {
            let cli = Cli::try_parse_from(["quire", flag]).unwrap();

            assert!(cli.info);
            assert!(cli.input.is_none());
            assert!(cli.output.is_none());
        }
    }

    #[test]
    fn cli_info_conflicts_with_completion_generation() {
        assert!(Cli::try_parse_from(["quire", "--info", "--generate-completion", "bash"]).is_err());
    }

    #[test]
    fn cli_rendering_still_requires_both_positionals() {
        assert!(Cli::try_parse_from(["quire"]).is_err());
        assert!(Cli::try_parse_from(["quire", "input.html"]).is_err());
    }

    #[test]
    fn cli_accepts_pdf_profile_and_legacy_aliases() {
        let cli = Cli::try_parse_from([
            "quire",
            "--pdf-profile",
            "pdf/a-1b",
            "input.html",
            "output.pdf",
        ])
        .unwrap();

        assert_eq!(cli.pdf_profile, PdfProfile::PdfA1B);

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
    fn cli_parses_non_zero_http_request_timeout() {
        let cli = Cli::try_parse_from(["quire", "-t", "15", "input.html", "output.pdf"]).unwrap();

        assert_eq!(
            cli.http_timeout.unwrap().duration(),
            Duration::from_secs(15)
        );
        assert!(
            Cli::try_parse_from(["quire", "--timeout", "0", "input.html", "output.pdf"]).is_err()
        );
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
            assert!(error.to_string().contains("pdf/a-1b"));
        }
    }

    #[test]
    fn cli_canonicalizes_output_path() {
        let output = canonicalize_output_path("output.pdf").unwrap();

        assert!(output.is_absolute());
        assert_eq!(output.file_name(), Some("output.pdf".as_ref()));
    }

    #[test]
    fn cli_rejects_output_with_missing_parent() {
        let missing_parent = format!("quire-missing-output-parent-{}", std::process::id());
        let output = format!("{missing_parent}/output.pdf");
        let error = canonicalize_output_path(&output).unwrap_err();

        assert!(error.contains("failed to canonicalize output directory"));
    }

    #[test]
    fn info_report_uses_compiled_architecture_when_host_architecture_is_unknown() {
        let system = os_info::Info::unknown();
        let mut report = Vec::new();

        write_info_report(&mut report, &system).unwrap();

        assert_eq!(
            String::from_utf8(report).unwrap(),
            format!(
                "System: {}\nMachine: {}\nVersion: {}\n\nQuire version: {}\n",
                system.os_type(),
                std::env::consts::ARCH,
                system.version(),
                env!("CARGO_PKG_VERSION"),
            )
        );
    }
}
