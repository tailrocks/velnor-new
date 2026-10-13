//! Generation filesystem guards: tool snapshots, destination validation,
//! commit ownership, and staging checks.
//!
//! Self-declared from `generate.rs` (`#[path]`, no `lib.rs` edit).
//! Preview destinations validate containment, ancestry, and symlinks
//! before any directory is created; in-place commits run under an
//! exclusive ownership lock; the tool snapshot proves read-only files
//! were never modified.

use std::path::{Component, Path, PathBuf};

use crate::OrchestratorError;

/// Read-only tool files generation must never modify (TOOL-2.10).
const TOOL_FILES: [&str; 6] = [
    "mise.toml",
    ".mise.toml",
    "mise.lock",
    ".mise.lock",
    ".mise-version",
    "rust-toolchain.toml",
];

/// Byte snapshot of the read-only tool files for drift detection.
///
/// Captured when generation starts and verified before any output
/// replacement, so a mid-generation tool-file change fails closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSnapshot {
    /// One entry per [`TOOL_FILES`] path: bytes, or `None` when absent.
    entries: Vec<(String, Option<Vec<u8>>)>,
    /// Tool files present but unreadable: never merged with missing.
    unreadable: Vec<String>,
}

impl ToolSnapshot {
    /// Capture the current tool-file bytes under `root`.
    #[must_use]
    pub fn capture(root: &Path) -> Self {
        let mut unreadable = Vec::new();
        let entries = TOOL_FILES
            .iter()
            .map(|rel| {
                let bytes = match std::fs::read(root.join(rel)) {
                    Ok(bytes) => Some(bytes),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                    Err(_) => {
                        unreadable.push((*rel).to_owned());
                        None
                    }
                };
                ((*rel).to_owned(), bytes)
            })
            .collect();
        Self {
            entries,
            unreadable,
        }
    }

    /// Fail when any tool file differs from the captured bytes.
    ///
    /// Unreadable files fail closed: the no-write proof needs contents.
    ///
    /// # Errors
    ///
    /// Returns a contract error naming the first drifted file.
    pub fn verify(&self, root: &Path) -> Result<(), OrchestratorError> {
        let fresh = Self::capture(root);
        let mut bad = self.unreadable.clone();
        bad.extend(fresh.unreadable.iter().cloned());
        if let Some(first) = bad.iter().min() {
            return Err(OrchestratorError::Contract {
                problem: format!("tool_files_unreadable:{first}"),
            });
        }
        for ((rel, want), (_, got)) in self.entries.iter().zip(fresh.entries.iter()) {
            if want != got {
                return Err(OrchestratorError::Contract {
                    problem: format!("tool_files_changed:{rel}"),
                });
            }
        }
        Ok(())
    }
}

/// Fixed-name lock directory claiming in-place generation ownership.
const GENERATE_LOCK_NAME: &str = ".github.velnor-generate.lock";

/// Exclusive in-place generation ownership, released on drop.
///
/// Created with `create_dir`, so a second simultaneous `generate` fails
/// closed instead of interleaving writes; the lock is removed on every
/// exit path via [`Drop`]. A stale lock (owner crash) refuses generation
/// until an operator removes it.
#[derive(Debug)]
pub(crate) struct GenerateOwnership {
    /// Lock directory removed on drop.
    path: PathBuf,
}

impl GenerateOwnership {
    /// Claim ownership of in-place generation under `root`.
    pub(crate) fn acquire(root: &Path) -> Result<Self, OrchestratorError> {
        let path = root.join(GENERATE_LOCK_NAME);
        match std::fs::create_dir(&path) {
            Ok(()) => Ok(Self { path }),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(OrchestratorError::Contract {
                    problem: format!("concurrent_generate:{}", path.display()),
                })
            }
            Err(err) => Err(OrchestratorError::io(
                path.display().to_string(),
                err.to_string(),
            )),
        }
    }
}

impl Drop for GenerateOwnership {
    /// Best-effort lock release; a stale lock refuses until removed.
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir(&self.path);
    }
}

/// Whether two paths share a filesystem (unix device ids).
///
/// Staging under the destination root constructs same-filesystem
/// staging on every platform; unix additionally verifies device ids.
#[cfg(unix)]
pub(crate) fn same_filesystem(first: &Path, second: &Path) -> Result<bool, OrchestratorError> {
    use std::os::unix::fs::MetadataExt;
    let dev = |path: &Path| {
        std::fs::metadata(path)
            .map(|meta| meta.dev())
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))
    };
    Ok(dev(first)? == dev(second)?)
}

/// Non-unix staging shares the destination filesystem by construction.
#[cfg(not(unix))]
pub(crate) fn same_filesystem(_first: &Path, _second: &Path) -> Result<bool, OrchestratorError> {
    Ok(true)
}

/// Build a preview refusal for one destination.
fn preview_refused(path: &str, reason: &str) -> OrchestratorError {
    OrchestratorError::PreviewRefused {
        path: path.to_owned(),
        reason: reason.to_owned(),
    }
}

