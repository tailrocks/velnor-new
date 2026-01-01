//! Snapshot-scoped tofu read cache (T25).
//!
//! One [`FileCache`] per planning phase: the prepare-phase instance is
//! shared by detection, inventory, and diagnostics, while the plan
//! phase owns a second instance for closure and identity resolution.
//! Passes consult the cache instead of re-reading, so each file is
//! read once per phase no matter how many passes need its bytes.
//!
//! Cached reads assume the checkout is a stable snapshot for the
//! life of the owning call: a second read of the same path returns
//! the first read's outcome even if the file changed in between,
//! which keeps every pass consistent with the same bytes. Files
//! larger than [`MAX_FILE_BYTES`] read through uncached so a giant
//! file can never pin memory for the whole phase; drift-proving
//! re-reads ([`TofuLockSnapshot`]) bypass the cache by design.
//!
//! [`MAX_FILE_BYTES`]: crate::parser::MAX_FILE_BYTES
//! [`TofuLockSnapshot`]: crate::lockfile::TofuLockSnapshot

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::effective::{Dialect, config_shape};
use crate::parser::{FileModel, MAX_FILE_BYTES, MAX_FILES_PER_UNIT, parse_json, parse_native};

/// Cached raw-read failure: the kind plus the exact message.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadFailure {
    /// IO error kind (`NotFound` stays distinguishable).
    kind: std::io::ErrorKind,
    /// Original error message, rebuilt verbatim.
    message: String,
}

/// Cached outcome of one orchestrator pinned read (`read_repo_file`).
///
/// Plain data only: the orchestrator populates this compartment on
/// its secure-read path, and tofu passes never touch it. `Unreadable`
/// carries the live error's display string verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinnedOutcome {
    /// The file is absent.
    Absent,
    /// The file read within the bound.
    Text(String),
    /// The read failed; the live error's display string.
    Unreadable(String),
}

/// Shared file reads for one planning phase.
#[derive(Debug, Default)]
pub struct FileCache {
    /// Raw read outcomes by joined path.
    raw: HashMap<PathBuf, Result<Vec<u8>, ReadFailure>>,
    /// Collected unit file lists by `(root, unit)`.
    walks: HashMap<(PathBuf, String), Result<Vec<String>, String>>,
    /// Parsed config models by joined path (`None` skips the file).
    models: HashMap<PathBuf, Result<Option<FileModel>, String>>,
    /// Orchestrator pinned-read outcomes by joined path.
    pinned: HashMap<PathBuf, PinnedOutcome>,
}

