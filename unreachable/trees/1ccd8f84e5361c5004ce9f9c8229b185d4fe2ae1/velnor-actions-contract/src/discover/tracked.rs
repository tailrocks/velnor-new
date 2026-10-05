//! Tracked/untracked file-index modes (par §1, §4).
//!
//! Local runs index tracked plus staged plus untracked files; CI runs
//! index tracked files only. The mode selects which caller-enumerated
//! lists enter the index; enumeration itself stays the caller's job.

use std::path::Path;

use super::index::{FileIndex, IndexError, build_index_from_list};

/// Which caller-enumerated file lists enter the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    /// Tracked files only (CI selection).
    Tracked,
    /// Untracked files only (local additive pass).
    Untracked,
    /// Tracked plus untracked files (local selection).
    Both,
}

/// Build the sorted index from tracked/untracked lists under `mode`.
///
/// Pure apart from canonicalizing `root`: duplicates drop, exclusions
/// apply, and the survivors sort. Existence is not checked.
///
/// # Errors
///
/// Forwards root, pattern, and entry failures as [`IndexError`].
pub fn build_index_from_tracked(
    root: &Path,
    tracked: &[String],
    untracked: &[String],
    mode: IndexMode,
    exclusions: &[String],
) -> Result<FileIndex, IndexError> {
    let mut files = Vec::new();
    if mode != IndexMode::Untracked {
        files.extend(tracked.iter().cloned());
    }
    if mode != IndexMode::Tracked {
        files.extend(untracked.iter().cloned());
    }
    build_index_from_list(root, &files, exclusions)
}
