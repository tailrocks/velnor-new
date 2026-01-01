//! Shared fixtures for generation integration tests, copied from the hub.
//!
//! Subset of the hub `impl_common` used by the colocated schema-2
//! routing suites; the hub keeps its own copy for the remaining
//! suites.

use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use tempfile::TempDir;

/// Test error shortcut.
pub(crate) type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Canonical-schema fixture for positive consumer-generation tests.
///
/// Its placeholder source and digests are serialization inputs, not
/// release provenance or qualification evidence.
pub(crate) fn fixture_manifest_json() -> String {
    include_str!("../../../../fixtures/consumer-release-manifest.json").to_owned()
}

/// Build a git fixture: config plus one root crate (uncommitted).
pub(crate) fn make_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}

/// Minimal valid config with an explicit branch (no git branch lookup).
pub(crate) fn config_with_branch() -> &'static str {
    "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n"
}

/// Run git with inherited failure context.
pub(crate) fn git(args: &[&str], cwd: &Path) -> TestResult {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .status()?;
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    Ok(())
}
