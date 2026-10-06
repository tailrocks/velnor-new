//! Sorted repository file index with exclusion filtering.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use super::cache_admission::{CacheAdmission, is_reserved_cache_path};
use super::glob::{is_excluded, validate_pattern};

/// Built-in exclusions applied before every detector runs.
pub const BUILTIN_EXCLUSIONS: &[&str] = &[".git/**", ".velnor/cache/**"];

/// Sorted repository-relative file paths plus the canonical root.
#[derive(Debug, Clone)]
pub struct FileIndex {
    root: PathBuf,
    files: Vec<String>,
    skipped_non_utf8: bool,
}

impl FileIndex {
    /// Canonical repository root this index was built from.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Sorted repository-relative POSIX paths of indexed files.
    #[must_use]
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// Number of indexed files.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the index holds no files.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Whether `relative` names an indexed file.
    #[must_use]
    pub fn contains(&self, relative: &str) -> bool {
        self.files.iter().any(|file| file == relative)
    }

    /// Whether any entry was skipped for a non-UTF-8 name.
    ///
    /// Skipped entries never abort indexing: detectors match ASCII
    /// manifest names no undecodable entry could equal, and the
    /// orchestrator broadens explicitly on this flag.
    #[must_use]
    pub fn skipped_non_utf8(&self) -> bool {
        self.skipped_non_utf8
    }
}

/// File-index construction failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexError {
    /// Repository root is missing or unreadable.
    RootUnreadable(String),
    /// A filesystem read failed mid-walk.
    ReadFailed(String),
    /// A symlink resolves outside the repository root.
    SymlinkEscape(String),
    /// A symlink loops or cannot be resolved.
    SymlinkLoop(String),
    /// An exclusion pattern is malformed.
    MalformedPattern(String),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootUnreadable(detail) => write!(f, "unreadable_root: {detail}"),
            Self::ReadFailed(detail) => write!(f, "index_read_failed: {detail}"),
            Self::SymlinkEscape(path) => write!(f, "symlink_escape: {path}"),
            Self::SymlinkLoop(path) => write!(f, "symlink_loop: {path}"),
            Self::MalformedPattern(pattern) => write!(f, "malformed_pattern:{pattern}"),
        }
    }
}

impl std::error::Error for IndexError {}

/// Filesystem-walk alias of [`build_index_walk`] (non-git contexts only).
///
/// # Errors
///
/// Passes through [`build_index_walk`] failures.
pub fn build_index(root: &Path, exclusions: &[String]) -> Result<FileIndex, IndexError> {
    build_index_walk(root, exclusions)
}

/// Build the sorted index of `root` by walking the filesystem.
///
/// Non-git contexts only. Applies [`BUILTIN_EXCLUSIONS`] plus `exclusions`;
/// refuses symlinks that escape `root` or loop. Entries with non-UTF-8
/// names are skipped and reported via [`FileIndex::skipped_non_utf8`],
/// never an error.
///
/// # Errors
///
/// Forwards root, pattern, and symlink failures as [`IndexError`].
pub fn build_index_walk(root: &Path, exclusions: &[String]) -> Result<FileIndex, IndexError> {
    for pattern in exclusions {
        validate_pattern(pattern)?;
    }
    let canonical = root
        .canonicalize()
        .map_err(|err| IndexError::RootUnreadable(err.to_string()))?;
    let cache_admission = CacheAdmission::new(&canonical);
    let mut files = BTreeSet::new();
    let mut stack = vec![canonical.clone()];
    let mut visited = BTreeSet::from([canonical.clone()]);
    let mut skipped_non_utf8 = false;
    while let Some(dir) = stack.pop() {
        walk_dir(
            &dir,
            &canonical,
            &mut stack,
            &mut visited,
            &mut files,
            &mut skipped_non_utf8,
            &cache_admission,
        )?;
    }
    Ok(FileIndex {
        root: canonical,
        files: apply_exclusions(files, exclusions),
        skipped_non_utf8,
    })
}

/// Build the sorted index from a caller-supplied allowlist.
///
/// `files` must be repository-relative POSIX paths the caller enumerated.
/// Drops duplicates, applies exclusions, and sorts. Existing symlink path
/// components are resolved only to keep aliases into the private cache out of
/// the index; missing listed paths remain allowed.
///
/// # Errors
///
/// Forwards root, pattern, and entry failures as [`IndexError`].
pub fn build_index_from_list(
    root: &Path,
    files: &[String],
    exclusions: &[String],
) -> Result<FileIndex, IndexError> {
    for pattern in exclusions {
        validate_pattern(pattern)?;
    }
    for entry in files {
        check_entry(entry)?;
    }
    let canonical = root
        .canonicalize()
        .map_err(|err| IndexError::RootUnreadable(err.to_string()))?;
    let mut cache_admission = CacheAdmission::new(&canonical);
    let mut admitted = BTreeSet::new();
    for entry in apply_exclusions(files.iter().cloned().collect(), exclusions) {
        if !cache_admission.listed_path_is_reserved(&canonical, &entry)? {
            admitted.insert(entry);
        }
    }
    Ok(FileIndex {
        root: canonical,
        files: admitted.into_iter().collect(),
        skipped_non_utf8: false,
    })
}

