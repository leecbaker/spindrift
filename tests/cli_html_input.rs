//! Integration tests for command-line HTML input handling.

use std::path::PathBuf;
use std::process::Command;

struct TemporaryOutput(PathBuf);

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temporary_fixture(label: &str, extension: &str, contents: &str) -> TemporaryOutput {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("cli-{label}-{}.{extension}", std::process::id()));
    std::fs::write(&path, contents).expect("temporary CLI fixture should be writable");
    TemporaryOutput(path)
}

fn assert_media_box(path: &PathBuf, width: u32, height: u32) {
    let pdf = std::fs::read(path).expect("CLI should write a PDF");
    let expected = format!("/MediaBox [0 0 {width} {height}]");
    assert!(
        pdf.windows(expected.len())
            .any(|window| window == expected.as_bytes()),
        "PDF should contain {expected}"
    );
}

#[test]
fn cli_rejects_literal_html_as_a_nonexistent_path() {
    let output = TemporaryOutput(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("cli-literal-html-{}.pdf", std::process::id())),
    );
    let result = Command::new(env!("CARGO_BIN_EXE_spindrift"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("RUST_LOG")
        .arg("<p>literal HTML input</p>")
        .arg(&output.0)
        .output()
        .expect("spindrift binary should run");

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("No such file or directory"));
    assert!(!output.0.exists());
}

#[test]
fn cli_stylesheet_supplies_the_default_page_size() {
    let input = temporary_fixture("user-page-default", "html", "<p>user preference</p>");
    let stylesheet = temporary_fixture("user-page-default", "css", "@page { size: 200pt 200pt }");
    let output = temporary_fixture("user-page-default", "pdf", "");

    let result = Command::new(env!("CARGO_BIN_EXE_spindrift"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("RUST_LOG")
        .arg("--stylesheet")
        .arg(&stylesheet.0)
        .arg(&input.0)
        .arg(&output.0)
        .output()
        .expect("spindrift binary should run");

    assert!(result.status.success());
    assert_media_box(&output.0, 200, 200);
}

#[test]
fn cli_stylesheet_does_not_override_an_author_page_size() {
    let input = temporary_fixture(
        "user-page-author-wins",
        "html",
        "<style>@page { size: 300pt 300pt }</style><p>author page</p>",
    );
    let stylesheet = temporary_fixture(
        "user-page-author-wins",
        "css",
        "@page { size: 200pt 200pt }",
    );
    let output = temporary_fixture("user-page-author-wins", "pdf", "");

    let result = Command::new(env!("CARGO_BIN_EXE_spindrift"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("RUST_LOG")
        .arg("--stylesheet")
        .arg(&stylesheet.0)
        .arg(&input.0)
        .arg(&output.0)
        .output()
        .expect("spindrift binary should run");

    assert!(result.status.success());
    assert_media_box(&output.0, 300, 300);
}
