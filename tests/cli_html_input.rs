//! Integration tests for command-line HTML input handling.

use std::path::PathBuf;
use std::process::Command;

struct TemporaryOutput(PathBuf);

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn cli_rejects_literal_html_as_a_nonexistent_path() {
    let output = TemporaryOutput(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("cli-literal-html-{}.pdf", std::process::id())),
    );
    let result = Command::new(env!("CARGO_BIN_EXE_quire"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("RUST_LOG")
        .arg("<p>literal HTML input</p>")
        .arg(&output.0)
        .output()
        .expect("quire binary should run");

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("No such file or directory"));
    assert!(!output.0.exists());
}