impl FileCache {
    /// Empty cache: every path reads live until first cached.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bytes of one joined path, cached after the first read.
    ///
    /// Hits return the first read's bytes or an error rebuilt with
    /// the same kind and message; misses read live and cache the
    /// outcome, except oversize files (over [`MAX_FILE_BYTES`]),
    /// which read through uncached on every call. Symlinks refuse
    /// without reading (even at live targets), mirroring the
    /// orchestrator's NOFOLLOW discipline; the refusal caches like
    /// any other failure.
    ///
    /// [`MAX_FILE_BYTES`]: crate::parser::MAX_FILE_BYTES
    ///
    /// # Errors
    ///
    /// Returns the live [`std::io::Error`] when the path cannot be
    /// read (cached verbatim for later hits), or a `symlink_refused`
    /// error when the final component is a symlink.
    pub fn read_raw(&mut self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        if let Some(hit) = self.raw.get(path) {
            return hit
                .clone()
                .map_err(|failure| std::io::Error::new(failure.kind, failure.message.clone()));
        }
        if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            let refused = std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("symlink_refused:{}", path.display()),
            );
            self.raw.insert(
                path.to_path_buf(),
                Err(ReadFailure {
                    kind: refused.kind(),
                    message: refused.to_string(),
                }),
            );
            return Err(refused);
        }
        match std::fs::read(path) {
            Ok(bytes) => {
                if u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= MAX_FILE_BYTES {
                    self.raw.insert(path.to_path_buf(), Ok(bytes.clone()));
                }
                Ok(bytes)
            }
            Err(err) => {
                self.raw.insert(
                    path.to_path_buf(),
                    Err(ReadFailure {
                        kind: err.kind(),
                        message: err.to_string(),
                    }),
                );
                Err(err)
            }
        }
    }

    /// Collected unit file list, walked once per `(root, unit)`.
    ///
    /// Hits replay the first walk's list or failure string; misses
    /// delegate to [`collect_unit_files`].
    ///
    /// # Errors
    ///
    /// Returns the walk failure string when the unit cannot be listed.
    pub fn unit_files(&mut self, root: &Path, unit: &str) -> Result<Vec<String>, String> {
        if let Some(hit) = self.walks.get(&(root.to_path_buf(), unit.to_owned())) {
            return hit.clone();
        }
        let outcome = collect_unit_files(root, unit);
        self.walks
            .insert((root.to_path_buf(), unit.to_owned()), outcome.clone());
        outcome
    }

    /// Parsed config model of one repo-relative path, parsed once.
    ///
    /// Reads through the raw compartment, so bytes are shared with
    /// every other pass. `Ok(None)` skips files no dialect owns.
    ///
    /// # Errors
    ///
    /// Returns the live `unreadable:`/`malformed:` reason.
    pub fn model_for(&mut self, root: &Path, path: &str) -> Result<Option<FileModel>, String> {
        let joined = root.join(path);
        if let Some(hit) = self.models.get(&joined) {
            return hit.clone();
        }
        let outcome = self.parse_model_live(root, path);
        self.models.insert(joined, outcome.clone());
        outcome
    }

    /// Live parse backing [`FileCache::model_for`]: read, shape, parse.
    fn parse_model_live(&mut self, root: &Path, path: &str) -> Result<Option<FileModel>, String> {
        let bytes = match self.read_raw(&root.join(path)) {
            Ok(bytes) => bytes,
            Err(err) => return Err(format!("unreadable:{path}:{err}")),
        };
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(err) => return Err(format!("unreadable:{path}:{err}")),
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        let Some(shape) = config_shape(name) else {
            return Ok(None);
        };
        let model = match shape.dialect {
            Dialect::Native => parse_native(&text),
            Dialect::Json => parse_json(&text),
        }
        .map_err(|err| format!("malformed:{path}:{err}"))?;
        Ok(Some(model))
    }

    /// Consult the pinned compartment for one joined path.
    #[must_use]
    pub fn pinned(&self, path: &Path) -> Option<PinnedOutcome> {
        self.pinned.get(path).cloned()
    }

    /// Store one orchestrator pinned-read outcome.
    pub fn store_pinned(&mut self, path: PathBuf, outcome: PinnedOutcome) {
        self.pinned.insert(path, outcome);
    }
}

/// Repo-relative files under `unit`, skipping hidden directories.
///
/// Symlinks, non-UTF-8 names, unreadable entries, and over-cap
/// selections fail the walk (the caller reports unknown).
fn collect_unit_files(root: &Path, unit: &str) -> Result<Vec<String>, String> {
    let mut base: PathBuf = root.to_path_buf();
    if !unit.is_empty() {
        base.push(unit);
    }
    let mut files = Vec::new();
    let mut stack = vec![base];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .map_err(|err| format!("unreadable_dir:{}:{err}", current.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|err| format!("unreadable_entry:{}:{err}", current.display()))?;
            if entry
                .file_type()
                .map_err(|err| format!("unreadable_entry:{err}"))?
                .is_symlink()
            {
                return Err(format!("symlink_present:{}", entry.path().display()));
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "non_utf8_name".to_owned())?;
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                if stack.len() >= MAX_FILES_PER_UNIT {
                    return Err("too_many_dirs".to_owned());
                }
                stack.push(path);
                continue;
            }
            if !path.is_file() {
                continue;
            }
            let stripped = path.strip_prefix(root).map_err(|_| "escape".to_owned())?;
            let mut parts = Vec::new();
            for component in stripped.components() {
                parts.push(
                    component
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| "non_utf8_name".to_owned())?,
                );
            }
            files.push(parts.join("/"));
            if files.len() > MAX_FILES_PER_UNIT {
                return Err(format!("too_many_files:{}", files.len()));
            }
        }
    }
    files.sort();
    Ok(files)
}
