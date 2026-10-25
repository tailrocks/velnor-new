use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use super::cache_admission::{CacheAdmission, is_reserved_cache_path};
use super::glob::{is_excluded, validate_pattern};

pub const BUILTIN_EXCLUSIONS: &[&str] = &[".git/**", ".velnor/cache/**"];

#[derive(Debug, Clone)]
pub struct FileIndex {
    root: PathBuf,
    files: Vec<String>,
    skipped_non_utf8: bool,
}

impl FileIndex {
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn files(&self) -> &[String] {
        &self.files
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    #[must_use]
    pub fn contains(&self, relative: &str) -> bool {
        self.files.iter().any(|file| file == relative)
    }

    #[must_use]
    pub fn skipped_non_utf8(&self) -> bool {
        self.skipped_non_utf8
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexError {
    RootUnreadable(String),

    ReadFailed(String),

    SymlinkEscape(String),

    SymlinkLoop(String),

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

pub fn build_index(root: &Path, exclusions: &[String]) -> Result<FileIndex, IndexError> {
    build_index_walk(root, exclusions)
}

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
    let target = match link.canonicalize() {
        Ok(target) => target,

        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            record_file(root, link, files, skipped_non_utf8)?;
            return Ok(());
        }
        Err(_) => return Err(IndexError::SymlinkLoop(show(link))),
    };
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

fn is_ancestor_or_self(target: &Path, link: &Path) -> bool {
    link.ancestors().skip(1).any(|dir| dir == target)
}

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

fn show(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
