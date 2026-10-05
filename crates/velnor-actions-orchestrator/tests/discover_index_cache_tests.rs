//! Planning rejects tracked generated cache content and excludes only its reserved root.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use super::build_file_index;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A staged Cargo payload in the reserved cache root fails planning.
#[test]
fn tracked_cache_payload_fails_discovery() -> TestResult {
    let root = TempDir::new()?;
    init_git(root.path())?;
    let cache = root.path().join(".velnor/cache/cargo/registry/cache");
    fs::create_dir_all(&cache)?;
    fs::write(cache.join("registry.crate"), b"payload")?;
    git(root.path(), &["add", ".velnor/cache"])?;

    let error = build_file_index(root.path(), &[]).expect_err("tracked cache is rejected");
    assert!(
        error.to_string().contains(
            "tracked_reserved_cache_path:.velnor/cache/cargo/registry/cache/registry.crate"
        ),
        "unexpected error: {error}"
    );
    Ok(())
}

/// A raw non-UTF-8 cache descendant is rejected, but a sibling is not.
#[test]
#[cfg(unix)]
fn raw_non_utf8_cache_path_keeps_its_reserved_prefix() -> TestResult {
    let root = TempDir::new()?;
    init_git(root.path())?;
    stage_raw_path(root.path(), b".velnor/cache-extra/\xffpayload.crate")?;
    let (_, skipped) = build_file_index(root.path(), &[])?;
    assert!(skipped, "the undecodable sibling is reported");

    stage_raw_path(root.path(), b".velnor/cache/\xffpayload.crate")?;
    let error = build_file_index(root.path(), &[]).expect_err("tracked cache is rejected");
    assert!(
        error
            .to_string()
            .contains("tracked_reserved_cache_path:.velnor/cache/"),
        "unexpected error: {error}"
    );
    Ok(())
}

/// A tracked submodule at the exact cache root fails planning.
#[test]
fn tracked_cache_submodule_fails_discovery() -> TestResult {
    let root = TempDir::new()?;
    init_git(root.path())?;
    let cache = root.path().join(".velnor/cache");
    fs::create_dir_all(&cache)?;
    init_git(&cache)?;
    git(&cache, &["commit", "--allow-empty", "-m", "cache"])?;
    git(root.path(), &["add", "-f", ".velnor/cache"])?;

    let output = git_output(root.path(), &["ls-files", "-s"])?;
    assert!(
        output.contains("160000 ") && output.contains("\t.velnor/cache"),
        "fixture must stage a gitlink: {output}"
    );
    let error = build_file_index(root.path(), &[]).expect_err("tracked cache gitlink is rejected");
    assert!(
        error
            .to_string()
            .contains("tracked_reserved_cache_path:.velnor/cache"),
        "unexpected error: {error}"
    );
    Ok(())
}

/// Git-backed indexing drops aliases into the cache while preserving siblings.
#[test]
#[cfg(unix)]
fn git_backed_cache_alias_is_pruned_without_hiding_similarly_named_source() -> TestResult {
    let root = TempDir::new()?;
    init_git(root.path())?;
    write_file(root.path(), ".velnor/cache/payload.crate")?;
    write_file(root.path(), ".velnor/cache-extra/source.toml")?;
    fs::create_dir_all(root.path().join("src"))?;
    std::os::unix::fs::symlink(
        "../.velnor/cache/payload.crate",
        root.path().join("src/cache-alias.crate"),
    )?;
    std::os::unix::fs::symlink(
        "../.velnor/cache-extra/source.toml",
        root.path().join("src/source-alias.toml"),
    )?;
    git(root.path(), &["add", "src"])?;

    let (index, _) = build_file_index(root.path(), &[])?;
    assert!(!index.contains("src/cache-alias.crate"));
    assert!(index.contains("src/source-alias.toml"));
    Ok(())
}

