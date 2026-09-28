//! Sorted repository file index with exclusion filtering.
//!
//! The orchestrator builds one index per run and hands surviving paths to
//! detectors; exclusions therefore apply before any detection runs.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

/// Built-in exclusions applied before every detector runs.
pub const BUILTIN_EXCLUSIONS: &[&str] = &[".git/**"];

/// Sorted repository-relative file paths plus the canonical root.
#[derive(Debug, Clone)]
pub struct FileIndex {
    root: PathBuf,
    files: Vec<String>,
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

/// Build the sorted index of `root`, dropping excluded paths.
///
/// Applies [`BUILTIN_EXCLUSIONS`] plus `exclusions` before returning, so
/// detectors only observe surviving paths. Refuses symlinks that escape
/// `root` or loop.
///
/// # Errors
///
/// Returns [`IndexError`] when the root is unreadable, a pattern is
/// malformed, or a symlink escapes or loops.
pub fn build_index(root: &Path, exclusions: &[String]) -> Result<FileIndex, IndexError> {
    for pattern in exclusions {
        validate_pattern(pattern)?;
    }
    let canonical = root
        .canonicalize()
        .map_err(|err| IndexError::RootUnreadable(err.to_string()))?;
    let mut files = BTreeSet::new();
    let mut stack = vec![canonical.clone()];
    let mut visited = BTreeSet::from([canonical.clone()]);
    while let Some(dir) = stack.pop() {
        walk_dir(&dir, &canonical, &mut stack, &mut visited, &mut files)?;
    }
    let mut patterns: Vec<&str> = Vec::with_capacity(BUILTIN_EXCLUSIONS.len() + exclusions.len());
    patterns.extend(BUILTIN_EXCLUSIONS.iter().copied());
    patterns.extend(exclusions.iter().map(String::as_str));
    let kept = files
        .into_iter()
        .filter(|path| !is_excluded(path, &patterns))
        .collect();
    Ok(FileIndex {
        root: canonical,
        files: kept,
    })
}

/// Read one directory, queueing subdirectories and recording files.
fn walk_dir(
    dir: &Path,
    root: &Path,
    stack: &mut Vec<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
    files: &mut BTreeSet<String>,
) -> Result<(), IndexError> {
    let entries = std::fs::read_dir(dir).map_err(|err| IndexError::ReadFailed(err.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|err| IndexError::ReadFailed(err.to_string()))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|err| IndexError::ReadFailed(err.to_string()))?;
        if kind.is_symlink() {
            walk_link(&path, root, stack, visited, files)?;
        } else if kind.is_dir() {
            if visited.insert(path.clone()) {
                stack.push(path);
            }
        } else if kind.is_file() {
            files.insert(relative_posix(root, &path)?);
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
) -> Result<(), IndexError> {
    let target = link
        .canonicalize()
        .map_err(|_| IndexError::SymlinkLoop(show(link)))?;
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
        files.insert(relative_posix(root, link)?);
    }
    Ok(())
}

/// Whether `target` is the link's directory or one of its ancestors.
fn is_ancestor_or_self(target: &Path, link: &Path) -> bool {
    let mut current = link.parent();
    while let Some(dir) = current {
        if dir == target {
            return true;
        }
        current = dir.parent();
    }
    false
}

/// Render `path` as a repository-relative POSIX path.
fn relative_posix(root: &Path, path: &Path) -> Result<String, IndexError> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| IndexError::SymlinkEscape(show(path)))?;
    let mut parts = Vec::new();
    for component in rel.components() {
        let text = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| IndexError::ReadFailed(format!("non_utf8_name: {}", show(path))))?;
        parts.push(text);
    }
    Ok(parts.join("/"))
}

/// Lossy display form for diagnostics.
fn show(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Validate one exclusion glob (relative, no traversal, well-formed).
///
/// # Errors
///
/// Returns [`IndexError::MalformedPattern`] for empty, absolute,
/// parent-traversal, or malformed patterns.
pub fn validate_pattern(pattern: &str) -> Result<(), IndexError> {
    let malformed = pattern.is_empty()
        || pattern.starts_with('/')
        || pattern.contains('\\')
        || pattern.split('/').any(|segment| segment == "..")
        || !pattern.bytes().all(is_glob_byte);
    if malformed {
        return Err(IndexError::MalformedPattern(pattern.to_owned()));
    }
    Ok(())
}

/// Bytes allowed in exclusion globs.
fn is_glob_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'/' | b'.'
                | b'-'
                | b'_'
                | b'*'
                | b'?'
                | b'['
                | b']'
                | b'{'
                | b'}'
                | b'!'
                | b'+'
                | b'@'
        )
}

