//! Read-only task-result cache helpers per the qualified mise schema.
//!
//! Covers cache modes, the `task-artifacts/v2` layout, artifact path
//! resolution, and digest verification. Nothing here executes `mise run`,
//! writes artifacts, or configures remote caching: reads only.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{canonical_json_bytes, digest_b3, validate_digest};

use crate::error::MiseError;

/// Directory holding cached task outputs inside a mise cache parent.
pub const TASK_ARTIFACTS_DIR_NAME: &str = "task-artifacts";

/// Artifact layout version kept as the final path segment.
pub const TASK_ARTIFACTS_VERSION: &str = "v2";

/// Environment variable overriding the task-cache parent directory.
pub const TASK_CACHE_DIR_ENV: &str = "MISE_TASK_CACHE_DIR";

/// Environment variable holding the default mise cache directory.
pub const CACHE_DIR_ENV: &str = "MISE_CACHE_DIR";

/// Environment variable selecting the task-cache access mode.
pub const TASK_CACHE_MODE_ENV: &str = "MISE_TASK_CACHE";

/// Qualified task-cache access modes (`--task-cache` / `MISE_TASK_CACHE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskCacheMode {
    /// Read and write cached task results.
    ReadWrite,
    /// Read cached task results without writing.
    ReadOnly,
    /// Write cached task results without reading.
    WriteOnly,
    /// Disable task-result caching.
    Off,
    /// Cache to the local directory only, never remote.
    LocalOnly,
}

impl Display for TaskCacheMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let mode = match self {
            Self::ReadWrite => "read-write",
            Self::ReadOnly => "read-only",
            Self::WriteOnly => "write-only",
            Self::Off => "off",
            Self::LocalOnly => "local-only",
        };
        write!(f, "{mode}")
    }
}

impl FromStr for TaskCacheMode {
    type Err = MiseError;

    fn from_str(mode: &str) -> Result<Self, Self::Err> {
        match mode {
            "read-write" => Ok(Self::ReadWrite),
            "read-only" => Ok(Self::ReadOnly),
            "write-only" => Ok(Self::WriteOnly),
            "off" => Ok(Self::Off),
            "local-only" => Ok(Self::LocalOnly),
            _ => Err(MiseError::UnknownCacheMode {
                mode: mode.to_owned(),
            }),
        }
    }
}

/// Resolve the artifact directory from explicit parent directories.
///
/// `MISE_TASK_CACHE_DIR` overrides the parent; otherwise the default mise
/// cache directory applies. Both keep the `task-artifacts/v2` suffix.
/// Returns `None` when neither parent is known.
#[must_use]
pub fn resolve_task_artifact_dir(
    task_cache_dir: Option<&Path>,
    cache_dir: Option<&Path>,
) -> Option<PathBuf> {
    task_cache_dir.or(cache_dir).map(|parent| {
        parent
            .join(TASK_ARTIFACTS_DIR_NAME)
            .join(TASK_ARTIFACTS_VERSION)
    })
}

/// Resolve the artifact directory from the process environment.
///
/// Reads [`TASK_CACHE_DIR_ENV`] and [`CACHE_DIR_ENV`]; see
/// [`resolve_task_artifact_dir`] for the layout rules.
#[must_use]
pub fn task_artifact_dir_from_env() -> Option<PathBuf> {
    let task_cache_dir = std::env::var_os(TASK_CACHE_DIR_ENV).map(PathBuf::from);
    let cache_dir = std::env::var_os(CACHE_DIR_ENV).map(PathBuf::from);
    resolve_task_artifact_dir(task_cache_dir.as_deref(), cache_dir.as_deref())
}

/// Read the task-cache mode from [`TASK_CACHE_MODE_ENV`].
///
/// Returns `None` when the variable is unset or empty.
///
/// # Errors
///
/// Returns [`MiseError::UnknownCacheMode`] for values outside the five
/// qualified modes.
pub fn cache_mode_from_env() -> Result<Option<TaskCacheMode>, MiseError> {
    let Some(raw) = std::env::var_os(TASK_CACHE_MODE_ENV) else {
        return Ok(None);
    };
    let mode = raw.to_string_lossy();
    if mode.is_empty() {
        return Ok(None);
    }
    TaskCacheMode::from_str(&mode).map(Some)
}

