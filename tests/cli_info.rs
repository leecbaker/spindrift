//! Integration tests for command-line information reporting.

use std::fs;
use std::process::Command;

struct TemporaryDirectory(std::path::PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "quire-cli-info-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after the Unix epoch")
                .as_nanos()
        ));
        fs::create_dir(&path).expect("temporary test directory should be created");
        Self(path)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn info_reports_the_host_without_rendering_or_logging() {
    let directory = TemporaryDirectory::new();
    let result = Command::new(env!("CARGO_BIN_EXE_quire"))
        .current_dir(&directory.0)
        .env_remove("RUST_LOG")
        .args(["--info", "input.html", "missing/output.pdf"])
        .output()
        .expect("quire binary should run");

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    assert!(
        fs::read_dir(&directory.0)
            .expect("test directory should remain readable")
            .next()
            .is_none()
    );

    let report = String::from_utf8(result.stdout).expect("info report should be UTF-8");
    assert!(report.starts_with("System: "));
    assert!(report.contains("\nMachine: "));
    assert!(report.contains("\nVersion: "));
    assert!(report.contains("\n\nQuire version: "));
    assert!(!report.contains("Pango version:"));
    assert!(!report.contains("Pydyf version:"));
}