/// A stale Git file entry cannot resolve through a replaced directory into cache.
#[test]
#[cfg(unix)]
fn git_backed_stale_path_through_cache_symlink_is_pruned() -> TestResult {
    let root = TempDir::new()?;
    init_git(root.path())?;
    write_file(root.path(), "src/generated/manifest.toml")?;
    git(root.path(), &["add", "src/generated/manifest.toml"])?;
    fs::remove_dir_all(root.path().join("src/generated"))?;
    write_file(root.path(), ".velnor/cache/manifest.toml")?;
    std::os::unix::fs::symlink("../.velnor/cache", root.path().join("src/generated"))?;

    let (index, _) = build_file_index(root.path(), &[])?;
    assert!(!index.contains("src/generated/manifest.toml"));
    Ok(())
}

/// Untracked cache payload is excluded, while similarly named source remains.
#[test]
fn cache_siblings_and_outside_source_remain_indexed() -> TestResult {
    let root = TempDir::new()?;
    init_git(root.path())?;
    write_file(
        root.path(),
        ".velnor/cache/cargo/registry/cache/untracked.crate",
    )?;
    write_file(root.path(), ".velnor/cache-extra/metadata.toml")?;
    write_file(root.path(), ".velnor/cache2/metadata.toml")?;
    write_file(root.path(), "src/generated/cache/metadata.toml")?;
    git(
        root.path(),
        &[
            "add",
            ".velnor/cache-extra",
            ".velnor/cache2",
            "src/generated/cache",
        ],
    )?;

    let (index, _) = build_file_index(root.path(), &[])?;
    assert!(!index.contains(".velnor/cache/cargo/registry/cache/untracked.crate"));
    assert!(index.contains(".velnor/cache-extra/metadata.toml"));
    assert!(index.contains(".velnor/cache2/metadata.toml"));
    assert!(index.contains("src/generated/cache/metadata.toml"));
    Ok(())
}

/// Non-Git discovery excludes only the reserved cache subtree.
#[test]
fn non_git_discovery_excludes_only_reserved_cache_root() -> TestResult {
    let root = TempDir::new()?;
    write_file(
        root.path(),
        ".velnor/cache/cargo/registry/cache/untracked.crate",
    )?;
    write_file(root.path(), ".velnor/cache-extra/metadata.toml")?;
    write_file(root.path(), "src/generated/cache/metadata.toml")?;

    let (index, _) = build_file_index(root.path(), &[])?;
    assert!(!index.contains(".velnor/cache/cargo/registry/cache/untracked.crate"));
    assert!(index.contains(".velnor/cache-extra/metadata.toml"));
    assert!(index.contains("src/generated/cache/metadata.toml"));
    Ok(())
}

/// Create a file and parent directories under `root`.
fn write_file(root: &Path, relative: &str) -> TestResult {
    let file = root.join(relative);
    fs::create_dir_all(file.parent().ok_or("file has no parent")?)?;
    fs::write(file, b"fixture")?;
    Ok(())
}

/// Initialize a local repository with stable author details.
fn init_git(root: &Path) -> TestResult {
    git(root, &["init", "-q"])?;
    git(root, &["config", "user.name", "Velnor Test"])?;
    git(
        root,
        &["config", "user.email", "velnor-test@example.invalid"],
    )?;
    Ok(())
}

/// Add an index entry with raw path bytes without creating a filesystem name.
#[cfg(unix)]
fn stage_raw_path(root: &Path, path: &[u8]) -> TestResult {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let blob = git_output(root, &["hash-object", "-w", "-t", "blob", "--stdin"])?;
    let mut cache_info = format!("100644,{},", blob.trim()).into_bytes();
    cache_info.extend_from_slice(path);
    let output = Command::new("git")
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(OsString::from_vec(cache_info))
        .current_dir(root)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git update-index failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

/// Run Git and require success.
fn git(root: &Path, args: &[&str]) -> TestResult {
    git_output(root, args)?;
    Ok(())
}

/// Run Git and return stdout, rejecting unsuccessful status.
fn git_output(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git").args(args).current_dir(root).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
