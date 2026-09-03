//! Integration tests for command-line logging behavior.

use std::process::{Command, Output};

const MISSING_STYLESHEET: &str = "tests/fixtures/cli-logging-missing.css";
const INPUT: &str = "tests/fixtures/inline-style-near-zero-svg-background.html";
const OUTPUT: &str = "tests/fixtures/cli-logging-output.pdf";

fn run_spindrift(logging_flag: Option<&str>, rust_log: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_spindrift"));
    command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("RUST_LOG")
        .arg("--stylesheet")
        .arg(MISSING_STYLESHEET)
        .arg(INPUT)
        .arg(OUTPUT);
    if let Some(logging_flag) = logging_flag {
        command.arg(logging_flag);
    }
    if let Some(rust_log) = rust_log {
        command.env("RUST_LOG", rust_log);
    }
    command.output().expect("spindrift binary should run")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("spindrift should write UTF-8 diagnostics")
}

#[test]
fn default_logging_reports_a_stylesheet_error_once() {
    let output = run_spindrift(None, None);
    let diagnostic = stderr(&output);

    assert!(!output.status.success());
    assert!(diagnostic.contains(" ERROR spindrift]"));
    assert_eq!(diagnostic.lines().count(), 1);
}

#[test]
fn verbose_logging_omits_debug_messages() {
    let output = run_spindrift(Some("--verbose"), None);

    assert!(!output.status.success());
    assert!(!stderr(&output).contains("loading stylesheet"));
}

#[test]
fn debug_logging_includes_debug_messages() {
    let output = run_spindrift(Some("--debug"), None);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("loading stylesheet"));
}

#[test]
fn rust_log_controls_logging_without_a_cli_flag() {
    let output = run_spindrift(None, Some("debug"));

    assert!(!output.status.success());
    assert!(stderr(&output).contains("loading stylesheet"));
}

#[test]
fn quiet_logging_ignores_rust_log() {
    let output = run_spindrift(Some("--quiet"), Some("trace"));

    assert!(!output.status.success());
    assert!(stderr(&output).is_empty());
}
