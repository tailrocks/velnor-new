//! The generated cache subtree is excluded before filesystem traversal.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::cache_admission::CacheAdmission;
use super::IndexError;
use super::{build_index_from_list, build_index_walk};

type TestResult = Result<(), Box<dyn std::error::Error>>;

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

/// Minimal isolated directory fixture without a crate-level temp dependency.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Create a unique directory under the operating system temp root.
    fn new() -> io::Result<Self> {
        for _attempt in 0..64 {
            let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("velnor-contract-index-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "temporary directory name collisions exhausted",
        ))
    }

    /// Return the fixture root.
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            eprintln!(
                "temporary test directory cleanup failed at {}: {error}",
                self.path.display()
            );
        }
    }
}

/// A cache symlink is skipped without resolving its external target.
#[test]
#[cfg(unix)]
fn cache_symlink_is_not_resolved() -> TestResult {
    let root = TempDir::new()?;
    let external = TempDir::new()?;
    fs::write(external.path().join("payload.crate"), b"private cache")?;
    let cache_parent = root.path().join(".velnor");
    fs::create_dir_all(&cache_parent)?;
    std::os::unix::fs::symlink(external.path(), cache_parent.join("cache"))?;
    write_file(root.path(), ".velnor/cache-extra/source.toml")?;
    write_file(root.path(), "src/generated/cache/source.toml")?;

    let index = build_index_walk(root.path(), &[])?;
    assert!(
        !index
            .files()
            .iter()
            .any(|path| path.starts_with(".velnor/cache/"))
    );
    assert!(index.contains(".velnor/cache-extra/source.toml"));
    assert!(index.contains("src/generated/cache/source.toml"));
    Ok(())
}

/// A dangling cache symlink is ignored instead of treated as an index loop.
#[test]
#[cfg(unix)]
fn dangling_cache_symlink_is_not_resolved() -> TestResult {
    let root = TempDir::new()?;
    let cache_parent = root.path().join(".velnor");
    fs::create_dir_all(&cache_parent)?;
    std::os::unix::fs::symlink("missing-cache-target", cache_parent.join("cache"))?;

    build_index_walk(root.path(), &[])?;
    Ok(())
}

/// A self-referential cache symlink is ignored before loop resolution.
#[test]
#[cfg(unix)]
fn looping_cache_symlink_is_not_resolved() -> TestResult {
    let root = TempDir::new()?;
    let cache_parent = root.path().join(".velnor");
    fs::create_dir_all(&cache_parent)?;
    std::os::unix::fs::symlink("cache", cache_parent.join("cache"))?;

    build_index_walk(root.path(), &[])?;
    Ok(())
}

/// A symlink alias cannot expose files stored inside the private cache.
#[test]
#[cfg(unix)]
fn symlink_target_inside_cache_is_not_indexed() -> TestResult {
    let root = TempDir::new()?;
    let cache_file = root
        .path()
        .join(".velnor/cache/cargo/registry/cache/item.crate");
    fs::create_dir_all(cache_file.parent().ok_or("cache file has no parent")?)?;
    fs::write(&cache_file, b"private cache")?;
    let alias = root.path().join("src/cache-alias.crate");
    fs::create_dir_all(alias.parent().ok_or("alias has no parent")?)?;
    std::os::unix::fs::symlink(&cache_file, &alias)?;

    let index = build_index_walk(root.path(), &[])?;
    assert!(!index.contains("src/cache-alias.crate"));
    assert!(!index.contains(".velnor/cache/cargo/registry/cache/item.crate"));
    Ok(())
}

/// Both index construction routes prune targets behind the cache-root symlink.
#[test]
#[cfg(unix)]
fn cache_target_admission_matches_for_list_and_walk() -> TestResult {
    let root = TempDir::new()?;
    let external = TempDir::new()?;
    write_file(external.path(), "payload.crate")?;
    let cache_parent = root.path().join(".velnor");
    fs::create_dir_all(&cache_parent)?;
    std::os::unix::fs::symlink(external.path(), cache_parent.join("cache"))?;
    let alias_parent = root.path().join("src");
    fs::create_dir_all(&alias_parent)?;
    std::os::unix::fs::symlink(
        "../.velnor/cache/payload.crate",
        alias_parent.join("cache-alias.crate"),
    )?;

    let walked = build_index_walk(root.path(), &[])?;
    let listed = build_index_from_list(root.path(), &["src/cache-alias.crate".to_owned()], &[])?;
    assert!(!walked.contains("src/cache-alias.crate"));
    assert!(!listed.contains("src/cache-alias.crate"));
    Ok(())
}

#[test]
fn listed_dot_path_cannot_bypass_reserved_cache_filter() -> TestResult {
    let root = TempDir::new()?;
    let repo = root.path();
    std::fs::create_dir_all(repo.join(".velnor/cache"))?;
    std::fs::write(repo.join(".velnor/cache/payload.crate"), b"cache")?;

    let index = build_index_from_list(repo, &["./.velnor/cache/payload.crate".to_owned()], &[])?;

    assert_eq!(index.files(), Vec::<String>::new());
    Ok(())
}

