//! `added_files` byte-exactness cases: quotepath and whitespace names.
//!
//! Declared via `#[path]` from `select_edges.rs` under `cfg(test)`.
//! Exact bytes here are what let `base_edges` skip added manifests and
//! narrow instead of broadening.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Run one git command in `root`, failing on nonzero exit.
fn git(args: &[&str], root: &Path) -> TestResult {
    let status = Command::new("git").args(args).current_dir(root).status()?;
    assert!(status.success(), "git {args:?} failed");
    Ok(())
}

/// One line of git output without the trailing newline.
fn git_line(args: &[&str], root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git").args(args).current_dir(root).output()?;
    assert!(output.status.success(), "git {args:?} failed");
    let text = String::from_utf8(output.stdout)?;
    Ok(text.trim().to_owned())
}

/// Empty repo with `core.quotepath` forced on (the git default).
fn make_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    git(&["config", "core.quotepath", "true"], root)?;
    Ok(dir)
}

/// Commit everything staged and return the new HEAD.
fn commit(root: &Path, message: &str) -> Result<String, Box<dyn std::error::Error>> {
    git(&["add", "."], root)?;
    git(&["commit", "-m", message], root)?;
    git_line(&["rev-parse", "HEAD"], root)
}

/// Read the added set, failing the test with the tag on error.
fn added(root: &Path, base: &str, head: &str) -> BTreeSet<String> {
    let result = added_files(root, base, head);
    assert!(result.is_ok(), "added_files failed: {result:?}");
    result.unwrap_or_default()
}

/// A quotepath-triggering added manifest reports its exact raw bytes.
///
/// Display parsing would return the C-quoted `"gamm\303\241/..."` form,
/// which never equals the wanted manifest and would broaden.
#[test]
fn added_quotepath_manifest_returns_exact_bytes() -> TestResult {
    let repo = make_repo()?;
    let root = repo.path();
    fs::write(root.join("README.md"), "base\n")?;
    let base = commit(root, "base")?;
    fs::create_dir_all(root.join("gammá/src"))?;
    fs::write(root.join("gammá/Cargo.toml"), "[package]\n")?;
    fs::write(root.join("gammá/src/lib.rs"), "pub fn f() {}\n")?;
    let head = commit(root, "head")?;
    let added = added(root, &base, &head);
    assert!(
        added.contains("gammá/Cargo.toml"),
        "exact quotepath: {added:?}"
    );
    assert!(
        added
            .iter()
            .all(|path| !path.contains('\\') && !path.starts_with('"')),
        "no C-quoting: {added:?}"
    );
    Ok(())
}

/// A trailing-space added file keeps its trailing space exactly.
///
/// Display parsing would trim it, so the path would never match.
#[test]
fn added_trailing_space_file_returns_exact_bytes() -> TestResult {
    let repo = make_repo()?;
    let root = repo.path();
    fs::write(root.join("README.md"), "base\n")?;
    let base = commit(root, "base")?;
    fs::create_dir_all(root.join("alpha/src"))?;
    fs::write(root.join("alpha/src/trailing.rs "), "pub fn f() {}\n")?;
    let head = commit(root, "head")?;
    let added = added(root, &base, &head);
    assert!(
        added.contains("alpha/src/trailing.rs "),
        "trailing space kept: {added:?}"
    );
    Ok(())
}