/// Create or validate the preview root, refusing unsafe destinations.
///
/// Validates containment, ancestry, and symlinks before creating any
/// directory, then creates the missing chain level by level, re-checks
/// the canonical path, and exclusively reserves `<dest>/.github`: a
/// refused preview leaves nothing behind. Residual: an out-of-band
/// actor swapping path components mid-write can only be narrowed, not
/// closed, with standard file APIs; same-repo races fail closed.
pub(crate) fn prepare_preview_dir(root: &Path, dest: &Path) -> Result<PathBuf, OrchestratorError> {
    let label = dest.display().to_string();
    let root_canon = root
        .canonicalize()
        .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
    if let Ok(meta) = std::fs::symlink_metadata(dest) {
        if meta.is_symlink() {
            return Err(preview_refused(&label, "symlink_refused"));
        }
        if !meta.is_dir() {
            return Err(preview_refused(&label, "not_a_directory"));
        }
        let mut entries = std::fs::read_dir(dest)
            .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
        if entries.next().is_some() {
            return Err(preview_refused(&label, "non_empty"));
        }
    }
    let (anchor, suffix) = preview_anchor(dest, &label)?;
    refuse_inside_or_ancestor(&root_canon, &anchor.join(&suffix), &label)?;
    create_preview_chain(&anchor, &suffix, &label)?;
    let canonical = dest
        .canonicalize()
        .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
    refuse_inside_or_ancestor(&root_canon, &canonical, &label)?;
    reserve_preview_github(&canonical, &label)?;
    Ok(canonical)
}

/// Nearest existing ancestor (canonicalized) plus the clean suffix.
///
/// Every existing component must be symlink-free; `..` and empty
/// segments in the remainder refuse as traversal. The anchor is
/// canonical, so the lexical suffix cannot escape through a link.
fn preview_anchor(dest: &Path, label: &str) -> Result<(PathBuf, PathBuf), OrchestratorError> {
    let mut found = None;
    for candidate in dest.ancestors() {
        match std::fs::symlink_metadata(candidate) {
            Ok(meta) if meta.is_symlink() => {
                return Err(preview_refused(label, "symlink_refused"));
            }
            Ok(_) => {
                found = Some(candidate);
                break;
            }
            Err(_) => {}
        }
    }
    let Some(anchor) = found else {
        return Err(OrchestratorError::io(
            label.to_owned(),
            "no_existing_ancestor",
        ));
    };
    let canonical = anchor
        .canonicalize()
        .map_err(|err| OrchestratorError::io(label.to_owned(), err.to_string()))?;
    let mut suffix = PathBuf::new();
    let rest = dest
        .strip_prefix(anchor)
        .map_err(|_| preview_refused(label, "traversal"))?;
    for component in rest.components() {
        match component {
            Component::Normal(part) => suffix.push(part),
            Component::CurDir => {}
            _ => return Err(preview_refused(label, "traversal")),
        }
    }
    Ok((canonical, suffix))
}

/// Refuse a preview candidate inside the repo or above it.
fn refuse_inside_or_ancestor(
    root: &Path,
    candidate: &Path,
    label: &str,
) -> Result<(), OrchestratorError> {
    if candidate == root || candidate.starts_with(root) {
        return Err(preview_refused(label, "inside_repository"));
    }
    if root.starts_with(candidate) {
        return Err(preview_refused(label, "ancestor_of_repository"));
    }
    Ok(())
}

/// Create the missing chain level by level, verifying each level.
///
/// Every level must be a real directory afterwards: a planted symlink
/// or file fails closed instead of diverting the write.
fn create_preview_chain(
    anchor: &Path,
    suffix: &Path,
    label: &str,
) -> Result<(), OrchestratorError> {
    let mut current = anchor.to_path_buf();
    for component in suffix.components() {
        current.push(component);
        drop(std::fs::create_dir(&current));
        let meta = std::fs::symlink_metadata(&current)
            .map_err(|err| OrchestratorError::io(label.to_owned(), err.to_string()))?;
        if meta.is_symlink() {
            return Err(preview_refused(label, "symlink_refused"));
        }
        if !meta.is_dir() {
            return Err(preview_refused(label, "not_a_directory"));
        }
    }
    Ok(())
}

/// Exclusively reserve `<dest>/.github`, refusing raced destinations.
///
/// The exclusive create fails closed when a peer reserved first; the
/// re-scan refuses when a peer planted anything else beside it.
fn reserve_preview_github(canonical: &Path, label: &str) -> Result<(), OrchestratorError> {
    let github = canonical.join(".github");
    match std::fs::create_dir(&github) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(preview_refused(label, "concurrent_preview"));
        }
        Err(err) => {
            return Err(OrchestratorError::io(label.to_owned(), err.to_string()));
        }
    }
    let entries = std::fs::read_dir(canonical)
        .map_err(|err| OrchestratorError::io(label.to_owned(), err.to_string()))?;
    let raced = entries
        .filter_map(Result::ok)
        .any(|entry| entry.file_name() != ".github");
    if raced {
        let _unreserved = std::fs::remove_dir(&github);
        return Err(preview_refused(label, "concurrent_preview"));
    }
    Ok(())
}
#[cfg(test)]
mod tests;