/// Join a relative artifact name onto its root, refusing escapes.
///
/// `.` segments are skipped so `a/./b` normalizes to `a/b`; anything that
/// could leave the root is rejected.
///
/// # Errors
///
/// Returns [`MiseError::ArtifactEscapesRoot`] for absolute names, names
/// with `..` segments or path prefixes, and names without artifacts.
pub fn artifact_path(root: &Path, relative: &str) -> Result<PathBuf, MiseError> {
    let candidate = Path::new(relative);
    if candidate.is_absolute() {
        return Err(escapes_root(relative));
    }
    let mut joined = PathBuf::from(root);
    let mut named = false;
    for component in candidate.components() {
        match component {
            Component::Normal(part) => {
                joined.push(part);
                named = true;
            }
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(escapes_root(relative));
            }
        }
    }
    if named {
        Ok(joined)
    } else {
        Err(escapes_root(relative))
    }
}

/// Build the shared rejection for an artifact name outside its root.
fn escapes_root(relative: &str) -> MiseError {
    MiseError::ArtifactEscapesRoot {
        path: relative.to_owned(),
    }
}

/// Read one artifact file into memory.
///
/// # Errors
///
/// Returns [`MiseError::ArtifactNotFound`] when the file does not exist
/// and [`MiseError::ArtifactUnreadable`] for any other read failure.
pub fn read_artifact_bytes(path: &Path) -> Result<Vec<u8>, MiseError> {
    std::fs::read(path).map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            MiseError::ArtifactNotFound {
                path: path.to_string_lossy().into_owned(),
            }
        } else {
            MiseError::ArtifactUnreadable {
                path: path.to_string_lossy().into_owned(),
                message: err.to_string(),
            }
        }
    })
}

/// Verify artifact bytes against an expected `b3-` digest.
///
/// # Errors
///
/// Returns [`MiseError::InvalidDigest`] for malformed digests and
/// [`MiseError::DigestMismatch`] when the bytes hash differently.
pub fn verify_artifact_digest(bytes: &[u8], expected: &str) -> Result<(), MiseError> {
    validate_digest(expected).map_err(|err| MiseError::InvalidDigest {
        value: expected.to_owned(),
        problem: err.to_string(),
    })?;
    let actual = digest_b3(bytes);
    if actual == expected {
        Ok(())
    } else {
        Err(MiseError::DigestMismatch {
            expected: expected.to_owned(),
            actual,
        })
    }
}

/// Declared cache-key inputs of one task, per the qualified schema.
///
/// Mirrors what mise hashes into a task key: source contents, declared
/// environment, command inputs (command text plus captured streams),
/// resolved tools, and dependency artifact keys. OS and architecture are
/// contributed by mise itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedTaskDescriptor {
    /// Task name as addressed by `mise run`.
    pub task_name: String,
    /// Source globs whose contents feed the key; at least one is required.
    pub sources: Vec<String>,
    /// Declared outputs restored from a cache hit.
    pub outputs: Vec<String>,
    /// Command inputs hashed with their captured stdout and stderr.
    pub command_inputs: Vec<String>,
    /// Declared environment entries (`cache.env`) feeding the key.
    pub env: BTreeMap<String, String>,
    /// Resolved `<tool>@<exact>` selectors feeding the key.
    pub tools: Vec<String>,
    /// Dependency artifact keys feeding the key.
    pub dep_keys: Vec<String>,
}

impl CachedTaskDescriptor {
    /// Reject descriptors mise could never cache (currently: no sources).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::CacheNotEligible`] when `sources` is empty.
    pub fn validate(&self) -> Result<(), MiseError> {
        if self.sources.is_empty() {
            return Err(MiseError::CacheNotEligible {
                task: self.task_name.clone(),
                reason: "task_declares_no_sources".to_owned(),
            });
        }
        Ok(())
    }

    /// Deterministic digest over the declared inputs (canonical JSON + BLAKE3).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::Contract`] when canonical serialization fails.
    pub fn cache_inputs_digest(&self) -> Result<String, MiseError> {
        self.validate()?;
        let bytes = canonical_json_bytes(self).map_err(|err| MiseError::Contract {
            problem: err.to_string(),
        })?;
        Ok(digest_b3(&bytes))
    }
}
