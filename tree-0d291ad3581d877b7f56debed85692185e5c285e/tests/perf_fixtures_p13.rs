//! P13 perf fixture generator: multi-crate workspace repos.
//!
//! Included via `#[path]` from `impl_perf_p13`, so the parent wires a
//! single `mod` line for the whole perf suite.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::impl_common::{TestResult, config_with_branch, fixture_manifest_json, git};

/// Write `.velnor` inputs: config plus the consumer-manifest fixture.
fn write_velnor(root: &Path) -> TestResult {
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    Ok(())
}

/// Init a git repo with identity config (plan needs commits).
fn git_init(root: &Path) -> TestResult {
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    Ok(())
}

/// One leaf crate with a lib target.
fn write_crate(dir: &Path, name: &str) -> TestResult {
    fs::create_dir_all(dir.join("src"))?;
    fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )?;
    fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(())
}

/// Workspace repo with `members` leaf crates under `crates/`.
///
/// The root is a package plus a workspace; no lockfile, so qualification
/// is skipped and `plan` measures discovery plus graph construction.
pub(crate) fn workspace_repo(members: usize) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git_init(root)?;
    write_velnor(root)?;
    let mut manifest = String::from(
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\nmembers = [\n",
    );
    for index in 0..members {
        let name = format!("c{index:03}");
        writeln!(manifest, "  \"crates/{name}\",")?;
        write_crate(&root.join("crates").join(&name), &name)?;
    }
    manifest.push_str("]\n");
    fs::write(root.join("Cargo.toml"), manifest)?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}

/// Repo with nested and independent workspaces beside the root workspace.
///
/// Root excludes both; `nested/` is an explicit workspace root and
/// `tools/tool/` is a lone package (its own implicit workspace).
pub(crate) fn nested_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git_init(root)?;
    write_velnor(root)?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         [workspace]\nmembers = [\"crates/a\"]\nexclude = [\"nested\", \"tools/tool\"]\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
    write_crate(&root.join("crates/a"), "a")?;
    fs::create_dir_all(root.join("nested/src"))?;
    fs::write(
        root.join("nested/Cargo.toml"),
        "[package]\nname = \"nested\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n",
    )?;
    fs::write(root.join("nested/src/lib.rs"), "pub fn f() {}\n")?;
    write_crate(&root.join("tools/tool"), "tool")?;
    Ok(dir)
}

/// Repo whose root manifest is not parseable TOML.
pub(crate) fn malformed_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git_init(root)?;
    write_velnor(root)?;
    fs::write(root.join("Cargo.toml"), "[[[ not toml\n")?;
    Ok(dir)
}