/// A large reserved tree is absent while exact cache siblings remain visible.
#[test]
fn large_cache_tree_is_excluded_without_hiding_siblings() -> TestResult {
    let root = TempDir::new()?;
    for index in 0..256 {
        write_file(
            root.path(),
            &format!(".velnor/cache/cargo/registry/src/package-{index}/Cargo.toml"),
        )?;
    }
    write_file(root.path(), ".velnor/cache2/source.toml")?;

    let index = build_index_walk(root.path(), &[])?;
    assert!(
        !index
            .files()
            .iter()
            .any(|path| path.starts_with(".velnor/cache/"))
    );
    assert!(index.contains(".velnor/cache2/source.toml"));
    Ok(())
}

/// A changed directory ancestor cannot reuse a stale cached resolution.
#[test]
#[cfg(unix)]
fn listed_path_rechecks_replaced_directory_ancestry() -> TestResult {
    let root = TempDir::new()?;
    let repo = root.path();
    write_file(repo, ".velnor/cache/z/Cargo.toml")?;
    let alias = repo.join("alias");
    fs::create_dir(&alias)?;
    fs::write(alias.join("a"), b"ordinary file")?;
    write_file(repo, ".velnor/cache-extra/payload.crate")?;

    let mut admission = CacheAdmission::new(repo);
    assert!(!admission.listed_path_is_reserved(repo, "alias/a")?);

    fs::remove_dir_all(&alias)?;
    std::os::unix::fs::symlink(".velnor/cache", &alias)?;

    assert!(admission.listed_path_is_reserved(repo, "alias/z/Cargo.toml")?);
    assert!(!admission.listed_path_is_reserved(repo, ".velnor/cache-extra/payload.crate")?);
    Ok(())
}

/// Replacing the cache-root symlink fails closed on the next listed path.
#[test]
#[cfg(unix)]
fn listed_path_rejects_replaced_cache_root_symlink() -> TestResult {
    let root = TempDir::new()?;
    let old_cache = TempDir::new()?;
    let new_cache = TempDir::new()?;
    write_file(old_cache.path(), "old.crate")?;
    write_file(new_cache.path(), "payload.crate")?;
    let cache_parent = root.path().join(".velnor");
    fs::create_dir_all(&cache_parent)?;
    let cache = cache_parent.join("cache");
    std::os::unix::fs::symlink(old_cache.path(), &cache)?;
    write_file(root.path(), "src/ordinary.txt")?;
    let alias = root.path().join("src/cache-alias.crate");
    std::os::unix::fs::symlink(new_cache.path().join("payload.crate"), &alias)?;

    let mut admission = CacheAdmission::new(root.path());
    assert!(!admission.listed_path_is_reserved(root.path(), "src/ordinary.txt")?);

    fs::remove_file(&cache)?;
    std::os::unix::fs::symlink(new_cache.path(), &cache)?;

    let error = admission
        .listed_path_is_reserved(root.path(), "src/cache-alias.crate")
        .expect_err("a replaced cache-root symlink must fail closed");
    assert!(matches!(error, IndexError::ReadFailed(_)));
    assert!(error.to_string().contains("reserved cache root changed"));
    Ok(())
}

/// Retargeting an intermediate symlink fails closed despite a stable cache link.
#[test]
#[cfg(unix)]
fn listed_path_rejects_retargeted_cache_root_chain() -> TestResult {
    let root = TempDir::new()?;
    let repo = root.path();
    write_file(repo, "old-cache/payload.crate")?;
    write_file(repo, "new-cache/payload.crate")?;
    let cache_link = repo.join("cache-link");
    std::os::unix::fs::symlink("old-cache", &cache_link)?;
    let cache_parent = repo.join(".velnor");
    fs::create_dir_all(&cache_parent)?;
    std::os::unix::fs::symlink("../cache-link", cache_parent.join("cache"))?;
    write_file(repo, "src/ordinary.txt")?;
    let alias = repo.join("src/cache-alias.crate");
    std::os::unix::fs::symlink("../new-cache/payload.crate", &alias)?;

    let mut admission = CacheAdmission::new(repo);
    assert!(!admission.listed_path_is_reserved(repo, "src/ordinary.txt")?);

    fs::remove_file(&cache_link)?;
    std::os::unix::fs::symlink("new-cache", &cache_link)?;

    let error = admission
        .listed_path_is_reserved(repo, "src/cache-alias.crate")
        .expect_err("a retargeted cache chain must not use stale resolution");
    assert!(matches!(error, IndexError::ReadFailed(_)));
    assert!(error.to_string().contains("reserved cache root changed"));
    Ok(())
}

/// Create a file and parent directories under `root`.
fn write_file(root: &Path, relative: &str) -> TestResult {
    let file = root.join(relative);
    fs::create_dir_all(file.parent().ok_or("file has no parent")?)?;
    fs::write(file, b"fixture")?;
    Ok(())
}