/// Apply built-in plus caller exclusions, returning sorted survivors.
fn apply_exclusions(files: BTreeSet<String>, exclusions: &[String]) -> Vec<String> {
    let patterns: Vec<&str> = BUILTIN_EXCLUSIONS
        .iter()
        .copied()
        .chain(exclusions.iter().map(String::as_str))
        .collect();
    files
        .into_iter()
        .filter(|path| !is_reserved_cache_path(path) && !is_excluded(path, &patterns))
        .collect()
}

/// Reject empty, absolute, and `..`-containing entries (never silently dropped).
fn check_entry(entry: &str) -> Result<(), IndexError> {
    let bad = entry.is_empty()
        || entry.starts_with('/')
        || entry.contains('\\')
        || entry.split('/').any(|segment| segment == "..");
    if bad {
        return Err(IndexError::SymlinkEscape(entry.to_owned()));
    }
    Ok(())
}

/// Read one directory, queueing subdirectories and recording files.
fn walk_dir(
    dir: &Path,
    root: &Path,
    stack: &mut Vec<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
    files: &mut BTreeSet<String>,
    skipped_non_utf8: &mut bool,
    cache_admission: &CacheAdmission,
) -> Result<(), IndexError> {
    let entries = std::fs::read_dir(dir).map_err(|err| IndexError::ReadFailed(err.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|err| IndexError::ReadFailed(err.to_string()))?;
        let path = entry.path();
        if cache_admission.target_is_reserved(&path)
            || relative_posix(root, &path)?
                .as_deref()
                .is_some_and(is_reserved_cache_path)
        {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|err| IndexError::ReadFailed(err.to_string()))?;
        if kind.is_symlink() {
            walk_link(
                &path,
                root,
                stack,
                visited,
                files,
                skipped_non_utf8,
                cache_admission,
            )?;
        } else if kind.is_dir() {
            if visited.insert(path.clone()) {
                stack.push(path);
            }
        } else if kind.is_file() {
            record_file(root, &path, files, skipped_non_utf8)?;
        }
    }
    Ok(())
}

/// Resolve one symlink, refusing escapes and loops.
fn walk_link(
    link: &Path,
    root: &Path,
    stack: &mut Vec<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
    files: &mut BTreeSet<String>,
    skipped_non_utf8: &mut bool,
    cache_admission: &CacheAdmission,
) -> Result<(), IndexError> {
    cache_admission.ensure_cache_root_unchanged()?;
    let target = link
        .canonicalize()
        .map_err(|_| IndexError::SymlinkLoop(show(link)))?;
    if cache_admission.target_is_reserved(&target) {
        return Ok(());
    }
    if !target.starts_with(root) {
        return Err(IndexError::SymlinkEscape(show(link)));
    }
    if target.is_dir() {
        if is_ancestor_or_self(&target, link) {
            return Err(IndexError::SymlinkLoop(show(link)));
        }
        if visited.insert(target.clone()) {
            stack.push(target);
        }
    } else if target.is_file() {
        record_file(root, link, files, skipped_non_utf8)?;
    }
    Ok(())
}

/// Record one file, skipping non-UTF-8 names with an explicit flag.
fn record_file(
    root: &Path,
    path: &Path,
    files: &mut BTreeSet<String>,
    skipped_non_utf8: &mut bool,
) -> Result<(), IndexError> {
    match relative_posix(root, path)? {
        Some(relative) => {
            files.insert(relative);
        }
        None => {
            *skipped_non_utf8 = true;
        }
    }
    Ok(())
}

/// Whether `target` is the link's directory or one of its ancestors.
fn is_ancestor_or_self(target: &Path, link: &Path) -> bool {
    link.ancestors().skip(1).any(|dir| dir == target)
}

/// Render `path` as a repository-relative POSIX path.
///
/// Returns `None` for non-UTF-8 names so the caller skips explicitly
/// instead of aborting the walk.
fn relative_posix(root: &Path, path: &Path) -> Result<Option<String>, IndexError> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| IndexError::SymlinkEscape(show(path)))?;
    let mut parts = Vec::new();
    for component in rel.components() {
        let Some(text) = component.as_os_str().to_str() else {
            return Ok(None);
        };
        parts.push(text);
    }
    Ok(Some(parts.join("/")))
}

/// Lossy display form for diagnostics.
fn show(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
#[path = "index_cache_pruning_tests.rs"]
mod cache_pruning_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodable names render as relative POSIX paths.
    #[test]
    fn relative_posix_renders_decodable() {
        let root = Path::new("/repo");
        let rendered =
            relative_posix(root, &root.join("alpha/src/lib.rs")).expect("decodable renders");
        assert_eq!(rendered.as_deref(), Some("alpha/src/lib.rs"));
    }

    /// Non-UTF-8 names skip explicitly instead of erroring.
    #[test]
    #[cfg(unix)]
    fn relative_posix_skips_non_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let root = Path::new("/repo");
        let raw = b"/repo/alpha/src/\xffinvalid.rs";
        let path = Path::new(std::ffi::OsStr::from_bytes(raw));
        let rendered = relative_posix(root, path).expect("skip is not an error");
        assert_eq!(rendered, None);
    }

    /// Paths outside the root still fail closed.
    #[test]
    fn relative_posix_rejects_escape() {
        let root = Path::new("/repo");
        let err = relative_posix(root, Path::new("/other/lib.rs")).expect_err("escape fails");
        assert!(
            matches!(err, IndexError::SymlinkEscape(_)),
            "fails closed: {err}"
        );
    }
}