/// Whether `path` or an ancestor directory matches any pattern.
#[must_use]
pub fn is_excluded(path: &str, patterns: &[&str]) -> bool {
    patterns
        .iter()
        .any(|pattern| matches_path_or_ancestor(pattern, path))
}

/// Match `path` plus each ancestor prefix against one pattern.
fn matches_path_or_ancestor(pattern: &str, path: &str) -> bool {
    if matches_glob(pattern, path) {
        return true;
    }
    let mut prefix = path;
    while let Some((parent, _)) = prefix.rsplit_once('/') {
        if matches_glob(pattern, parent) {
            return true;
        }
        prefix = parent;
    }
    false
}

/// Match a repository-relative path against one glob.
///
/// Supports `**` across segments, `*` and `?` within a segment, `[...]`
/// classes, and non-nested `{a,b}` alternation.
#[must_use]
pub fn matches_glob(pattern: &str, path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    expand_braces(pattern)
        .iter()
        .any(|expanded| match_segments(&expanded.split('/').collect::<Vec<_>>(), &segments))
}

/// Expand the first top-level `{a,b}` group; without braces returns `pattern`.
fn expand_braces(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_owned()];
    };
    let Some(close) = pattern[open..].find('}') else {
        return vec![pattern.to_owned()];
    };
    let close = open + close;
    let inner = &pattern[open + 1..close];
    if !inner.contains(',') {
        return vec![pattern.to_owned()];
    }
    let (head, _) = pattern.split_at(open);
    let tail = &pattern[close + 1..];
    let mut out = Vec::new();
    for option in inner.split(',') {
        out.extend(expand_braces(&format!("{head}{option}{tail}")));
    }
    out
}

/// Match pattern segments against path segments; `**` spans segments.
fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((head, tail)) if *head == "**" => {
            (0..=path.len()).any(|skip| match_segments(tail, &path[skip..]))
        }
        Some((head, tail)) => path
            .split_first()
            .is_some_and(|(name, rest)| match_segment(head, name) && match_segments(tail, rest)),
    }
}

/// Match one segment with `*`, `?`, and `[...]` classes.
fn match_segment(pattern: &str, name: &str) -> bool {
    match_chars(
        &pattern.chars().collect::<Vec<_>>(),
        &name.chars().collect::<Vec<_>>(),
    )
}

/// Match character patterns against character text.
fn match_chars(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skip| match_chars(rest, &text[skip..])),
        Some(('?', rest)) => text
            .split_first()
            .is_some_and(|(_, tail)| match_chars(rest, tail)),
        Some(('[', _)) => match_class(pattern, text),
        Some((literal, rest)) => text
            .split_first()
            .is_some_and(|(head, tail)| head == literal && match_chars(rest, tail)),
    }
}

/// Match a leading `[...]` class against the first text character.
fn match_class(pattern: &[char], text: &[char]) -> bool {
    let Some(head) = text.first() else {
        return false;
    };
    let Some(end) = pattern.iter().position(|char| *char == ']') else {
        return false;
    };
    if end < 2 {
        return false;
    }
    let (mut items, rest) = (&pattern[1..end], &pattern[end + 1..]);
    let mut negated = false;
    if items.first() == Some(&'!') {
        negated = true;
        items = &items[1..];
    }
    (class_hit(items, *head) != negated) && match_chars(rest, &text[1..])
}

/// Whether `target` is listed by class `items`, honoring `a-z` ranges.
fn class_hit(items: &[char], target: char) -> bool {
    let mut index = 0;
    while index < items.len() {
        if index + 2 < items.len() && items[index + 1] == '-' {
            if items[index] <= target && target <= items[index + 2] {
                return true;
            }
            index += 3;
        } else {
            if items[index] == target {
                return true;
            }
            index += 1;
        }
    }
    false
}
